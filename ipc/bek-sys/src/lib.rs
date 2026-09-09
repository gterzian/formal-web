//! Raw FFI bindings for BrowserEngineKit (Apple iOS/iPadOS only).
//!
//! BrowserEngineKit is a Swift/Objective-C framework that launches a
//! browser engine's helper processes (`NetworkingProcess`,
//! `WebContentProcess`, `RenderingProcess`) under the OS's extension model —
//! no launchd plists, no `std::process::Command`. This crate is the
//! counterpart of `xpc-sys`: it exists so `ipc`'s `bek` backend can obtain a
//! raw `xpc_connection_t` from a BEK process handle, which the existing
//! `xpc_sys::XpcConnection::from_raw` wrapper then owns unmodified.
//!
//! The framework's API is Swift-only and async (a struct initializer like
//! `NetworkingProcess(bundleIdentifier:onInterruption:) async throws`, and
//! `makeLibXPCConnection() throws -> xpc_connection_t`), so the crate's FFI
//! surface is `swift-shim/BekShim.swift`, compiled by `build.rs` against the
//! real framework in the iOS/iPhoneSimulator SDKs. Compiling that shim is
//! the design check for the `bek` backend: the Rust declarations in
//! [`shim`] must match BrowserEngineKit's actual API or `swiftc` fails.
//!
//! The crate compiles on any target (off-iOS functions return transport
//! errors), but the shim is only built for iOS-family targets.

mod shim;

pub use shim::*;
