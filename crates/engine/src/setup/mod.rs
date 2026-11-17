//! First-run setup and Library → Library and local AI: choose a library, a region's map and the local AI, then download,
//! check, unpack and switch, so that a working library is never broken. The contract with the interface is
//! docs/SETUP-API.md. Ported from the original Lastbook's engine/setup/.

pub mod api;
pub mod download;
pub mod hardware;
pub mod install;
pub mod manifest;
pub mod net;
pub mod volumes;
#[cfg(test)]
pub mod testserver;
#[cfg(test)]
mod api_tests;

use serde_json::{Map, Value};

/// Why a step stopped. Each carries what the interface shows: a code from docs/SETUP-API.md and a plain sentence.
#[derive(Debug, Clone)]
pub enum SetupError {
    /// The person paused or cancelled: "paused" or "cancelled".
    Stopped(String),
    /// A failure to show, with its code (no_space, network, checksum, found_mismatch...) and anything more to say.
    Failed { code: String, message: String, extra: Map<String, Value> },
    /// The downloaded file did not match its published SHA-256.
    BadChecksum(String),
    /// A pack, map or library that does not check out (codes unpack, activate).
    Refused { code: String, message: String },
}

impl SetupError {
    pub fn failed(code: &str, message: impl Into<String>) -> SetupError {
        SetupError::Failed { code: code.into(), message: message.into(), extra: Map::new() }
    }

    pub fn refused(code: &str, message: impl Into<String>) -> SetupError {
        SetupError::Refused { code: code.into(), message: message.into() }
    }

    pub fn code(&self) -> &str {
        match self {
            SetupError::Stopped(why) => why,
            SetupError::Failed { code, .. } | SetupError::Refused { code, .. } => code,
            SetupError::BadChecksum(_) => "checksum",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SetupError::Stopped(why) => why.clone(),
            SetupError::Failed { message, .. } | SetupError::Refused { message, .. } | SetupError::BadChecksum(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message(), self.code())
    }
}

/// `12.3 GB`, or `123 GB` from 10 GB up, in powers of ten as the original Lastbook wrote them.
pub fn gb(n: u64) -> String {
    let g = n as f64 / 1e9;
    if g >= 10.0 {
        format!("{g:.0} GB")
    } else {
        format!("{g:.1} GB")
    }
}
