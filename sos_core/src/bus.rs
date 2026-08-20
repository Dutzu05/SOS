use std::sync::mpsc;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: String },
    Log { source: String, text: String },
    Command { name: String, args: Vec<String> },
}

impl BusMessage { //Serialize into a sg line of JSON and ready to write into a single line TCP
    pub fn to_line(&self) -> Result<String, AppError> {
        let mut json =
            serde_json::to_string(self).map_err(|e| AppError::Serialization(e.to_string()))?;
        json.push('\n');
        Ok(json)
    }

    //Parse a line produce by to line into BusMessage

    pub fn from_line(line: &str) -> Result<Self, AppError> {
        serde_json::from_str(line.trim()).map_err(|e| AppError::Serialization(e.to_string()))
    }
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
