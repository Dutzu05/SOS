use std::sync::mpsc;
use std::thread;

// An enum where each variant can carry its own different data.
// This is our "message bus protocol" — every message on the bus
// must be one of these variants.
#[derive(Debug)]
enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
}

fn read_sensor(sensor_id: u8) -> Result<BusMessage, String> {
    if (sensor_id == 0) {
        return Err(format!("Sensor {} does not exist!", sensor_id));
    }
    Ok(BusMessage::Telemetry { sensor_id, value: 1.0 })
}

fn main() {
    let (tx, rx) = mpsc::channel::<BusMessage>();
    let sensor_app = thread::spawn(move || {
        for id in [1, 0] {
            match read_sensor(id) {
                Ok(msg) => {
                    tx.send(msg).unwrap();
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
    });
    sensor_app.join().unwrap();

    while let Ok(msg) = rx.recv() {
        println!("{:?}", msg);
    }
}