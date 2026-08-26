use crate::app::App;
use crate::bus::{BusHandle, BusMessage, Name, Severity};
use crate::error::{fmt_text, AppError};

pub struct TempSensorApp {
    sensor_id: u8,
    tick_count: u32,
}

impl TempSensorApp {
    pub fn new(sensor_id: u8) -> Self {
        TempSensorApp {
            sensor_id,
            tick_count: 0,
        }
    }
}

impl App for TempSensorApp {
    fn init(&mut self) -> Result<(), AppError> {
        self.tick_count = 0;
        Ok(())
    }

    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError> {
        self.tick_count += 1;
        let value = 20.0 + self.tick_count as f32;
        bus.send(BusMessage::Telemetry {
            sensor_id: self.sensor_id,
            value,
        })
    }

    fn name(&self) -> &'static str {
        "temp_sensor"
    }
}

pub struct HeartbeatApp;

impl App for HeartbeatApp {
    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError> {
        bus.send(BusMessage::Heartbeat {
            app_name: Name::try_from("heartbeat").unwrap(),
        })
    }

    fn name(&self) -> &'static str {
        "heartbeat"
    }
}

pub struct BatteryApp {
    sensor_id: u8,
    charge_level: f32,
    drain_rate: f32,
    fault_threshold: f32,
}

impl BatteryApp {
    pub fn new(sensor_id: u8) -> Self {
        BatteryApp {
            sensor_id,
            charge_level: 100.0, // Starts fully charged
            drain_rate: 5.5,     // How much it drains per tick
            fault_threshold: 15.0, // The limit this app watches for
        }
    }
}impl App for BatteryApp {
    fn init(&mut self) -> Result<(), AppError> {
        self.charge_level = 100.0;
        Ok(())
    }

    /// A minimal fault-detection-isolation-recovery loop: this is the one
    /// limit this app watches (`fault_threshold`), and crossing it doesn't
    /// just get reported and left for a human — the app issues the same
    /// `reset-battery` command ground control could send by hand, right
    /// here, autonomously. The `Err` still surfaces the breach as a normal
    /// tick fault (so the scheduler's per-app health tracking sees it too);
    /// the correction has already happened by the time it does.
    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError> {
        self.charge_level -= self.drain_rate;

        if self.charge_level < self.fault_threshold {
            let breach_level = self.charge_level;
            let _ = self.handle_command("reset-battery", &[]);
            let _ = bus.send(BusMessage::Log {
                severity: Severity::Critical,
                source: Name::try_from("battery_app").unwrap_or_default(),
                text: fmt_text(format_args!(
                    "charge {breach_level:.2} below limit {:.2} — auto-corrected via reset-battery",
                    self.fault_threshold
                )),
            });
            return Err(AppError::SensorFault(self.sensor_id));
        }

        bus.send(BusMessage::Telemetry {
            sensor_id: self.sensor_id,
            value: self.charge_level,
        })
    }

    fn handle_command(&mut self, name: &str, _args: &[Name]) -> Result<(), AppError> {
        if name == "reset-battery" {
            self.charge_level = 100.0;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "battery_app"
    }
}

/// Every concrete app type the scheduler can run, wrapped in one enum.
/// This is the heapless answer to `Box<dyn App>`: no heap allocation, but
/// every variant has to be listed here by hand — the real cost of the
/// full-heapless path over `no_std` + `alloc`.
pub enum AnyApp {
    TempSensor(TempSensorApp),
    Heartbeat(HeartbeatApp),
    Battery(BatteryApp),
}

impl App for AnyApp {
    fn init(&mut self) -> Result<(), AppError> {
        match self {
            AnyApp::TempSensor(a) => a.init(),
            AnyApp::Heartbeat(a) => a.init(),
            AnyApp::Battery(a) => a.init(),
        }
    }

    fn tick(&mut self, bus: &BusHandle) -> Result<(), AppError> {
        match self {
            AnyApp::TempSensor(a) => a.tick(bus),
            AnyApp::Heartbeat(a) => a.tick(bus),
            AnyApp::Battery(a) => a.tick(bus),
        }
    }

    fn shutdown(&mut self) -> Result<(), AppError> {
        match self {
            AnyApp::TempSensor(a) => a.shutdown(),
            AnyApp::Heartbeat(a) => a.shutdown(),
            AnyApp::Battery(a) => a.shutdown(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            AnyApp::TempSensor(a) => a.name(),
            AnyApp::Heartbeat(a) => a.name(),
            AnyApp::Battery(a) => a.name(),
        }
    }

    fn handle_command(&mut self, name: &str, args: &[Name]) -> Result<(), AppError> {
        match self {
            AnyApp::TempSensor(a) => a.handle_command(name, args),
            AnyApp::Heartbeat(a) => a.handle_command(name, args),
            AnyApp::Battery(a) => a.handle_command(name, args),
        }
    }
}
