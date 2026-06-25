use crate::types::{
    ExtensionHandle, ExtensionManifest, ExtensionServer, IpcConnection, IpcSerialize,
};
use crate::IpcError;
use serde::de::DeserializeOwned;

pub(crate) mod ipc_channel;

#[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
pub(crate) mod xpc;

#[cfg(all(not(feature = "ipc-channel-backend"), not(target_vendor = "apple")))]
compile_error!(
    "non-Apple builds require --features ipc-channel-backend \
     until a native Linux transport exists"
);

/// Launch an extension process and return its handle plus the first connection.
pub fn launch_extension<M, Out, In>(
    manifest: &M,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::launch_extension(manifest)
    }
    #[cfg(not(feature = "ipc-channel-backend"))]
    {
        match manifest.endpoint() {
            crate::types::ExtensionEndpoint::Singleton { .. } => xpc::launch_extension(manifest),
            crate::types::ExtensionEndpoint::MultiInstance { .. } => {
                unimplemented!("XPC backend does not support MultiInstance (content) extensions")
            }
        }
    }
}

/// Run an extension process.
///
/// Connects to the parent's bootstrap rendezvous, establishes the IPC channel,
/// then calls `run` with the resulting [`ExtensionServer`].  The `run`
/// callback implements the business logic of the extension (event loop,
/// request handling, etc.).
///
/// This is the entry point for the child process — called from `main()` or
/// from a C FFI bridge on BEK.
pub fn run_extension<Out, In>(
    token: &str,
    run: impl FnOnce(ExtensionServer<In, Out>) -> Result<(), String>,
) -> Result<(), String>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let server = bootstrap_extension::<Out, In>(token)
        .map_err(|error| format!("ipc bootstrap failed: {error}"))?;
    run(server)
}

fn bootstrap_extension<Out, In>(
    token: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::run_extension::<Out, In>(token)
    }
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    {
        xpc::run_extension::<Out, In>(token)
    }
}
