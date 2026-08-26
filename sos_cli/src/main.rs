use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use embedded_hal::delay::DelayNs;
use heapless::Vec as HVec;

use sos_core::apps::{AnyApp, BatteryApp, HeartbeatApp, TempSensorApp};
use sos_core::{BusMessage, CommandOutcome, Name, Scheduler, WireMessage, MAX_ARGS, NAME_CAP};

#[derive(Parser)]
#[command(name = "sos", version, about = "Space Operating System control CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    RunSim {
        #[arg(short, long, default_value_t = 100_000)]
        ticks: u32,

        #[arg(short = 'i', long, default_value_t = 200)]
        interval_ms: u64,

        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },
    GroundControl {
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::RunSim { ticks, interval_ms, addr } => run_sim(ticks, interval_ms, &addr),
        Command::GroundControl { addr } => ground_control(&addr),
    }
}

/// Turns `std::thread::sleep` into the `DelayNs` interface `Scheduler::run`
/// expects. On real ESP32 hardware, this gets swapped for a HAL timer —
/// `sos_core` itself never changes.
struct HostDelay;

impl DelayNs for HostDelay {
    fn delay_ns(&mut self, ns: u32) {
        thread::sleep(Duration::from_nanos(ns as u64));
    }
}

fn send_wire(client_writer: &Arc<Mutex<Option<TcpStream>>>, wire: &WireMessage) {
    let mut guard = client_writer.lock().unwrap();
    if let Some(stream) = guard.as_mut() {
        match wire.to_frame() {
            Ok(frame) => {
                if let Err(e) = stream.write_all(&frame) {
                    eprintln!("[sim] failed to send, dropping client: {e}");
                    *guard = None;
                }
            }
            Err(e) => eprintln!("[sim] failed to serialize message: {e}"),
        }
    }
}

/// What `sos_core`'s old `log_message` used to do — living here now
/// because "print to a terminal" is a host capability, not something the
/// portable core should assume.
fn log_to_console(msg: &BusMessage) {
    match msg {
        BusMessage::Telemetry { sensor_id, value } => {
            println!("[bus] telemetry sensor={sensor_id} value={value:.2}");
        }
        BusMessage::Heartbeat { app_name } => {
            println!("[bus] heartbeat from {app_name}");
        }
        BusMessage::Log { source, text } => {
            println!("[bus] log [{source}] {text}");
        }
        BusMessage::Command { name, args } => {
            println!("[bus] command received: {name} {args:?}");
        }
    }
}

