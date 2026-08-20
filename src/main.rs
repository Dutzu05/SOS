use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

// An enum where each variant can carry its own different data.
// This is our "message bus protocol" — every message on the bus
// must be one of these variants.
#[derive(Debug)]
enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
}

fn main() {
    let (tx, rx) = mpsc::channel::<BusMessage>();

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_for_app = Arc::clone(&shutdown);

    let sensor_app = thread::spawn(move || {
        let mut tick = 0;

        while !shutdown_for_app.load(Ordering::Relaxed) {
            tick+=1;
            let msg = BusMessage::Telemetry {sensor_id: 1, value: 20.0 + tick as f32};
            if tx.send(msg).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        println!("Shutting down...");
    });
    thread::sleep(Duration::from_millis(500));
    shutdown.store(true, Ordering::Relaxed);

    while let Ok(msg) = rx.try_recv() {
        println!("[main app] received:{:?}", msg);
    }
    sensor_app.join().unwrap();

}