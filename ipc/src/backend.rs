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

use crate::types::{
    BootstrapPayload, ExtensionClient, ExtensionHandle, ExtensionManifest, ExtensionServer,
    IpcConnection, IpcSerialize,
};

pub(crate) mod ipc_channel;

// Only on Apple when NOT using ipc-channel-backend.
#[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
pub(crate) mod xpc;

// No backend available on non-Apple without ipc-channel-backend.
#[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
compile_error!(
    "non-Apple builds require --features ipc-channel-backend \
     until a native Linux transport exists"
);

use serde::de::DeserializeOwned;

// ── Launch extension (new API) ───────────────────────────────────────────────

/// Launch an extension process and return its handle plus the first connection.
///
/// This is the standard entry point for the parent process. The `bootstrap`
/// payload carries named endpoints for additional channels (e.g., content
/// receives "net" and "media" endpoints).
pub fn launch_extension<M, Out, In>(
    manifest: &M,
    bootstrap: BootstrapPayload,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::launch_extension(manifest, bootstrap)
    }
    #[cfg(not(feature = "ipc-channel-backend"))]
    {
        match manifest.endpoint() {
            crate::types::ExtensionEndpoint::Singleton { .. } => {
                xpc::launch_extension(manifest, bootstrap)
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

// ── start_extension (legacy) ────────────────────────────────────────────────

/// Legacy: start an extension and return a client handle.
///
/// Equivalent to [`launch_extension`] with an empty bootstrap. Retained
/// for backward compatibility. New code should prefer [`launch_extension`].
pub fn start_extension<M, Out, In>(manifest: &M) -> Result<ExtensionClient<Out, In>, IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (handle, connection) = launch_extension(manifest, BootstrapPayload::new())?;
    Ok(ExtensionClient { handle, connection })
}

// ── run_extension (child side) ─────────────────────────────────────────────

/// Run as an extension process. Called by the child process on startup.
///
/// Accepts the primary bootstrap connection and any additional endpoints
/// embedded in the bootstrap payload. Returns an [`ExtensionServer`]
/// with the primary channel and any named extra channels.
pub fn run_extension<M, Out, In>(
    manifest: &M,
    token: &str,
    service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
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

use crate::IpcError;
