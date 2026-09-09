use crate::IpcError;
use crate::types::{
    ExtensionHandle, ExtensionManifest, ExtensionServer, IpcConnection, IpcSerialize,
};
use serde::de::DeserializeOwned;

#[cfg(feature = "ipc-channel-backend")]
pub(crate) mod ipc_channel;

#[cfg(feature = "bek")]
pub(crate) mod bek;

#[cfg(all(not(feature = "ipc-channel-backend"), not(feature = "bek")))]
compile_error!(
    "no IPC backend enabled: enable the ipc-channel-backend feature, \
     or the bek feature (BrowserEngineKit, iOS/iPadOS)"
);

/// Launch an extension process and return its handle plus the first connection.
///
/// The backend is selected at compile time: `ipc-channel` (default) spawns
/// the extension binary with `ExtensionManifest::spawn`; `bek` asks
/// BrowserEngineKit (via `ExtensionManifest::bek_target`) to launch the OS
/// extension process instead.
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
    #[cfg(all(not(feature = "ipc-channel-backend"), feature = "bek"))]
    {
        bek::launch_extension(manifest)
    }
    #[cfg(all(not(feature = "ipc-channel-backend"), not(feature = "bek")))]
    {
        let _ = manifest;
        Err(IpcError::Transport(
            "no IPC backend enabled: enable the ipc-channel-backend feature, \
             or the bek feature on iOS/iPadOS"
                .into(),
        ))
    }
}

/// Run an extension process.
///
/// On the `bek` backend the OS launches the extension and delivers its
/// connection through BrowserEngineKit — there is no argv-token bootstrap to
/// run — so this entry point returns a transport error there.
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

fn bootstrap_extension<Out, In>(token: &str) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    #[cfg(feature = "ipc-channel-backend")]
    {
        ipc_channel::run_extension::<Out, In>(token)
    }
    #[cfg(all(not(feature = "ipc-channel-backend"), feature = "bek"))]
    {
        let _ = token;
        Err(IpcError::Transport(
            "extension-side bootstrap is delivered by BrowserEngineKit \
             (the host hands the extension its connection), not run from argv"
                .into(),
        ))
    }
    #[cfg(all(not(feature = "ipc-channel-backend"), not(feature = "bek")))]
    {
        let _ = token;
        Err(IpcError::Transport("no IPC backend enabled".into()))
    }
}
