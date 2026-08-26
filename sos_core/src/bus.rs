use core::cell::RefCell;

use critical_section::Mutex;
use heapless::{Deque, String as HString, Vec as HVec};
use serde::{Deserialize, Serialize};

use crate::error::{fmt_text, AppError};

pub const NAME_CAP: usize = 16;
pub const TEXT_CAP: usize = 64;
pub const MAX_ARGS: usize = 4;
pub const BUS_CAP: usize = 32;
/// Ceiling on registered apps, and so on how many `AppHealth` entries a
/// `Housekeeping` message can carry. Lives here (not `scheduler.rs`, which
/// re-exports it) because `BusMessage` needs it for `Housekeeping`'s capacity.
pub const MAX_APPS: usize = 8;
/// Max bytes of one COBS-framed, postcard-encoded message on the wire —
/// comfortable headroom over our worst case, a fully-populated
/// `Housekeeping` (`MAX_APPS` entries, ~258 bytes pre-COBS). See
/// `tests::housekeeping_frame_fits_within_frame_cap`.
pub const FRAME_CAP: usize = 288;

pub type Name = HString<NAME_CAP>;
pub type Text = HString<TEXT_CAP>;

/// How urgently a `Log` message should be treated — lets ground control
/// (or a future on-board Event Service) filter/prioritize instead of every
/// log line reading as equally important.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Debug,
    Info,
    Error,
    Critical,
}

/// One app's slice of a `Housekeeping` packet — the same per-app numbers
/// the `noop` command and the tick-failure watchdog already track,
/// snapshotted in one place instead of scattered across separate `Log`
/// lines. Mirrors cFE HK: small, fixed-shape, cheap to downlink often.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHealth {
    pub name: Name,
    pub cmd_accepted: u32,
    pub cmd_rejected: u32,
    pub consecutive_tick_failures: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusMessage {
    Telemetry { sensor_id: u8, value: f32 },
    Heartbeat { app_name: Name },
    Log { severity: Severity, source: Name, text: Text },
    Command { name: Name, args: HVec<Name, MAX_ARGS> },
    Housekeeping { apps: HVec<AppHealth, MAX_APPS> },
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

    /// Pins `FRAME_CAP` against a fully-populated `Housekeeping` (`MAX_APPS`
    /// entries, each with a max-length name and maxed-out u32 counters) —
    /// the actual worst-case `BusMessage` now, bigger than `Command`. Fails
    /// loudly if `MAX_APPS` grows without `FRAME_CAP` growing to match.
    #[test]
    fn housekeeping_frame_fits_within_frame_cap() {
        let max_name = || {
            let bytes = [b'A'; NAME_CAP];
            Name::try_from(core::str::from_utf8(&bytes).unwrap()).unwrap()
        };

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
        let msg = BusMessage::Housekeeping { apps };

        let frame = msg.to_frame().expect("worst-case Housekeeping must fit in FRAME_CAP");
        assert!(
            frame.len() <= FRAME_CAP,
            "worst-case Housekeeping frame is {} bytes, only {FRAME_CAP} bytes of headroom",
            frame.len()
        );
    }
}