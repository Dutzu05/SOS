use heapless::{String as HString, Vec as HVec};
use serde::{Deserialize, Serialize};
use std::sync::mpsc;

use crate::error::AppError;

/// Max length for short identifiers: app names, command names, log sources.
pub const NAME_CAP: usize = 16;
/// Max length for a log message body.
pub const TEXT_CAP: usize = 64;
/// Max number of arguments a single command can carry.
pub const MAX_ARGS: usize = 4;

pub type Name = HString<NAME_CAP>;
pub type Text = HString<TEXT_CAP>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: Name },
    Log { source: Name, text: Text },
    Command { name: Name, args: HVec<Name, MAX_ARGS> },
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
