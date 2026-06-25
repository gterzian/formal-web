//! Abstract IPC API for formal-web.
//!
//! Provides a transport-neutral interface for communication between the user
//! agent process and its helper processes (content, net, media).  Backend is
//! selected by Cargo feature: `ipc-channel-backend` (default) or native XPC
//! (Apple only).
//!
//! ## Key concepts
//!
//! - [`ExtensionHandle`]: a launched extension process.  Can create multiple
//!   [`IpcConnection`]s, create [`IpcEndpoint`]s for direct child-to-child
//!   channels, and [`invalidate`](ExtensionHandle::invalidate) the process.
//! - [`IpcConnection`]: a single bidirectional channel.  Wraps [`IpcSender`] +
//!   [`IpcReceiver`].
//! - [`IpcReceiver`]: transport-agnostic receiver.  Not tied to crossbeam;
//!   call [`IpcReceiver::into_crossbeam`] if you need `select!`.
//! - [`BootstrapPayload`]: data sent on the first connection, carrying named
//!   [`IpcEndpoint`]s for additional channels (e.g., content receives "net"
//!   and "media" endpoints).
//!
//! ## Legacy API
//!
//! [`start_extension`] and [`run_extension`] are retained for backward
//! compatibility.  New code should prefer [`launch_extension`].

mod error;
mod serialize;
mod types;

pub use error::IpcError;
pub use serialize::{IpcDeserialize, IpcSerialize};
pub use types::*;

mod backend;

pub use backend::{launch_extension, run_extension, start_extension};
