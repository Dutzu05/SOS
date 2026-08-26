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

/// Per-app command accept/reject counts — the cFS convention for a cheap
/// "is this app's command path even alive" check, independent of what any
/// particular command does. `accepted` counts every `handle_command` call
/// that returned `Ok`, including ones the app didn't specifically
/// recognize (the default `App::handle_command` impl is a no-op `Ok`) —
/// that's what makes a plain, unhandled `"noop"` command a meaningful
/// liveness probe for every app without each one needing its own case for it.
#[derive(Default, Clone, Copy)]
struct CommandCounters {
    accepted: u32,
    rejected: u32,
}

/// Command name that exercises every app's `handle_command` path without
/// asking any of them to actually do anything — see `CommandCounters`.
const NOOP_COMMAND: &str = "noop";

pub struct Scheduler<F: FnMut(&BusMessage), G: FnMut(&CommandOutcome)> {
    bus: Bus,
    apps: HVec<AnyApp, MAX_APPS>,
    counters: HVec<CommandCounters, MAX_APPS>,
    sink: F,
    command_result_sink: G,
    ticks_since_heartbeat: Option<u32>,
}

impl<F: FnMut(&BusMessage), G: FnMut(&CommandOutcome)> Scheduler<F, G> {
    pub fn new(sink: F, command_result_sink: G) -> Self {
        Scheduler {
            bus: Bus::new(),
            apps: HVec::new(),
            counters: HVec::new(),
            sink,
            command_result_sink,
            ticks_since_heartbeat: None,
        }
    }

    pub fn register(&mut self, app: AnyApp) {
        self.apps
            .push(app)
            .unwrap_or_else(|_| panic!("too many apps registered (max {MAX_APPS}) — raise MAX_APPS"));
        // Always pushed in lockstep with `apps` so the two stay index-aligned.
        let _ = self.counters.push(CommandCounters::default());
    }

    pub fn bus_handle(&self) -> BusHandle {
        self.bus.handle()
    }

    pub fn run(mut self, ticks: u32, tick_interval_ms: u32, delay: &mut impl DelayNs) {
        let bus_handle = self.bus.handle();

        for app in self.apps.iter_mut() {
            if let Err(e) = app.init() {
                log_event(&bus_handle, Severity::Error, app.name(), &e);
            }
        }

        for tick_num in 1..=ticks {
            for app in self.apps.iter_mut() {
                if let Err(e) = app.tick(&bus_handle) {
                    log_event(&bus_handle, Severity::Error, app.name(), format_args!("tick {tick_num}: {e}"));
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
                    for (app, counters) in self.apps.iter_mut().zip(self.counters.iter_mut()) {
                        match app.handle_command(name.as_str(), args.as_slice()) {
                            Ok(()) => counters.accepted += 1,
                            Err(e) => {
                                counters.rejected += 1;
                                log_event(&bus_handle, Severity::Error, app.name(), &e);
                                failed = true;
                            }
                        }
                    }
                    let outcome = if failed {
                        CommandOutcome::Failed(Text::try_from("one or more apps rejected the command").unwrap_or_default())
                    } else {
                        CommandOutcome::Success
                    };
                    (self.command_result_sink)(&outcome);

                    if name.as_str() == NOOP_COMMAND {
                        for (app, counters) in self.apps.iter().zip(self.counters.iter()) {
                            log_event(
                                &bus_handle,
                                Severity::Info,
                                app.name(),
                                format_args!("noop: accepted={} rejected={}", counters.accepted, counters.rejected),
                            );
                        }
                    }
                }
                (self.sink)(&msg);
            }

            if heartbeat_seen_this_tick {
                self.ticks_since_heartbeat = Some(0);
            } else if let Some(missed) = self.ticks_since_heartbeat.as_mut() {
                *missed += 1;
                if *missed == HEARTBEAT_WATCHDOG_TICKS {
                    log_event(
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
                log_event(&bus_handle, Severity::Error, app.name(), &e);
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

/// Puts an event (a fault, a watchdog trip, a noop confirmation) onto the
/// bus as a `Log` message instead of a local `eprintln!` — there's no
/// stdout without an OS, and this is what the bus already exists for. Also
/// means events are visible on the ground, not just the satellite's own
/// terminal.
fn log_event(bus_handle: &BusHandle, severity: Severity, app_name: &'static str, context: impl fmt::Display) {
    let _ = bus_handle.send(BusMessage::Log {
        severity,
        source: Name::try_from(app_name).unwrap_or_default(),
        text: fmt_text(context),
    });
}