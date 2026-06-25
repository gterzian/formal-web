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
///
/// The parent side: creates the bootstrap rendezvous, spawns the child
/// process via [`ExtensionManifest::spawn`], and waits for the child to
/// connect.
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

/// Run as an extension process. Called by the child process on startup.
///
/// The child side: connects back to the parent's bootstrap rendezvous and
/// returns an [`ExtensionServer`] with the established channel pair.
///
/// No manifest needed — the child doesn't spawn anything, it only
/// connects.  The transport backend is selected at compile time.
pub fn run_extension<Out, In>(
    token: &str,
    service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::run_extension::<Out, In>(token, service_name)
    }
    #[cfg(all(not(feature = "ipc-channel-backend"), target_vendor = "apple"))]
    {
        xpc::run_extension::<Out, In>(token, service_name)
    }
}
