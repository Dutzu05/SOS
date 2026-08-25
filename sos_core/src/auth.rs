//! Connection-level authentication handshake.
//!
//! Deliberately kept separate from `BusMessage` — this is a transport-layer
//! gate, not app data. A forged or malformed auth attempt should never be
//! able to reach the internal app bus.

use heapless::Vec;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::error::{fmt_text, AppError};

pub const TOKEN_LEN: usize = 16;

/// Baked in at compile time from a git-ignored local file (see
/// `secrets/README.md`) rather than a literal, so the real key never lands
/// in version control. If the file is missing or isn't exactly
/// `TOKEN_LEN` bytes, this fails to compile — there is no default.
pub const SHARED_SECRET: [u8; TOKEN_LEN] = *include_bytes!("../../secrets/shared.key");

const FRAME_CAP: usize = 32;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AuthToken(pub [u8; TOKEN_LEN]);

impl AuthToken {
    /// Serialize + COBS-encode this token into a 0x00-delimited frame,
    /// mirroring `BusMessage::to_frame` in `bus.rs`.
    pub fn to_frame(&self) -> Result<Vec<u8, FRAME_CAP>, AppError> {
        postcard::to_vec_cobs::<Self, FRAME_CAP>(self).map_err(|e| AppError::Serialization(fmt_text(e)))
    }

    /// Decode a 0x00-delimited COBS frame (including the trailing 0x00)
    /// back into an `AuthToken`.
    pub fn from_frame(frame: &mut [u8]) -> Result<Self, AppError> {
        postcard::from_bytes_cobs(frame).map_err(|e| AppError::Serialization(fmt_text(e)))
    }
}

/// Constant-time comparison against the shared secret.
/// Using `subtle` instead of `==` so a byte-by-byte mismatch doesn't leak
/// timing information about how many leading bytes matched.
pub fn verify(candidate: &[u8; TOKEN_LEN]) -> bool {
    candidate.ct_eq(&SHARED_SECRET).into()
}