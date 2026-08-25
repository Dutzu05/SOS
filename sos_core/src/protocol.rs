//! Wire-level envelope for messages crossing the ground-satellite link.
//!
//! Deliberately separate from `BusMessage`, the same way `auth.rs` is: apps
//! on the internal bus never see sequence numbers or acknowledgements —
//! only the socket-handling code (currently in `sos_cli`, later `sos_fw`)
//! constructs and consumes a `WireMessage`.

use heapless::Vec as HVec;
use serde::{Deserialize, Serialize};

use crate::bus::{BusMessage, Text};
use crate::error::{fmt_text, AppError};

/// Max bytes of one COBS-framed, postcard-encoded `WireMessage`. Larger
/// than `bus::FRAME_CAP` alone since every variant now also carries a
/// `seq: u32` plus the outer enum's discriminant byte.
pub const WIRE_FRAME_CAP: usize = 144;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    Bus {seq: u32, msg: BusMessage},
    Ack {seq: u32},
    Nack {seq: u32, reason: NackReason},
    CommandResult{ seq: u32, outcome: CommandOutcome },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NackReason {
    Malformed,
    UnknownCommand,
    StaleOrReplayedSeq,
    AppFault,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandOutcome {
    Success,
    Failed(Text),
}

impl WireMessage {
    /// Serialize + COBS-frame this message, same pattern as
    /// `BusMessage::to_frame` / `AuthToken::to_frame`.
    pub fn to_frame(&self) -> Result<HVec<u8, WIRE_FRAME_CAP>, AppError> {
        postcard::to_vec_cobs::<Self, WIRE_FRAME_CAP>(self)
            .map_err(|e| AppError::Serialization(fmt_text(e)))
    }

    /// Parse one COBS-framed `WireMessage` out of `buf`.
    pub fn from_frame(buf: &mut [u8]) -> Result<Self, AppError> {
        postcard::from_bytes_cobs(buf).map_err(|e| AppError::Serialization(fmt_text(e)))
    }
}