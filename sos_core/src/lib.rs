#![no_std]

mod app;
mod bus;
mod error;
mod scheduler;

pub mod apps;
mod auth;
mod protocol;
pub use protocol::{CommandOutcome, NackReason, WireMessage, WIRE_FRAME_CAP};

pub use auth::{verify, AuthToken, SHARED_SECRET, TOKEN_LEN};
pub use app::App;
pub use bus::{AppHealth, Bus, BusHandle, BusMessage, Name, Severity, Text, NAME_CAP, TEXT_CAP, MAX_ARGS, BUS_CAP, FRAME_CAP};
pub use error::AppError;
pub use scheduler::{Scheduler, MAX_APPS};

