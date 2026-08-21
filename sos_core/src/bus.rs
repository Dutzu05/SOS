use core::cell::RefCell;

use critical_section::Mutex;
use heapless::{Deque, String as HString, Vec as HVec};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Max length for short identifiers: app names, command names, log sources.
pub const NAME_CAP: usize = 16;
/// Max length for a log message body.
pub const TEXT_CAP: usize = 64;
/// Max number of arguments a single command can carry.
pub const MAX_ARGS: usize = 4;
/// How many messages the bus can hold between drains. Sending onto a full
/// bus now returns an `Err` instead of silently succeeding — the old
/// `mpsc::channel()` was unbounded and could never say "no", which isn't
/// realistic for a device with finite memory.
pub const BUS_CAP: usize = 32;

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

/// The one message queue for the whole program. There's exactly one bus,
/// same as there's exactly one Scheduler — this `static` is what makes it
/// reachable from apps and network threads without anyone owning it.
static QUEUE: Mutex<RefCell<Deque<BusMessage, BUS_CAP>>> =
    Mutex::new(RefCell::new(Deque::new()));

#[derive(Clone, Copy)]
pub struct BusHandle;

impl BusHandle {
    pub fn send(&self, message: BusMessage) -> Result<(), AppError> {
        critical_section::with(|cs| {
            QUEUE
                .borrow(cs)
                .borrow_mut()
                .push_back(message)
                .map_err(|_| AppError::SendFailed("bus is full".into()))
        })
    }
}

pub struct Bus;
impl Bus {
    pub fn new() -> Self {
        Bus
    }

    pub fn handle(&self) -> BusHandle {
        BusHandle
    }

    pub fn drain(&self) -> HVec<BusMessage, BUS_CAP> {
        critical_section::with(|cs| {
            let mut queue = QUEUE.borrow(cs).borrow_mut();
            let mut out = HVec::new();
            while let Some(msg) = queue.pop_front() {
                let _ = out.push(msg); // can't fail: out's capacity == queue's
            }
            out
        })
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}
