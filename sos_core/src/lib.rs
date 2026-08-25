#![no_std]

mod app;
mod bus;
mod error;
mod scheduler;

pub mod apps;
mod auth;
pub use auth::{verify, AuthToken, SHARED_SECRET, TOKEN_LEN};
pub use app::App;
pub use bus::{Bus, BusHandle, BusMessage, Name, Text, NAME_CAP, TEXT_CAP, MAX_ARGS, BUS_CAP, FRAME_CAP};
pub use error::AppError;
pub use scheduler::{Scheduler, MAX_APPS};

    