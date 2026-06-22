//! Minimal XPC bindings for formal-web.
//!
//! Provides only the XPC surface needed by the native IPC backend.
//! Only compiled on Apple targets.

#![allow(non_camel_case_types, non_snake_case)]

#[cfg(target_vendor = "apple")]
mod apple;

#[cfg(target_vendor = "apple")]
pub use apple::*;

#[cfg(not(target_vendor = "apple"))]
compile_error!("xpc-sys is only available on Apple targets");
