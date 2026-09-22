mod error;
mod serialize;
mod types;

pub use error::IpcError;
pub use serialize::{IpcDeserialize, IpcSerialize};
pub use types::*;

pub(crate) mod backend;

#[cfg(feature = "thread-backend")]
pub use backend::thread::register_extension_runner;
pub use backend::{launch_extension, run_extension};
pub use types::crossbeam_proxy;
