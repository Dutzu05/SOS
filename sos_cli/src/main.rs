use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};

use sos_core::apps::{BatteryApp, HeartbeatApp, TempSensorApp};
use sos_core::{BusMessage, Scheduler};

#[derive(Parser)]
#[command(name = "sos", version, about = "Space Operating System control CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Acts as the satellite: runs the scheduler and serves telemetry/commands over TCP.
    RunSim {
        #[arg(short, long, default_value_t = 100_000)]
        ticks: u32,

        #[arg(short = 'i', long, default_value_t = 200)]
        interval_ms: u64,

        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },

    /// Acts as ground control: connects to a running satellite.
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

fn run_sim(ticks: u32, interval_ms: u64, addr: &str) {
    let mut scheduler = Scheduler::new();
    scheduler.register(Box::new(TempSensorApp::new(1)));
    scheduler.register(Box::new(HeartbeatApp));
    scheduler.register(Box::new(BatteryApp::new(2)));

    // Grab a handle before `run()` takes ownership of the scheduler.
    let bus_handle = scheduler.bus_handle();

    let listener = TcpListener::bind(addr).expect("failed to bind TCP listener");
    println!("[sim] listening on {addr}, waiting for ground control...");

    // Holds the current ground-station connection's write half.
    // `None` until someone connects; the sink checks it every tick.
    let client_writer: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    // Accept connections in the background so run-sim doesn't block
    // startup waiting for a ground station to show up.
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
                // One reader thread per connection: blocks on read_line,
                // forwarding every uplinked command onto the bus.
                thread::spawn(move || {
                    let mut reader = BufReader::new(reader_stream);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line) {
                            Ok(0) => {
                                println!("[sim] ground control disconnected");
                                break;
                            }
                            Ok(_) => match BusMessage::from_line(&line) {
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

    // Forward every bus message out to whichever ground station is
    // currently connected, one JSON line per message.
    scheduler.set_sink(move |msg| {
        let mut guard = client_writer.lock().unwrap();
        if let Some(stream) = guard.as_mut() {
            match msg.to_line() {
                Ok(line) => {
                    if let Err(e) = stream.write_all(line.as_bytes()) {
                        eprintln!("[sim] failed to send telemetry, dropping client: {e}");
                        *guard = None;
                    }
                }
                Err(e) => eprintln!("[sim] failed to serialize message: {e}"),
            }
        }
    });

    println!("[sim] running {ticks} ticks at {interval_ms}ms interval");
    scheduler.run(ticks, Duration::from_millis(interval_ms));
}

fn ground_control(addr: &str) {
    let stream = TcpStream::connect(addr).expect("failed to connect to satellite");
    println!("[ground] connected to {addr}");

    let reader_stream = stream.try_clone().expect("failed to clone stream");
    thread::spawn(move || {
        let mut reader = BufReader::new(reader_stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    println!("[ground] satellite disconnected");
                    std::process::exit(0);
                }
                Ok(_) => match BusMessage::from_line(&line) {
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
        use sos_core::{Name, MAX_ARGS, NAME_CAP};
        // (add this near the other `use` lines at the top of the file)

        let mut parts = line.split_whitespace();

        let raw_name = parts.next().unwrap_or_default();
        let name = match Name::try_from(raw_name) {
            Ok(n) => n,
            Err(_) => {
                eprintln!("[ground] command name '{raw_name}' is too long (max {NAME_CAP} chars), ignoring");
                continue;
            }
        };

        let mut args: heapless::Vec<Name, MAX_ARGS> = heapless::Vec::new();
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

        match (BusMessage::Command { name, args }).to_line() {
            Ok(out) => {
                if let Err(e) = writer.write_all(out.as_bytes()) {
                    eprintln!("[ground] failed to send command: {e}");
                    break;
                }
            }
            Err(e) => eprintln!("[ground] failed to serialize command: {e}"),
        }
    }
}