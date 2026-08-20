use std::sync::mpsc;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: &'static str },
    Log { source: &'static str, text: String },
}

#[derive(Clone)]
pub struct BusHandle {
    tx: mpsc::Sender<BusMessage>,
}

impl BusHandle {
    pub fn send(&self, message: BusMessage) -> Result<(), AppError> {
        self.tx
            .send(message)
            .map_err(|e| AppError::SendFailed(e.to_string()))
    }
}

pub struct Bus {
    tx: mpsc::Sender<BusMessage>,
    rx: mpsc::Receiver<BusMessage>,
}

impl Bus {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Bus { tx, rx }
    }

    pub fn handle(&self) -> BusHandle {
        BusHandle { tx: self.tx.clone() }
    }
    pub fn drain(&self) ->Vec<BusMessage> {
        let mut out = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            out.push(message);
        }
        out
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}
