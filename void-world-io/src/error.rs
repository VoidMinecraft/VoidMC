//! Error type for world persistence.

use std::fmt;

/// Errors produced by the persistence layer.
#[derive(Debug)]
pub enum PersistenceError {
    /// Underlying filesystem I/O failure.
    Io(std::io::Error),
    /// The on-disk data could not be decoded (corrupt file, bad NBT, etc).
    Corrupt(String),
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistenceError::Io(e) => write!(f, "world io error: {e}"),
            PersistenceError::Corrupt(msg) => write!(f, "corrupt chunk data: {msg}"),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        PersistenceError::Io(e)
    }
}

/// Convenience alias for persistence results.
pub type Result<T> = std::result::Result<T, PersistenceError>;
