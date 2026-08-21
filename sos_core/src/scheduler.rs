use embedded_hal::delay::DelayNs;
use heapless::Vec as HVec;

use crate::app::App;
use crate::apps::AnyApp;
use crate::bus::{Bus, BusHandle, BusMessage};

/// Max number of apps a single Scheduler can hold. Bump this if you
/// register more — heapless containers need their capacity fixed up front.
pub const MAX_APPS: usize = 8;

/*struct Registered {
    name: String,
    app: Box<dyn App>,
}*/

pub struct Scheduler {
    bus: Bus,
    apps: HVec<AnyApp, MAX_APPS>,
    sink: Option<Box<dyn FnMut(&BusMessage) + Send>>, // still std-only — next step
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            bus: Bus::new(),
            apps: HVec::new(),
            sink: None,
        }
    }

    pub fn register(&mut self, app: AnyApp) {
        self.apps
            .push(app)
            .unwrap_or_else(|_| panic!("too many apps registered (max {MAX_APPS}) — raise MAX_APPS"));
    }

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

    pub fn run(mut self, ticks: u32, tick_interval_ms: u32, delay: &mut impl DelayNs) {
        let bus_handle = self.bus.handle();

        for app in self.apps.iter_mut() {
            if let Err(e) = app.init() {
                eprintln!("[scheduler] '{}' failed to init: {e}", app.name());
            }
        }

        for tick_num in 1..=ticks {
            for app in self.apps.iter_mut() {
                if let Err(e) = app.tick(&bus_handle) {
                    eprintln!("[scheduler] '{}' tick {tick_num} error: {e}", app.name());
                }
            }

            for msg in self.bus.drain() {
                log_message(&msg);
                if let Some(sink) = self.sink.as_mut() {
                    sink(&msg);
                }
            }

            delay.delay_ms(tick_interval_ms);
        }

        for app in self.apps.iter_mut() {
            if let Err(e) = app.shutdown() {
                eprintln!("[scheduler] '{}' failed to shut down: {e}", app.name());
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