fn run_sim(ticks: u32, interval_ms: u64, addr: &str) {
    let listener = TcpListener::bind(addr).expect("failed to bind TCP listener");
    println!("[sim] listening on {addr}, waiting for ground control...");

    let client_writer: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    // Sequence numbers are a wire-layer concept `sos_core` never sees (see
    // `protocol.rs`), so the pairing between an inbound command and its
    // eventual result lives here, not in the `Scheduler`. The network
    // thread pushes a `seq` here only once its `Command` is successfully
    // enqueued on the bus; the scheduler emits outcomes in the same FIFO
    // order it received the commands, so popping the front always matches.
    let pending_command_seqs: Arc<Mutex<VecDeque<u32>>> = Arc::new(Mutex::new(VecDeque::new()));

    // The sinks are needed at construction time now, so they're built
    // before the scheduler exists rather than bolted on afterward.
    let sink_writer = Arc::clone(&client_writer);
    let mut downlink_seq: u32 = 0;
    let result_writer = Arc::clone(&client_writer);
    let result_seqs = Arc::clone(&pending_command_seqs);
    let mut scheduler = Scheduler::new(
        move |msg: &BusMessage| {
            log_to_console(msg);
            downlink_seq = downlink_seq.wrapping_add(1);
            send_wire(&sink_writer, &WireMessage::Bus { seq: downlink_seq, msg: msg.clone() });
        },
        move |outcome: &CommandOutcome| {
            match result_seqs.lock().unwrap().pop_front() {
                Some(seq) => send_wire(&result_writer, &WireMessage::CommandResult { seq, outcome: outcome.clone() }),
                None => eprintln!("[sim] got a command outcome with no pending seq to pair it with"),
            }
        },
    );

    scheduler.register(AnyApp::TempSensor(TempSensorApp::new(1)));
    scheduler.register(AnyApp::Heartbeat(HeartbeatApp));
    scheduler.register(AnyApp::Battery(BatteryApp::new(2)));

    let bus_handle = scheduler.bus_handle();

    {
        let client_writer = Arc::clone(&client_writer);
        thread::spawn(move || {
            for incoming in listener.incoming() {
                let stream = match incoming {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[sim] failed to accept connection: {e}");
                        continue;
                    }
                };
                println!("[sim] ground control connected: {:?}", stream.peer_addr());

                let reader_stream = stream.try_clone().expect("failed to clone stream");

                let bus_handle = bus_handle.clone();
                let client_writer = Arc::clone(&client_writer);
                let pending_command_seqs = Arc::clone(&pending_command_seqs);
                thread::spawn(move || {
                    let mut reader = BufReader::new(reader_stream);
                    let mut frame = Vec::new();

                    // First frame on a fresh connection must be the shared-secret
                    // auth token — nothing else is accepted until it checks out.
                    // Only wire this connection up as the telemetry/log sink
                    // (`client_writer`) after it authenticates, so a failed
                    // attempt never becomes the thing bus messages get written to.
                    match reader.read_until(0u8, &mut frame) {
                        Ok(0) => {
                            println!("[sim] ground control disconnected before authenticating");
                            return;
                        }
                        Ok(_) => match sos_core::AuthToken::from_frame(&mut frame) {
                            Ok(token) if sos_core::verify(&token.0) => {
                                println!("[sim] ground control authenticated");
                            }
                            _ => {
                                let _ = bus_handle.send(BusMessage::Log {
                                    source: Name::try_from("auth").unwrap(),
                                    text: sos_core::Text::try_from("rejected connection: bad auth token").unwrap(),
                                });
                                // Silent drop — no response sent to the caller, since
                                // telling an attacker "wrong secret" is itself a leak.
                                return;
                            }
                        },
                        Err(e) => {
                            eprintln!("[sim] read error while awaiting auth: {e}");
                            return;
                        }
                    }

                    *client_writer.lock().unwrap() = Some(stream);
                    let mut last_uplink_seq: Option<u32> = None;

                    loop {
                        frame.clear();
                        match reader.read_until(0u8, &mut frame) {
                            Ok(0) => {
                                println!("[sim] ground control disconnected");
                                break;
                            }
                            Ok(_) => match WireMessage::from_frame(&mut frame) {
                                Ok(WireMessage::Bus { seq, msg }) => {
                                    let is_stale = last_uplink_seq.is_some_and(|last| seq <= last);
                                    if is_stale {
                                        eprintln!("[sim] rejecting seq {seq}: stale or replayed");
                                        send_wire(&client_writer, &WireMessage::Nack {
                                            seq,
                                            reason: sos_core::NackReason::StaleOrReplayedSeq,
                                        });
                                    } else {
                                        let is_command = matches!(msg, BusMessage::Command { .. });
                                        match bus_handle.send(msg) {
                                            Ok(()) => {
                                                last_uplink_seq = Some(seq);
                                                if is_command {
                                                    pending_command_seqs.lock().unwrap().push_back(seq);
                                                }
                                                send_wire(&client_writer, &WireMessage::Ack { seq });
                                            }
                                            Err(e) => {
                                                // Bus is full — a transient failure, not a
                                                // rejection. Don't advance `last_uplink_seq`
                                                // so a retransmit of this same `seq` is
                                                // accepted (not treated as stale/replayed),
                                                // and don't Ack a command that never actually
                                                // got enqueued.
                                                eprintln!("[sim] failed to route command onto bus: {e}");
                                                send_wire(&client_writer, &WireMessage::Nack {
                                                    seq,
                                                    reason: sos_core::NackReason::BusFull,
                                                });
                                            }
                                        }
                                    }
                                }
                                Ok(other) => eprintln!("[sim] unexpected message from ground control: {other:?}"),
                                Err(e) => eprintln!("[sim] bad message from ground control: {e}"),
                            },
                            Err(e) => {
                                eprintln!("[sim] read error: {e}");
                                break;
                            }
                        }
                    }
                });
            }
        });
    }

    println!("[sim] running {ticks} ticks at {interval_ms}ms interval");
    let mut delay = HostDelay;
    scheduler.run(ticks, interval_ms as u32, &mut delay);
}

