use core::fmt;

#[derive(Debug)]
pub enum AppError {
    InitFailed(String),
    SendFailed(String),
    SensorFault(u8),
    ShutdownFailed(String),
    Serialization(String),
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