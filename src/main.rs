use std::any::Any;
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
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) -> Result<(), String>;
}

struct TempSensorApp {
    sensor_id: u8,
}

impl App for TempSensorApp {
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) -> Result<(), String> {
        let mut tick = 0;
        while !shutdown.load(Ordering::Relaxed) {
            tick += 1;
            let msg = BusMessage::Telemetry {
                sensor_id: self.sensor_id,
                value: 20.0 + tick as f32,
            };
            // .send() returns Result<(), SendError<T>>. The `?` here means:
            // if Ok, continue; if Err, immediately return that Err from run(),
            // converted via map_err into our String error type.
            tx.send(msg).map_err(|e| format!("send failed: {e}"))?;
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
}

struct HeartbeatApp;

impl App for HeartbeatApp {
    fn run(&mut self, tx: mpsc::Sender<BusMessage>, shutdown: Arc<AtomicBool>) -> Result<(), String> {
        while !shutdown.load(Ordering::Relaxed) {
            let msg = BusMessage::Heartbeat {
                app_name: "heartbeat",
            };
            tx.send(msg).map_err(|e| format!("send failed: {e}"))?;
            thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    }
}

struct Bus {
    tx: mpsc::Sender<BusMessage>,
    rx: mpsc::Receiver<BusMessage>,
    shutdown: Arc<AtomicBool>,
    // Store the app's name alongside its handle, purely so error/panic
    // messages can say *which* app failed instead of just "an app failed".
    handles: Vec<(String, thread::JoinHandle<Result<(), String>>)>,
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

    fn register(&mut self, name: &str, mut app: Box<dyn App>) {
        let tx = self.tx.clone();
        let shutdown = Arc::clone(&self.shutdown);

        let handle = thread::spawn(move || app.run(tx, shutdown));

        self.handles.push((name.to_string(), handle));
    }

    fn run(self, duration: Duration) {
        let Bus { tx, rx, shutdown, handles } = self;

        thread::sleep(duration);
        shutdown.store(true, Ordering::Relaxed);
        drop(tx);

        for msg in rx {
            println!("[bus] received: {:?}", msg);
        }

        for (name, handle) in handles {
            // handle.join() -> Result<Result<(), String>, Box<dyn Any + Send>>
            // Outer Result: did the thread panic?
            // Inner Result: did the app's run() return an error on its own?
            match handle.join() {
                Ok(Ok(())) => {
                    println!("[bus] app '{name}' shut down cleanly");
                }
                Ok(Err(app_err)) => {
                    println!("[bus] app '{name}' reported an error: {app_err}");
                }
                Err(panic_payload) => {
                    println!("[bus] app '{name}' PANICKED: {}", describe_panic(&panic_payload));
                }
            }
        }
    }
}

// Panic payloads are `Box<dyn Any + Send>` because Rust doesn't know ahead
// of time what type a panic! carries — usually it's &str or String, so we
// downcast to check for those specifically, falling back to a generic message.
fn describe_panic(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn main() {
    let mut bus = Bus::new();

    bus.register("temp_sensor", Box::new(TempSensorApp { sensor_id: 1 }));
    bus.register("heartbeat", Box::new(HeartbeatApp));

    bus.run(Duration::from_millis(500));
}