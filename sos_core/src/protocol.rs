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
/// `seq: u32` plus the outer enum's discriminant byte. The actual
/// worst-case size (currently well under this cap) is pinned by
/// `tests::every_wire_message_variant_fits_within_wire_frame_cap` below, so
/// this headroom is measured, not eyeballed.
///
/// Sized for the current TCP transport, which has no practical MTU limit.
/// `sos_fw`'s eventual radio/BLE link will: default BLE ATT MTU is 23 bytes
/// (20 usable) unless both sides negotiate a larger one, so a single
/// `WireMessage` frame at this size will need fragmentation/reassembly at
/// that transport's edge — not handled here yet.
pub const WIRE_FRAME_CAP: usize = 320;

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
    BusFull,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{AppHealth, Name, NAME_CAP, MAX_APPS, MAX_ARGS, TEXT_CAP};
    use heapless::Vec as HVec;

    fn max_name() -> Name {
        let bytes = [b'A'; NAME_CAP];
        Name::try_from(core::str::from_utf8(&bytes).unwrap()).unwrap()
    }

    fn max_text() -> Text {
        let bytes = [b'A'; TEXT_CAP];
        Text::try_from(core::str::from_utf8(&bytes).unwrap()).unwrap()
    }

    fn max_command() -> BusMessage {
        let mut args: HVec<Name, MAX_ARGS> = HVec::new();
        for _ in 0..MAX_ARGS {
            args.push(max_name()).unwrap();
        }
        BusMessage::Command { name: max_name(), args }
    }

    fn max_housekeeping() -> BusMessage {
        let mut apps: HVec<AppHealth, MAX_APPS> = HVec::new();
        for _ in 0..MAX_APPS {
            apps.push(AppHealth {
                name: max_name(),
                cmd_accepted: u32::MAX,
                cmd_rejected: u32::MAX,
                consecutive_tick_failures: u32::MAX,
            })
            .unwrap();
        }
        BusMessage::Housekeeping { apps }
    }

    /// Pins `WIRE_FRAME_CAP` against the actual worst case across every
    /// `WireMessage` variant, rather than the hand-estimated comment above —
    /// in particular a `Bus` frame wrapping a fully-populated
    /// `Housekeeping`, now the biggest thing on the wire. Fails loudly if a
    /// future change (bigger `MAX_APPS`/`TEXT_CAP`, a new variant, more
    /// args) outgrows the cap, instead of `to_frame` quietly erroring on
    /// hardware later.
    #[test]
    fn every_wire_message_variant_fits_within_wire_frame_cap() {
        let variants = [
            WireMessage::Bus { seq: u32::MAX, msg: max_command() },
            WireMessage::Bus { seq: u32::MAX, msg: max_housekeeping() },
            WireMessage::Ack { seq: u32::MAX },
            WireMessage::Nack { seq: u32::MAX, reason: NackReason::BusFull },
            WireMessage::CommandResult { seq: u32::MAX, outcome: CommandOutcome::Failed(max_text()) },
        ];

        for variant in &variants {
            let frame = variant.to_frame().expect("worst-case WireMessage must fit in WIRE_FRAME_CAP");
            assert!(
                frame.len() <= WIRE_FRAME_CAP,
                "worst-case {variant:?} frame is {} bytes, only {WIRE_FRAME_CAP} bytes of headroom",
                frame.len()
            );
        }
    }
}