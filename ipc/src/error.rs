//! Error types for the IPC abstraction.

use std::fmt;

/// Errors that can occur during IPC operations.
#[derive(Debug)]
pub enum IpcError {
    /// The connection was closed or disconnected.
    Disconnected,
    /// An error occurred during serialization.
    Serialize(String),
    /// An error occurred during deserialization.
    Deserialize(String),
    /// A transport-level error occurred.
    Transport(String),
}

impl fmt::Display for IpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::Disconnected => write!(formatter, "IPC connection closed"),
            IpcError::Serialize(message) => write!(formatter, "IPC serialization error: {message}"),
            IpcError::Deserialize(message) => {
                write!(formatter, "IPC deserialization error: {message}")
            }
            IpcError::Transport(message) => write!(formatter, "IPC transport error: {message}"),
        }
    }
}

impl std::error::Error for IpcError {}
