//! Backend implementations for the IPC abstraction.
//!
//! ## Backend selection
//!
//! When the `ipc-channel-backend` feature is enabled, all extensions use
//! ipc-channel (Unix domain sockets + Mach ports). This works on all platforms.
//!
//! When the feature is disabled (default), a mixed backend is used:
//!
//! | Extension | Endpoint      | Backend      | Transport          |
//! |-----------|---------------|--------------|--------------------|
//! | net       | Singleton     | XPC          | launchd Mach service |
//! | media     | Singleton     | XPC          | launchd Mach service |
//! | content   | MultiInstance | ipc-channel  | Unix domain socket   |
//!
//! Content cannot use embedded XPC services because macOS AMFI rejects
//! ad-hoc-signed binaries in XPCServices/ (error -423: "The file is adhoc
//! signed or signed by an unknown certificate chain"). A paid Apple
//! Developer certificate would be required.
//!
//! The ipc-channel module is always compiled so it is available for content
//! in mixed mode.

use serde::{Serialize, de::DeserializeOwned};

use crate::IpcError;
use crate::types::{ExtensionClient, ExtensionEndpoint, ExtensionManifest, ExtensionServer};

// Always compiled — used by content in mixed mode, and all extensions in
// ipc-channel-backend mode.
mod ipc_channel;

// Only on Apple when NOT using ipc-channel-backend.
#[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
mod xpc;

// No backend available on non-Apple without ipc-channel-backend.
#[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
compile_error!(
    "non-Apple builds require --features ipc-channel-backend \
     until a native Linux transport exists"
);

// ── start_extension ─────────────────────────────────────────────────────────

pub fn start_extension<M, Out, In>(manifest: &M) -> Result<ExtensionClient<Out, In>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::start_extension(manifest)
    }
    #[cfg(not(feature = "ipc-channel-backend"))]
    {
        match manifest.endpoint() {
            ExtensionEndpoint::Singleton { .. } => xpc::start_extension(manifest),
            ExtensionEndpoint::MultiInstance { .. } => ipc_channel::start_extension(manifest),
        }
    }
}

// ── run_extension ───────────────────────────────────────────────────────────

pub fn run_extension<M, Out, In>(
    manifest: &M,
    token: &str,
    service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::run_extension(manifest, token, service_name)
    }
    #[cfg(not(feature = "ipc-channel-backend"))]
    {
        match manifest.endpoint() {
            ExtensionEndpoint::Singleton { .. } => {
                xpc::run_extension(manifest, token, service_name)
            }
            ExtensionEndpoint::MultiInstance { .. } => {
                ipc_channel::run_extension(manifest, token, service_name)
            }
        }
    }
}