/// How long ground control waits for an `Ack`/`Nack` before assuming the
/// frame was lost and resending it. Retries reuse the same `seq`, which is
/// safe: the satellite either hasn't seen it yet (fine, executes once) or
/// has already advanced past it and responds `Nack(StaleOrReplayedSeq)`
/// (which ground control reads as "already delivered", not a failure).
const ACK_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_SEND_ATTEMPTS: u32 = 4;

enum AckOutcome {
    Ack,
    Nack(sos_core::NackReason),
}

struct PendingAck {
    seq: u32,
    outcome: Option<AckOutcome>,
}

/// Called from the reader thread when an `Ack`/`Nack` arrives. Only
/// resolves it if it's for the `seq` the stdin loop is currently waiting
/// on — a late reply for an earlier, already-resolved attempt is ignored.
fn resolve_ack(ack_state: &(Mutex<PendingAck>, Condvar), seq: u32, outcome: AckOutcome) {
    let (lock, cvar) = ack_state;
    let mut pending = lock.lock().unwrap();
    if pending.seq == seq && pending.outcome.is_none() {
        pending.outcome = Some(outcome);
        cvar.notify_all();
    }
}

fn ground_control(addr: &str) {
    // 1. Notice 'mut' is added here so we can write to it!
    let mut stream = TcpStream::connect(addr).expect("failed to connect to satellite");
    println!("[ground] connected to {addr}");

    // 2. Send the AuthToken as the very first action
    let frame = sos_core::AuthToken(sos_core::SHARED_SECRET)
        .to_frame()
        .expect("failed to serialize auth token");
    std::io::Write::write_all(&mut stream, &frame).expect("failed to send auth token");
    println!("[ground] auth token sent");

    // 3. NOW it is safe to clone the stream and start the listening thread

    // Tracks the one command currently awaiting delivery confirmation, so
    // the stdin loop can block on it and the reader thread can resolve it
    // when an `Ack`/`Nack` for that `seq` comes back.
    let ack_state: Arc<(Mutex<PendingAck>, Condvar)> =
        Arc::new((Mutex::new(PendingAck { seq: 0, outcome: None }), Condvar::new()));

    let reader_stream = stream.try_clone().expect("failed to clone stream");
    let reader_ack_state = Arc::clone(&ack_state);
    thread::spawn(move || {
        let mut reader = BufReader::new(reader_stream);
        let mut frame = Vec::new();
        let mut last_seq: Option<u32> = None;
        loop {
            frame.clear();
            match reader.read_until(0u8, &mut frame) {
                Ok(0) => {
                    println!("[ground] satellite disconnected");
                    std::process::exit(0);
                }
                Ok(_) => match WireMessage::from_frame(&mut frame) {
                    Ok(WireMessage::Bus { seq, msg }) => {
                        if let Some(last) = last_seq {
                            let expected = last.wrapping_add(1);
                            if seq != expected {
                                println!("[ground] !! sequence gap: expected {expected}, got {seq}");
                            }
                        }
                        last_seq = Some(seq);
                        println!("[ground] <- #{seq} {msg:?}");
                    }
                    Ok(WireMessage::Ack { seq }) => {
                        println!("[ground] <- ACK #{seq}");
                        resolve_ack(&reader_ack_state, seq, AckOutcome::Ack);
                    }
                    Ok(WireMessage::Nack { seq, reason }) => {
                        println!("[ground] <- NACK #{seq}: {reason:?}");
                        resolve_ack(&reader_ack_state, seq, AckOutcome::Nack(reason));
                    }
                    Ok(WireMessage::CommandResult { seq, outcome }) => {
                        println!("[ground] <- result #{seq}: {outcome:?}")
                    }
                    Err(e) => eprintln!("[ground] bad message from satellite: {e}"),
                },
                Err(e) => {
                    eprintln!("[ground] read error: {e}");
                    break;
                }
            }
        }
    });

    println!("[ground] type a command (e.g. `reset-battery`) and press enter:");
    let mut writer = stream;
    let mut uplink_seq: u32 = 0;
    for line in std::io::stdin().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[ground] stdin error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();

        let raw_name = parts.next().unwrap_or_default();
        let name = match Name::try_from(raw_name) {
            Ok(n) => n,
            Err(_) => {
                eprintln!("[ground] command name '{raw_name}' is too long (max {NAME_CAP} chars), ignoring");
                continue;
            }
        };

        let mut args: HVec<Name, MAX_ARGS> = HVec::new();
        for arg in parts {
            match Name::try_from(arg) {
                Ok(a) => {
                    if args.push(a).is_err() {
                        eprintln!("[ground] too many arguments (max {MAX_ARGS}), dropping the rest");
                        break;
                    }
                }
                Err(_) => eprintln!("[ground] argument '{arg}' is too long (max {NAME_CAP} chars), skipping"),
            }
        }

        uplink_seq = uplink_seq.wrapping_add(1);
        let wire = WireMessage::Bus {
            seq: uplink_seq,
            msg: BusMessage::Command { name, args },
        };
        let frame = match wire.to_frame() {
            Ok(frame) => frame,
            Err(e) => {
                eprintln!("[ground] failed to serialize command: {e}");
                continue;
            }
        };

        {
            let (lock, _) = &*ack_state;
            *lock.lock().unwrap() = PendingAck { seq: uplink_seq, outcome: None };
        }

        let mut attempt = 0;
        loop {
            attempt += 1;
            if let Err(e) = writer.write_all(&frame) {
                eprintln!("[ground] failed to send command: {e}");
                return;
            }

            let (lock, cvar) = &*ack_state;
            let pending = lock.lock().unwrap();
            let (pending, wait_result) = cvar
                .wait_timeout_while(pending, ACK_TIMEOUT, |p| p.outcome.is_none())
                .unwrap();

            match &pending.outcome {
                Some(AckOutcome::Ack) => {
                    println!("[ground] #{uplink_seq} delivered");
                    break;
                }
                Some(AckOutcome::Nack(sos_core::NackReason::StaleOrReplayedSeq)) => {
                    println!("[ground] #{uplink_seq} already delivered on an earlier attempt");
                    break;
                }
                Some(AckOutcome::Nack(sos_core::NackReason::BusFull)) => {
                    if attempt >= MAX_SEND_ATTEMPTS {
                        println!("[ground] #{uplink_seq} bus still full after {attempt} attempts, giving up");
                        break;
                    }
                    println!("[ground] #{uplink_seq} bus full, retrying (attempt {}/{MAX_SEND_ATTEMPTS})", attempt + 1);
                    drop(pending);
                    let (lock, _) = &*ack_state;
                    lock.lock().unwrap().outcome = None;
                    continue;
                }
                Some(AckOutcome::Nack(reason)) => {
                    println!("[ground] #{uplink_seq} rejected: {reason:?}");
                    break;
                }
                None => {
                    debug_assert!(wait_result.timed_out());
                    if attempt >= MAX_SEND_ATTEMPTS {
                        println!("[ground] #{uplink_seq} got no response after {attempt} attempts, giving up");
                        break;
                    }
                    println!("[ground] #{uplink_seq} timed out waiting for ACK, retrying (attempt {}/{MAX_SEND_ATTEMPTS})", attempt + 1);
                    drop(pending);
                    continue;
                }
            }
        }
    }
}