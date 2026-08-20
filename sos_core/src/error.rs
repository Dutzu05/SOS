use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("init failed: {0}")]
    InitFailed(String),

    #[error("failed to send message on bus: {0}")]
    SendFailed(String),

    #[error("sensor {0} reported a fault")]
    SensorFault(u8),

    #[error("shutdown did not complete cleanly: {0}")]
    ShutdownFailed(String),

    #[error("(de)serialization error: {0}")]
    Serialization(String),
}

