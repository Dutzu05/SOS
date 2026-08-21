use core::fmt;

use embedded_hal::delay::DelayNs;
use heapless::Vec as HVec;

use crate::app::App;
use crate::apps::AnyApp;
use crate::bus::{Bus, BusHandle, BusMessage, Name};
use crate::error::fmt_text;

pub const MAX_APPS: usize = 8;

pub struct Scheduler<F: FnMut(&BusMessage)> {
    bus: Bus,
    apps: HVec<AnyApp, MAX_APPS>,
    sink: F,
}

impl<F: FnMut(&BusMessage)> Scheduler<F> {
    pub fn new(sink: F) -> Self {
        Scheduler {
            bus: Bus::new(),
            apps: HVec::new(),
            sink,
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

    pub fn run(mut self, ticks: u32, tick_interval_ms: u32, delay: &mut impl DelayNs) {
        let bus_handle = self.bus.handle();

        for app in self.apps.iter_mut() {
            if let Err(e) = app.init() {
                report_fault(&bus_handle, app.name(), &e);
            }
        }

        for tick_num in 1..=ticks {
            for app in self.apps.iter_mut() {
                if let Err(e) = app.tick(&bus_handle) {
                    report_fault(&bus_handle, app.name(), format_args!("tick {tick_num}: {e}"));
                }
            }

            for msg in self.bus.drain() {
                (self.sink)(&msg);
            }

            delay.delay_ms(tick_interval_ms);
        }

        for app in self.apps.iter_mut() {
            if let Err(e) = app.shutdown() {
                report_fault(&bus_handle, app.name(), &e);
            }
        }
    }
}

impl Scheduler<fn(&BusMessage)> {
    /// A scheduler with no telemetry sink. Bus traffic (including the
    /// fault `Log` messages below) still happens, it just isn't forwarded
    /// anywhere outside the process.
    pub fn without_sink() -> Self {
        fn no_op(_msg: &BusMessage) {}
        Self::new(no_op)
    }
}

/// Turns an app error into a bus `Log` message instead of a local
/// `eprintln!` — there's no stdout without an OS, and this is what the
/// bus already exists for. Also fixes the earlier gap where faults were
/// only ever visible in the satellite's own terminal.
fn report_fault(bus_handle: &BusHandle, app_name: &'static str, context: impl fmt::Display) {
    let _ = bus_handle.send(BusMessage::Log {
        source: Name::try_from(app_name).unwrap_or_default(),
        text: fmt_text(context),
    });
}