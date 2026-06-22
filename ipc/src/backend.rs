//! Backend implementations for the IPC abstraction.
//!
//! Selects between `ipc-channel` and native XPC based on Cargo features.

#[cfg(feature = "ipc-channel-backend")]
mod ipc_channel;

#[cfg(feature = "ipc-channel-backend")]
pub use ipc_channel::{run_extension, start_extension};

#[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
mod native;

#[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
pub use native::{run_extension, start_extension};

#[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
compile_error!(
    "non-Apple builds require --features ipc-channel-backend \
     until a native Linux transport exists"
);
