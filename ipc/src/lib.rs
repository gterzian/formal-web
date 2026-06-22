//! Abstract IPC API for formal-web.
//!
//! Provides a transport-neutral interface for communication between the user agent
//! process and its helper processes (content, net, media). Backend is selected by
//! Cargo feature: `ipc-channel-backend` (default) or native XPC (Apple only).

mod error;
mod serialize;
mod types;

pub use error::IpcError;
pub use serialize::{IpcDeserialize, IpcSerialize};
pub use types::*;

mod backend;

pub use backend::{run_extension, start_extension};
