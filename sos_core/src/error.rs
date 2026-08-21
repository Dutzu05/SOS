use core::fmt;
use core::fmt::Write as _;

use crate::bus::Text;

#[derive(Debug)]
pub enum AppError {
    InitFailed(Text),
    SendFailed(Text),
    SensorFault(u8),
    ShutdownFailed(Text),
    Serialization(Text),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::InitFailed(msg) => write!(f, "init failed: {msg}"),
            AppError::SendFailed(msg) => write!(f, "failed to send message on bus: {msg}"),
            AppError::SensorFault(id) => write!(f, "sensor {id} reported a fault"),
            AppError::ShutdownFailed(msg) => write!(f, "shutdown did not complete cleanly: {msg}"),
            AppError::Serialization(msg) => write!(f, "(de)serialization error: {msg}"),
        }
    }
}

impl core::error::Error for AppError {}

/// Formats any `Display`-able value into a fixed-capacity `Text`, since
/// `format!`/`String` aren't available without an allocator. Overflow is
/// silently truncated — a clipped message beats no message.
pub(crate) fn fmt_text<E: fmt::Display>(e: E) -> Text {
    let mut text = Text::new();
    let _ = write!(text, "{e}");
    text
}