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

trait App: Send {
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>);
}

struct TempSensorApp {
    sensor_id: u8,
}

impl App for TempSensorApp {
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) {
        let mut tick = 0;

        while !shutdown.load(Ordering::Relaxed) {
            tick += 1;
            let msg = BusMessage::Telemetry { sensor_id: 1, value: 20.0 + tick as f32 };
            if tx.send(msg).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn spawn_app(mut app: Box<dyn App>, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        app.run(tx, shutdown)
    })
}

fn main() {
    let (tx, rx) = mpsc::channel::<BusMessage>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let app = Box::new(TempSensorApp { sensor_id: 1 });
    let handle = spawn_app(app, tx, Arc::clone(&shutdown));

    thread::sleep(Duration::from_millis(500));
    shutdown.store(true, Ordering::Relaxed);

    while let Ok(msg) = rx.recv() {
        println!("[main app] has {:?}", msg);
    }
    handle.join().unwrap();

}