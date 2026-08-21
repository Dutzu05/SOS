use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use embedded_hal::delay::DelayNs;
use heapless::Vec as HVec;

use sos_core::apps::{AnyApp, BatteryApp, HeartbeatApp, TempSensorApp};
use sos_core::{BusMessage, Name, Scheduler, MAX_ARGS, NAME_CAP};

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

    // The sink is needed at construction time now, so it's built before
    // the scheduler exists rather than bolted on afterward.
    let sink_writer = Arc::clone(&client_writer);
    let mut scheduler = Scheduler::new(move |msg: &BusMessage| {
        log_to_console(msg);
        let mut guard = sink_writer.lock().unwrap();
        if let Some(stream) = guard.as_mut() {
            match msg.to_frame() {
                Ok(frame) => {
                    if let Err(e) = stream.write_all(&frame) {
                        eprintln!("[sim] failed to send telemetry, dropping client: {e}");
                        *guard = None;
                    }
                }
                Err(e) => eprintln!("[sim] failed to serialize message: {e}"),
            }
        }
    });

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
                *client_writer.lock().unwrap() = Some(stream);

                let bus_handle = bus_handle.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(reader_stream);
                    let mut frame = Vec::new();
                    loop {
                        frame.clear();
                        match reader.read_until(0u8, &mut frame) {
                            Ok(0) => {
                                println!("[sim] ground control disconnected");
                                break;
                            }
                            Ok(_) => match BusMessage::from_frame(&mut frame) {
                                Ok(msg) => {
                                    if let Err(e) = bus_handle.send(msg) {
                                        eprintln!("[sim] failed to route command onto bus: {e}");
                                    }
                                }
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

fn ground_control(addr: &str) {
    let stream = TcpStream::connect(addr).expect("failed to connect to satellite");
    println!("[ground] connected to {addr}");

    let reader_stream = stream.try_clone().expect("failed to clone stream");
    thread::spawn(move || {
        let mut reader = BufReader::new(reader_stream);
        let mut frame = Vec::new();
        loop {
            frame.clear();
            match reader.read_until(0u8, &mut frame) {
                Ok(0) => {
                    println!("[ground] satellite disconnected");
                    std::process::exit(0);
                }
                Ok(_) => match BusMessage::from_frame(&mut frame) {
                    Ok(msg) => println!("[ground] <- {msg:?}"),
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

        match (BusMessage::Command { name, args }).to_frame() {
            Ok(frame) => {
                if let Err(e) = writer.write_all(&frame) {
                    eprintln!("[ground] failed to send command: {e}");
                    break;
                }
            }
            Err(e) => eprintln!("[ground] failed to serialize command: {e}"),
        }
    }
}