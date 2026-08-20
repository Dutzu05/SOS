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
    let tx2 = tx.clone();
    let temp_sensor_app = thread::spawn(move || {
        let msg = BusMessage::Telemetry { sensor_id: 1, value: 1.0 };
        tx.send(msg).unwrap();
    });

    let nav_app = thread::spawn(move || {
        let msg = BusMessage::Command  {
            target_app: "thruster_app".to_string(),
            action: "burn".to_string(),
        };
        tx2.send(msg).unwrap();
    });

    temp_sensor_app.join().unwrap();
    nav_app.join().unwrap();

    while let Ok(msg) = rx.recv() {
        match msg {
            BusMessage::Telemetry { sensor_id, value } => {
                println!("[{:?}] {:?}", sensor_id, value)
            }
            BusMessage::Command { target_app, action } => {
                println!("[{:?}] {:?}", target_app, action);
            }
            BusMessage::Error(msg) => {
                println!("[{:?}] {:?}", msg, msg);
            }
        }
    }
}