//! `ipc-channel` backend implementation.
//!
//! Provides `start_extension` and `run_extension` using `ipc_channel::ipc`
//! one-shot bootstrap servers and typed channels.

use crossbeam_channel::unbounded;
use ipc_channel::ipc::{
    self as ipc_ipc, IpcOneShotServer, IpcReceiver, IpcSender as IpcChannelSender,
    IpcSharedMemory,
};
use ipc_channel::router::ROUTER;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::types::{
    BootstrapToken, ExtensionClient, ExtensionManifest, ExtensionServer, IpcSharedRegion,
    IpcTransport,
};
use crate::{IpcError, IpcIncoming, IpcSender};

/// The ipc-channel backend wraps `(payload, Option<shmem>)` as a single
/// serde message so that shared memory regions are transferred through
/// ipc-channel's native serde machinery (which serializes the handle as
/// an index into a thread-local vector).  On the receiving side the
/// ROUTER callback unwraps the tuple into `IpcIncoming { payload, shmem }`.
type ChannelMessage<T> = (T, Option<IpcSharedMemory>);

// ── Bootstrap message ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct BootstrapMessage<Out, In> {
    parent_to_child_tx: IpcChannelSender<ChannelMessage<Out>>,
    child_to_parent_rx: IpcReceiver<ChannelMessage<In>>,
}

// ── start_extension ─────────────────────────────────────────────────────────

/// Start an extension process and return a client handle.
pub fn start_extension<M, Out, In>(manifest: &M) -> Result<ExtensionClient<Out, In>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    let (server, token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
        IpcOneShotServer::<BootstrapMessage<Out, In>>::new().map_err(|error| {
            IpcError::Transport(format!("failed to create IPC one-shot server: {error}"))
        })?;

    let bootstrap_token = BootstrapToken { inner: token };
    let child = manifest.spawn(&bootstrap_token)?;

    let (_receiver, bootstrap): (
        IpcReceiver<BootstrapMessage<Out, In>>,
        BootstrapMessage<Out, In>,
    ) = server.accept().map_err(|error| {
        IpcError::Transport(format!("failed to accept extension bootstrap: {error}"))
    })?;

    let BootstrapMessage {
        parent_to_child_tx,
        child_to_parent_rx,
    } = bootstrap;

    let out_tx = IpcSender {
        transport: IpcTransport::IpcChannel(parent_to_child_tx),
    };

    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();
    ROUTER.add_typed_route(
        child_to_parent_rx,
        Box::new(move |message: Result<(In, Option<IpcSharedMemory>), _>| {
            if let Ok((payload, shmem)) = message {
                let incoming = if let Some(shmem) = shmem {
                    IpcIncoming {
                        payload,
                        shmem: Some(IpcSharedRegion::from_ipc_shmem(shmem)),
                    }
                } else {
                    IpcIncoming::new(payload)
                };
                let _ = crossbeam_in_tx.send(incoming);
            }
        }),
    );

    Ok(ExtensionClient {
        tx: out_tx,
        rx: crossbeam_in_rx,
        child: Some(child),
    })
}

// ── run_extension ───────────────────────────────────────────────────────────

/// Run as an extension process.
/// Returns `ExtensionServer<In, Out>` (swapped) because the child's "outgoing"
/// is the parent's `In` type.
pub fn run_extension<M, Out, In>(
    _manifest: &M,
    token: &str,
    _service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: Serialize + DeserializeOwned + Send + 'static,
    In: Serialize + DeserializeOwned + Send + 'static,
{
    let (parent_to_child_tx, parent_to_child_rx): (
        IpcChannelSender<ChannelMessage<Out>>,
        IpcReceiver<ChannelMessage<Out>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create IPC channel: {error}"))
    })?;
    let (child_to_parent_tx, child_to_parent_rx): (
        IpcChannelSender<ChannelMessage<In>>,
        IpcReceiver<ChannelMessage<In>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create IPC channel: {error}"))
    })?;

    log::info!(
        "ipc-channel backend: child connecting to bootstrap token='{}'",
        token
    );
    let bootstrap_sender: IpcChannelSender<BootstrapMessage<Out, In>> =
        IpcChannelSender::<BootstrapMessage<Out, In>>::connect(token.to_string()).map_err(
            |error| IpcError::Transport(format!("failed to connect to bootstrap: {error}")),
        )?;
    log::info!("ipc-channel backend: child connected to bootstrap");

    bootstrap_sender
        .send(BootstrapMessage {
            parent_to_child_tx,
            child_to_parent_rx,
        })
        .map_err(|error| IpcError::Transport(format!("failed to send bootstrap: {error}")))?;

    let (crossbeam_in_tx, crossbeam_in_rx) = unbounded();

    ROUTER.add_typed_route(
        parent_to_child_rx,
        Box::new(move |message: Result<(Out, Option<IpcSharedMemory>), _>| {
            if let Ok((payload, shmem)) = message {
                let incoming = if let Some(shmem) = shmem {
                    IpcIncoming {
                        payload,
                        shmem: Some(IpcSharedRegion::from_ipc_shmem(shmem)),
                    }
                } else {
                    IpcIncoming::new(payload)
                };
                let _ = crossbeam_in_tx.send(incoming);
            }
        }),
    );

    Ok(ExtensionServer {
        tx: IpcSender {
            transport: IpcTransport::IpcChannel(child_to_parent_tx),
        },
        rx: crossbeam_in_rx,
        _listener: None,
    })
}
