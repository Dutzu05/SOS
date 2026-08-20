use crate::app::App;
use crate::bus::{BusHandle, BusMessage};
use crate::error::AppError;

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
            app_name: "heartbeat",
        })
    }

    fn name(&self) -> &'static str {
        "heartbeat"
    }
}