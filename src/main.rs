use std::sync::{mpsc, Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: &'static str },
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
            let msg = BusMessage::Telemetry {
                sensor_id: self.sensor_id,
                value: 20.0 + tick as f32,
            };
            if tx.send(msg).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        // tx is dropped here automatically when run() returns
    }
}

struct HeartbeatApp;

impl App for HeartbeatApp {
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) {
        while !shutdown.load(Ordering::Relaxed) {
            let msg = BusMessage::Heartbeat {
                app_name: "heartbeat",
            };
            if tx.send(msg).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
}

struct Bus {
    tx: mpsc::Sender<BusMessage>,
    rx: mpsc::Receiver<BusMessage>,
    shutdown: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<()>>,
}

impl Bus {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<BusMessage>();

        Bus {
            tx,
            rx,
            shutdown: Arc::new(AtomicBool::new(false)),
            handles: Vec::new(),
        }
    }

    fn register(&mut self, mut app: Box<dyn App>) {
        let tx = self.tx.clone();
        let shutdown = Arc::clone(&self.shutdown);
        let handle = thread::spawn(move || {
            app.run(tx, shutdown);
        });
        self.handles.push(handle);
    }

    fn run(self, duration: Duration) {
        let Bus{ tx, rx, shutdown, handles } = self;
        thread::sleep(duration);
        shutdown.store(true, Ordering::Relaxed);

        drop(tx);
        for msg in rx {
            println!("[bus] receive{:?}", msg);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }
}

fn main() {
    let mut bus = Bus::new();

    bus.register(Box::new(TempSensorApp{sensor_id: 1}));
    bus.register(Box::new(HeartbeatApp{}));

    bus.run(Duration::from_millis(500));
}