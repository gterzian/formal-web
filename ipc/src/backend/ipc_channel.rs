//! `ipc-channel` backend implementation.
//!
//! Provides `start_extension` and `run_extension` using `ipc_channel::ipc`
//! one-shot bootstrap servers and typed channels.

use crossbeam_channel::unbounded;
use ipc_channel::ipc::{
    self as ipc_ipc, IpcOneShotServer, IpcReceiver, IpcSender as IpcChannelSender,
};
use ipc_channel::router::ROUTER;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::types::{
    BootstrapToken, ExtensionClient, ExtensionManifest, ExtensionServer, IpcTransport,
};
use crate::{IpcError, IpcIncoming, IpcSender};

// ── Bootstrap message ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct BootstrapMessage<Out, In> {
    parent_to_child_tx: IpcChannelSender<Out>,
    child_to_parent_rx: IpcReceiver<In>,
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
        Box::new(move |message| {
            if let Ok(payload) = message {
                let _ = crossbeam_in_tx.send(IpcIncoming::new(payload));
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
    let (parent_to_child_tx, parent_to_child_rx): (IpcChannelSender<Out>, IpcReceiver<Out>) =
        ipc_ipc::channel().map_err(|error| {
            IpcError::Transport(format!("failed to create IPC channel: {error}"))
        })?;
    let (child_to_parent_tx, child_to_parent_rx): (IpcChannelSender<In>, IpcReceiver<In>) =
        ipc_ipc::channel().map_err(|error| {
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

    // Spawn a thread to forward IPC messages to the crossbeam channel.
    // Directly recv() from the IpcReceiver (not RouterProxy), because
    // RouterProxy would be dropped when this function returns.
    std::thread::spawn(move || {
        loop {
            match parent_to_child_rx.recv() {
                Ok(payload) => {
                    if crossbeam_in_tx.send(IpcIncoming::new(payload)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(ExtensionServer {
        tx: IpcSender {
            transport: IpcTransport::IpcChannel(child_to_parent_tx),
        },
        rx: crossbeam_in_rx,
        _listener: None,
    })
}
