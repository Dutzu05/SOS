use std::thread;
use std::time::Duration;

use crate::app::App;
use crate::bus::{Bus, BusHandle, BusMessage};

struct Registered {
    name: String,
    app: Box<dyn App>,
}

pub struct Scheduler {
    bus: Bus,
    apps: Vec<Registered>,
    sink: Option<Box<dyn FnMut(&BusMessage) + Send>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            bus: Bus::new(),
            apps: Vec::new(),
            sink: None,
        }
    }

    pub fn register(&mut self, app: Box<dyn App>) {
        let name = app.name().to_string();
        self.apps.push(Registered { name, app });
    }

    /// Get a cloneable handle to the bus before starting the run loop.
    /// External I/O (like a TCP reader thread) uses this to inject
    /// messages — e.g. uplinked commands — onto the bus from outside.
    pub fn bus_handle(&self) -> BusHandle {
        self.bus.handle()
    }

    /// Register a callback invoked for every message drained from the
    /// bus each tick, in addition to the normal console log. This is
    /// how we forward telemetry out over the network without Scheduler
    /// needing to know anything about sockets.
    pub fn set_sink<F>(&mut self, sink: F)
    where
        F: FnMut(&BusMessage) + Send + 'static,
    {
        self.sink = Some(Box::new(sink));
    }

    pub fn run(mut self, ticks: u32, tick_interval: Duration) {
        let bus_handle = self.bus.handle();

        for reg in &mut self.apps {
            if let Err(e) = reg.app.init() {
                eprintln!("[scheduler] '{}' failed to init: {e}", reg.name);
            }
        }

        for tick_num in 1..=ticks {
            for reg in &mut self.apps {
                if let Err(e) = reg.app.tick(&bus_handle) {
                    eprintln!("[scheduler] '{}' tick {tick_num} error: {e}", reg.name);
                }
            }

            for msg in self.bus.drain() {
                log_message(&msg);
                if let Some(sink) = self.sink.as_mut() {
                    sink(&msg);
                }
            }

            thread::sleep(tick_interval);
        }

        for reg in &mut self.apps {
            if let Err(e) = reg.app.shutdown() {
                eprintln!("[scheduler] '{}' failed to shut down: {e}", reg.name);
            }
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

fn log_message(msg: &BusMessage) {
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