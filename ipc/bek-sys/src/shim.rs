//! C declarations and safe wrappers for the BrowserEngineKit Swift shim.
//!
//! The `extern "C"` block below mirrors the `@_cdecl` exports in
//! `swift-shim/BekShim.swift` symbol for symbol. The shim is compiled
//! against the real `BrowserEngineKit` framework by `build.rs` on iOS
//! targets, so any mismatch between these declarations and the framework's
//! actual Swift API fails that `swiftc` invocation at build time — the
//! design validation for the `bek` backend (see `ipc/ARCHITECTURE.md`).
//!
//! Ownership across the boundary:
//!
//! - A process handle returned by [`launch_process`] is a retained reference
//!   to a Swift `ProcessBox` holding the launched
//!   `NetworkingProcess`/`RenderingProcess`/`WebContentProcess` struct.
//!   [`invalidate`] stops the process and releases the box.
//! - The connection pointer returned by [`make_xpc_connection`] carries one
//!   owned reference; the caller hands it to
//!   `xpc_sys::XpcConnection::from_raw`, whose drop releases it.
//! - A grant handle from [`grant_capability`] is a retained reference to a
//!   Swift `GrantBox`; [`invalidate_grant`] releases it.
//! - Error strings are `strdup`'d on the Swift side and freed with
//!   [`free_error_string`].

#![allow(non_camel_case_types)]

#[cfg(target_os = "ios")]
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
#[cfg(target_os = "ios")]
use std::ptr;
#[cfg(target_os = "ios")]
use std::sync::mpsc;

/// Opaque handle to a launched extension process (Swift `ProcessBox`).
pub type ProcessHandle = *mut c_void;
/// Opaque handle to a live capability grant (Swift `GrantBox`).
pub type GrantHandle = *mut c_void;
/// A libxpc connection to a launched extension process, owned (+1).
pub type ConnectionHandle = *mut c_void;

/// Fired by the OS when a launched extension process dies.
pub type InterruptCallback = extern "C" fn();
/// Fires once the (async) launch completes: (process handle or null, error
/// message or null, caller context).
pub type LaunchCompletion =
    extern "C" fn(handle: *mut c_void, error_message: *mut c_char, context: *mut c_void);

/// Kind discriminants shared with `BekProcessKind` in the `ipc` crate and
/// the `BekKind` enum in `BekShim.swift`.
pub const KIND_NETWORKING: c_int = 0;
pub const KIND_RENDERING: c_int = 1;
pub const KIND_WEB_CONTENT: c_int = 2;

/// Capability discriminants shared with the `ipc` crate's
/// `ProcessCapability` and `BekCapability` in `BekShim.swift`.
pub const CAPABILITY_FOREGROUND: c_int = 0;
pub const CAPABILITY_BACKGROUND: c_int = 1;
pub const CAPABILITY_SUSPENDED: c_int = 2;

#[cfg(target_os = "ios")]
mod ffi {
    use super::*;

    unsafe extern "C" {
        pub fn bek_launch_process(
            kind: c_int,
            bundle_id: *const c_char,
            on_interrupt: InterruptCallback,
            on_complete: LaunchCompletion,
            context: *mut c_void,
        );
        pub fn bek_free_string(pointer: *mut c_char);
        pub fn bek_process_make_xpc_connection(
            handle: ProcessHandle,
            kind: c_int,
            error_message: *mut *mut c_char,
        ) -> ConnectionHandle;
        pub fn bek_process_grant_capability(
            handle: ProcessHandle,
            kind: c_int,
            capability: c_int,
            error_message: *mut *mut c_char,
        ) -> GrantHandle;
        pub fn bek_process_invalidate(handle: ProcessHandle, kind: c_int);
        pub fn bek_grant_invalidate(handle: GrantHandle);
    }
}

/// Read an error message handed back across the FFI and free it.
#[cfg(target_os = "ios")]
fn take_error_message(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: the pointer was null-checked above; the Swift side `strdup`s
    // the message, so it is a NUL-terminated string this call owns.
    let message = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the string was `strdup`ed by the shim and is freed exactly once,
    // here (the copy above no longer borrows it).
    unsafe {
        ffi::bek_free_string(pointer);
    }
    Some(message)
}

/// Launch (or find) an extension process of the given kind with the given
/// bundle ID. Blocks until BrowserEngineKit's asynchronous launch completes.
///
/// The `bundle_id` names the entitled extension target Apple should launch;
/// pass `None` to ask for the host app's default extension of that kind.
#[cfg(target_os = "ios")]
pub fn launch_process(kind: c_int, bundle_id: Option<&str>) -> Result<ProcessHandle, String> {
    let (result_tx, result_rx) = mpsc::channel::<Result<ProcessHandle, String>>();
    // The completion callback (fired exactly once from a Swift task) owns
    // this channel sender and drops it after sending.
    let context = Box::into_raw(Box::new(result_tx)) as *mut c_void;

    extern "C" fn on_interrupt() {}
    extern "C" fn on_complete(
        handle: *mut c_void,
        error_message: *mut c_char,
        context: *mut c_void,
    ) {
        // SAFETY: `context` is the `Box::into_raw` pointer from
        // `launch_process`; Swift invokes this completion exactly once, so the
        // box is reclaimed exactly once.
        let result_tx =
            unsafe { Box::from_raw(context as *mut mpsc::Sender<Result<ProcessHandle, String>>) };
        let result = if handle.is_null() {
            Err(take_error_message(error_message).unwrap_or_else(|| {
                "BrowserEngineKit failed to launch the extension process".into()
            }))
        } else {
            Ok(handle)
        };
        // The receiver side blocks in `launch_process` until this send.
        let _ = result_tx.send(result);
    }

    let bundle_cstring =
        bundle_id.map(|id| std::ffi::CString::new(id).expect("bundle id is not NUL-free"));
    let bundle_ptr = bundle_cstring
        .as_ref()
        .map_or(ptr::null(), |cstring| cstring.as_ptr());

    // SAFETY: the arguments are the enum value, a NUL-terminated bundle string
    // that outlives the call, two `extern "C"` callbacks, and the boxed sender
    // whose ownership the completion callback takes.
    unsafe {
        ffi::bek_launch_process(kind, bundle_ptr, on_interrupt, on_complete, context);
    }

    result_rx
        .recv()
        .map_err(|_| String::from("launch callback channel closed without a result"))?
}

