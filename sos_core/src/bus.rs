use core::cell::RefCell;

use critical_section::Mutex;
use heapless::{Deque, String as HString, Vec as HVec};
use serde::{Deserialize, Serialize};

use crate::error::{fmt_text, AppError};

pub const NAME_CAP: usize = 16;
pub const TEXT_CAP: usize = 64;
pub const MAX_ARGS: usize = 4;
pub const BUS_CAP: usize = 32;
/// Max bytes of one COBS-framed, postcard-encoded message on the wire —
/// comfortable headroom over our worst case (a full Command, ~90 bytes).
pub const FRAME_CAP: usize = 128;

pub type Name = HString<NAME_CAP>;
pub type Text = HString<TEXT_CAP>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: Name },
    Log { source: Name, text: Text },
    Command { name: Name, args: HVec<Name, MAX_ARGS> },
}

impl BusMessage {
    /// Serialize + COBS-frame this message into a fixed-capacity buffer,
    /// ready to write straight onto a TCP stream, a UART, or a radio.
    pub fn to_frame(&self) -> Result<HVec<u8, FRAME_CAP>, AppError> {
        postcard::to_vec_cobs::<Self, FRAME_CAP>(self).map_err(|e| AppError::Serialization(fmt_text(e)))
    }

    /// Parse one COBS-framed message out of `buf`. `buf` must hold exactly
    /// one frame, terminating 0x00 included — the caller finds that
    /// boundary (the CLI does this with `read_until(0, ..)`).
    pub fn from_frame(buf: &mut [u8]) -> Result<Self, AppError> {
        postcard::from_bytes_cobs(buf).map_err(|e| AppError::Serialization(fmt_text(e)))
    }
}

/// The one message queue for the whole program.
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
                .map_err(|_| AppError::SendFailed(Text::try_from("bus is full").unwrap()))
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
                let _ = out.push(msg);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins `FRAME_CAP` against the actual worst case instead of the eyeballed
    /// "~90 bytes" comment: a `Command` with a max-length name and `MAX_ARGS`
    /// max-length args. Fails loudly (rather than a silent `to_frame` error
    /// on hardware) if a future change to `NAME_CAP`/`MAX_ARGS` outgrows the cap.
    #[test]
    fn command_frame_fits_within_frame_cap() {
        let max_name = || {
            let bytes = [b'A'; NAME_CAP];
            Name::try_from(core::str::from_utf8(&bytes).unwrap()).unwrap()
        };

        let mut args: HVec<Name, MAX_ARGS> = HVec::new();
        for _ in 0..MAX_ARGS {
            args.push(max_name()).unwrap();
        }
        let msg = BusMessage::Command { name: max_name(), args };

        let frame = msg.to_frame().expect("worst-case Command must fit in FRAME_CAP");
        assert!(
            frame.len() <= FRAME_CAP,
            "worst-case Command frame is {} bytes, only {FRAME_CAP} bytes of headroom",
            frame.len()
        );
    }
}