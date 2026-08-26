use core::fmt;

use embedded_hal::delay::DelayNs;
use heapless::Vec as HVec;

use crate::app::App;
use crate::apps::AnyApp;
use crate::bus::{Bus, BusHandle, BusMessage, Name, Severity, Text};
use crate::error::fmt_text;
use crate::protocol::CommandOutcome;

pub const MAX_APPS: usize = 8;

/// Consecutive ticks with no `BusMessage::Heartbeat` observed on the bus
/// before the scheduler raises a fault. Only starts counting after the
/// first heartbeat is seen — if nothing registered ever sends one, the
/// watchdog stays silent rather than assuming one was expected.
const HEARTBEAT_WATCHDOG_TICKS: u32 = 5;

pub struct Scheduler<F: FnMut(&BusMessage), G: FnMut(&CommandOutcome)> {
    bus: Bus,
    apps: HVec<AnyApp, MAX_APPS>,
    sink: F,
    command_result_sink: G,
    ticks_since_heartbeat: Option<u32>,
}

impl<F: FnMut(&BusMessage), G: FnMut(&CommandOutcome)> Scheduler<F, G> {
    pub fn new(sink: F, command_result_sink: G) -> Self {
        Scheduler {
            bus: Bus::new(),
            apps: HVec::new(),
            sink,
            command_result_sink,
            ticks_since_heartbeat: None,
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
                report_fault(&bus_handle, Severity::Error, app.name(), &e);
            }
        }

        for tick_num in 1..=ticks {
            for app in self.apps.iter_mut() {
                if let Err(e) = app.tick(&bus_handle) {
                    report_fault(&bus_handle, Severity::Error, app.name(), format_args!("tick {tick_num}: {e}"));
                }
            }

            let mut heartbeat_seen_this_tick = false;

            for msg in self.bus.drain() {
                if matches!(msg, BusMessage::Heartbeat { .. }) {
                    heartbeat_seen_this_tick = true;
                }
                if let BusMessage::Command { name, args } = &msg {
                    // The satellite shouts commands to every app at once, so
                    // the command as a whole only succeeds if every app that
                    // recognizes it does — one failing app fails the result.
                    let mut failed = false;
                    for app in self.apps.iter_mut() {
                        if let Err(e) = app.handle_command(name.as_str(), args.as_slice()) {
                            report_fault(&bus_handle, Severity::Error, app.name(), &e);
                            failed = true;
                        }
                    }
                    let outcome = if failed {
                        CommandOutcome::Failed(Text::try_from("one or more apps rejected the command").unwrap_or_default())
                    } else {
                        CommandOutcome::Success
                    };
                    (self.command_result_sink)(&outcome);
                }
                (self.sink)(&msg);
            }

            if heartbeat_seen_this_tick {
                self.ticks_since_heartbeat = Some(0);
            } else if let Some(missed) = self.ticks_since_heartbeat.as_mut() {
                *missed += 1;
                if *missed == HEARTBEAT_WATCHDOG_TICKS {
                    report_fault(
                        &bus_handle,
                        Severity::Critical,
                        "watchdog",
                        format_args!("no heartbeat in {HEARTBEAT_WATCHDOG_TICKS} ticks"),
                    );
                }
            }

            delay.delay_ms(tick_interval_ms);
        }

        for app in self.apps.iter_mut() {
            if let Err(e) = app.shutdown() {
                report_fault(&bus_handle, Severity::Error, app.name(), &e);
            }
        }
    }
}

impl Scheduler<fn(&BusMessage), fn(&CommandOutcome)> {
    /// A scheduler with no telemetry or command-result sink. Bus traffic
    /// (including the fault `Log` messages below) still happens, it just
    /// isn't forwarded anywhere outside the process.
    pub fn without_sink() -> Self {
        fn no_op_bus(_msg: &BusMessage) {}
        fn no_op_result(_outcome: &CommandOutcome) {}
        Self::new(no_op_bus, no_op_result)
    }
}

/// Turns an app error into a bus `Log` message instead of a local
/// `eprintln!` — there's no stdout without an OS, and this is what the
/// bus already exists for. Also fixes the earlier gap where faults were
/// only ever visible in the satellite's own terminal.
fn report_fault(bus_handle: &BusHandle, severity: Severity, app_name: &'static str, context: impl fmt::Display) {
    let _ = bus_handle.send(BusMessage::Log {
        severity,
        source: Name::try_from(app_name).unwrap_or_default(),
        text: fmt_text(context),
    });
}