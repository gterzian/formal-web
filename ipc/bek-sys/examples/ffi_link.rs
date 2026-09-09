//! Link-time validation that `shim.rs`'s extern declarations match the
//! Swift shim's `@_cdecl` exports. Building this example for an iOS target
//! links the Rust declarations against `libBekShim.a`; a mismatch between
//! the two sides (name, signature, or ABI) fails the link.
//!
//! Nothing here runs — BrowserEngineKit needs a signed, entitled app on a
//! device or Simulator to launch a process. The point is that it *links*.

use bek_sys::{
    CAPABILITY_FOREGROUND, KIND_NETWORKING, grant_capability, invalidate, invalidate_grant,
    launch_process, make_xpc_connection,
};

fn main() {
    // Reference every exported symbol so the linker checks all of them.
    match launch_process(
        KIND_NETWORKING,
        Some("com.formal-web.app.NetworkingExtension"),
    ) {
        Ok(handle) => {
            let _ = unsafe { make_xpc_connection(handle, KIND_NETWORKING) };
            if let Ok(grant) =
                unsafe { grant_capability(handle, KIND_NETWORKING, CAPABILITY_FOREGROUND) }
            {
                unsafe { invalidate_grant(grant) };
            }
            unsafe { invalidate(handle, KIND_NETWORKING) };
        }
        Err(message) => println!("launch failed (expected without an entitlement): {message}"),
    }
}