/// Create the libxpc connection to a launched extension process. The
/// returned pointer carries one owned reference for
/// `xpc_sys::XpcConnection::from_raw`.
///
/// # Safety
///
/// `handle` must be a live process handle from [`launch_process`] of the
/// given `kind`, not yet passed to [`invalidate`].
#[cfg(target_os = "ios")]
pub unsafe fn make_xpc_connection(
    handle: ProcessHandle,
    kind: c_int,
) -> Result<ConnectionHandle, String> {
    let mut error_message: *mut c_char = ptr::null_mut();
    // SAFETY: the caller guarantees `handle` is a live process handle for
    // `kind`; `error_message` is a live out-parameter.
    let connection =
        unsafe { ffi::bek_process_make_xpc_connection(handle, kind, &mut error_message) };
    if connection.is_null() {
        Err(take_error_message(error_message)
            .unwrap_or_else(|| "BrowserEngineKit could not create a libxpc connection".into()))
    } else {
        Ok(connection)
    }
}

/// Stop the extension process and release the handle's reference to it.
///
/// # Safety
///
/// `handle` must be a live process handle from [`launch_process`] of the
/// given `kind`; it is consumed and must not be used again.
#[cfg(target_os = "ios")]
pub unsafe fn invalidate(handle: ProcessHandle, kind: c_int) {
    // SAFETY: the caller guarantees `handle` is a live process handle for
    // `kind`, consumed by the call.
    unsafe { ffi::bek_process_invalidate(handle, kind) };
}

/// Request a capability grant for a launched extension process.
///
/// # Safety
///
/// `handle` must be a live process handle from [`launch_process`] of the
/// given `kind`, not yet passed to [`invalidate`].
#[cfg(target_os = "ios")]
pub unsafe fn grant_capability(
    handle: ProcessHandle,
    kind: c_int,
    capability: c_int,
) -> Result<GrantHandle, String> {
    let mut error_message: *mut c_char = ptr::null_mut();
    // SAFETY: the caller guarantees `handle` is a live process handle for
    // `kind`; `error_message` is a live out-parameter.
    let grant =
        unsafe { ffi::bek_process_grant_capability(handle, kind, capability, &mut error_message) };
    if grant.is_null() {
        Err(take_error_message(error_message)
            .unwrap_or_else(|| "BrowserEngineKit could not grant the capability".into()))
    } else {
        Ok(grant)
    }
}

/// Invalidate a capability grant and release the handle's reference to it.
///
/// # Safety
///
/// `handle` must be a live grant handle from [`grant_capability`]; it is
/// consumed and must not be used again.
#[cfg(target_os = "ios")]
pub unsafe fn invalidate_grant(handle: GrantHandle) {
    // SAFETY: the caller guarantees `handle` is a live grant handle, consumed
    // by the call.
    unsafe { ffi::bek_grant_invalidate(handle) };
}

// ── Off-iOS fallback ──────────────────────────────────────────────────────
//
// BrowserEngineKit does not exist for classic macOS apps. These stubs keep
// the crate (and anything built on it, such as a macOS build of the
// workspace that enables `bek` by mistake) compiling on non-iOS targets;
// every operation fails at runtime with a transport error instead.

#[cfg(not(target_os = "ios"))]
pub fn launch_process(_kind: c_int, _bundle_id: Option<&str>) -> Result<ProcessHandle, String> {
    Err("BrowserEngineKit is unavailable outside iOS/iPadOS targets".into())
}

/// Off-iOS stub.
///
/// # Safety
///
/// Same contract as the iOS variant: `_handle` must be a live process
/// handle. The stub never dereferences it.
#[cfg(not(target_os = "ios"))]
pub unsafe fn make_xpc_connection(
    _handle: ProcessHandle,
    _kind: c_int,
) -> Result<ConnectionHandle, String> {
    Err("BrowserEngineKit is unavailable outside iOS/iPadOS targets".into())
}

/// Off-iOS stub.
///
/// # Safety
///
/// Same contract as the iOS variant: `_handle` must be a live process
/// handle. The stub never dereferences it.
#[cfg(not(target_os = "ios"))]
pub unsafe fn invalidate(_handle: ProcessHandle, _kind: c_int) {}

/// Off-iOS stub.
///
/// # Safety
///
/// Same contract as the iOS variant: `_handle` must be a live process
/// handle. The stub never dereferences it.
#[cfg(not(target_os = "ios"))]
pub unsafe fn grant_capability(
    _handle: ProcessHandle,
    _kind: c_int,
    _capability: c_int,
) -> Result<GrantHandle, String> {
    Err("BrowserEngineKit is unavailable outside iOS/iPadOS targets".into())
}

/// Off-iOS stub.
///
/// # Safety
///
/// Same contract as the iOS variant: `_handle` must be a live grant
/// handle. The stub never dereferences it.
#[cfg(not(target_os = "ios"))]
pub unsafe fn invalidate_grant(_handle: GrantHandle) {}
