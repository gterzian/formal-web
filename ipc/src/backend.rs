//! Backend implementations for the IPC abstraction.
//!
//! ## Backend selection
//!
//! When the `ipc-channel-backend` feature is enabled (default), all extensions
//! use ipc-channel (Unix domain sockets + Mach ports). This works on all
//! platforms.
//!
//! When the feature is disabled, only the native XPC backend is available
//! (macOS only). Only Singleton launchd services (net, media) are supported.
//! MultiInstance extensions (content) cannot use XPC because macOS AMFI
//! rejects ad-hoc-signed embedded XPC services (error -423).

use serde::{Serialize, de::DeserializeOwned};

use crate::IpcError;
use crate::types::{ExtensionClient, ExtensionManifest, ExtensionServer};

#[cfg(feature = "ipc-channel-backend")]
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
        // XPC backend only supports Singleton launchd services (net, media).
        // Content (MultiInstance) would require embedded XPC, which macOS
        // AMFI rejects for ad-hoc-signed binaries.
        match manifest.endpoint() {
            crate::types::ExtensionEndpoint::Singleton { .. } => xpc::start_extension(manifest),
            crate::types::ExtensionEndpoint::MultiInstance { .. } => {
                unimplemented!(
                    "XPC backend does not support MultiInstance (content) \
                     extensions; use --features ipc-channel-backend"
                )
            }
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
            crate::types::ExtensionEndpoint::Singleton { .. } => {
                xpc::run_extension(manifest, token, service_name)
            }
            crate::types::ExtensionEndpoint::MultiInstance { .. } => {
                unimplemented!(
                    "XPC backend does not support MultiInstance (content) \
                     extensions; use --features ipc-channel-backend"
                )
            }
        }
    }
}
