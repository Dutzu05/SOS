use std::time::Duration;

use clap::{Parser, Subcommand};

use sos_core::apps::{HeartbeatApp, TempSensorApp};
use sos_core::Scheduler;

#[derive(Parser)]
#[command(name = "sos", version, about = "Space Operating System control CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Run {
        #[arg(short, long, default_value_t = 10)]
        ticks: u32,

        #[arg(short = 'i', long, default_value_t = 200)]
        interval_ms: u64,
    },

    Status,
}
fn main(){
    let cli = Cli::parse();
    match cli.command {
        Command::Run{ticks, interval_ms} => {
            let mut scheduler = Scheduler::new();
            scheduler.register(Box::new(TempSensorApp::new(1)));
            scheduler.register(Box::new(HeartbeatApp));

            println!("[cli] running {ticks} ticks at {interval_ms}ms interval");
            scheduler.run(ticks, Duration::from_millis(interval_ms));
        }

        Command::Status => {
            println!("[cli] status is a stub for now — this becomes real once the bus is live (Phase 5).");
        }
    }
}
