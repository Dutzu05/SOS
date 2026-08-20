use std::sync::mpsc;
use std::thread;

// An enum where each variant can carry its own different data.
// This is our "message bus protocol" — every message on the bus
// must be one of these variants.
#[derive(Debug)]
enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Command { target_app: String, action: String },
    Error(String),
}

fn main() {
    let (tx, rx) = mpsc::channel::<BusMessage>();

    let sensor_app = thread::spawn(move || {
        let msg = BusMessage::Telemetry { sensor_id: 1, value: 23.7 };
        println!("[sensor_app] sending: {:?}", msg);
        tx.send(msg).unwrap();
    });

    let received = rx.recv().unwrap();

    // `match` forces you to handle every possible variant.
    // This is Rust's answer to "what if I forget to handle a message type?"
    match received {
        BusMessage::Telemetry { sensor_id, value } => {
            println!("[main_app] telemetry from sensor {}: {}", sensor_id, value);
        }
        BusMessage::Command { target_app, action } => {
            println!("[main_app] command for {}: {}", target_app, action);
        }
        BusMessage::Error(msg) => {
            println!("[main_app] error: {}", msg);
        }
    }

    sensor_app.join().unwrap();
}