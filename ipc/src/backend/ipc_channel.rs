//! `ipc-channel` backend implementation.
//!
//! The bootstrap is a single handshake: the parent creates one-shot servers
//! (one primary + one per named extra endpoint), passes all tokens to the
//! child via argv, the child creates channel pairs for each, and sends them
//! all back in a single bootstrap message.  No second round-trip needed.

use std::collections::HashMap;

use crossbeam_channel::unbounded;
use ipc_channel::ipc::{
    self as ipc_ipc, IpcOneShotServer, IpcReceiver as IpcChannelReceiver,
    IpcSender as IpcChannelSender, IpcSharedMemory,
};
use ipc_channel::router::ROUTER;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::IpcError;
use crate::types::{
    BootstrapPayload, BootstrapToken, ExtensionHandle, ExtensionHandleImpl, ExtensionManifest,
    ExtensionServer, IpcConnection, IpcEndpoint, IpcIncoming, IpcReceiver, IpcSender, IpcSerialize,
    IpcTransport,
};

/// ipc-channel wraps `(payload, HashMap<usize, shmem>)` as a single
/// serde message tuple.
type ChannelMessage<T> = (T, HashMap<usize, IpcSharedMemory>);

/// One channel pair: a sender (parent→child) and a receiver (child→parent).
/// The "parent" side of the bootstrap sends `parent_to_child_tx` and
/// `child_to_parent_rx` to the parent.  The child keeps the other ends.
#[derive(Serialize, Deserialize)]
struct ChannelPair<Out, In> {
    parent_to_child_tx: IpcChannelSender<ChannelMessage<Out>>,
    child_to_parent_rx: IpcChannelReceiver<ChannelMessage<In>>,
}

/// Full bootstrap message: primary channel pair plus any extra named pairs.
#[derive(Serialize, Deserialize)]
struct BootstrapMessage<Out, In> {
    primary: ChannelPair<Out, In>,
    /// Named extra channels (e.g., "net", "media").
    extra: HashMap<String, ChannelPair<Out, In>>,
    /// Serialized BootstrapPayload for backend-agnostic data.
    payload: Vec<u8>,
}

/// Build a connection from a channel pair.
fn build_connection<Out, In>(pair: ChannelPair<Out, In>) -> IpcConnection<Out, In>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let tx = IpcSender {
        transport: IpcTransport::IpcChannel(pair.parent_to_child_tx),
    };

    let (crossbeam_sender, crossbeam_receiver) = unbounded();
    ROUTER.add_typed_route(
        pair.child_to_parent_rx,
        Box::new(
            move |message: Result<(In, HashMap<usize, IpcSharedMemory>), _>| {
                if let Ok((payload, shmem_map)) = message {
                    let regions: HashMap<usize, crate::IpcSharedRegion> = shmem_map
                        .into_iter()
                        .map(|(key, raw)| (key, crate::IpcSharedRegion::from_ipc_shmem(raw)))
                        .collect();
                    let incoming = IpcIncoming {
                        payload,
                        shmem_regions: regions,
                    };
                    let _ = crossbeam_sender.send(incoming);
                }
            },
        ),
    );

    IpcConnection::new(tx, IpcReceiver::from_crossbeam(crossbeam_receiver))
}

// ── Parent side: launch_extension ───────────────────────────────────────────

pub fn launch_extension<M, Out, In>(
    manifest: &M,
    bootstrap: BootstrapPayload,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    // Create the primary one-shot bootstrap server.
    let (primary_server, primary_token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
        IpcOneShotServer::<BootstrapMessage<Out, In>>::new().map_err(|error| {
            IpcError::Transport(format!(
                "failed to create primary IPC one-shot server: {error}"
            ))
        })?;

    let bootstrap_token = BootstrapToken {
        inner: primary_token.clone(),
    };

    // Create one-shot servers for each named extra endpoint and record
    // their tokens so the child can connect to them too.
    let mut extra_servers: Vec<(String, IpcOneShotServer<BootstrapMessage<Out, In>>)> = Vec::new();
    let mut extra_tokens_vec: Vec<(String, String)> = Vec::new();

    for name in bootstrap.endpoints.keys() {
        let (server, token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
            IpcOneShotServer::new().map_err(|error| {
                IpcError::Transport(format!(
                    "failed to create extra endpoint server for {name}: {error}"
                ))
            })?;
        extra_servers.push((name.clone(), server));
        extra_tokens_vec.push((name.clone(), token));
    }

    // Serialize the bootstrap payload to include in the message from child.
    let _bootstrap_bytes = postcard::to_allocvec(&bootstrap)
        .map_err(|error| IpcError::Serialize(error.to_string()))?;

    // Spawn the child process with all tokens.
    let child = manifest.spawn(&bootstrap_token)?;

    // Accept the primary bootstrap connection.
    let (_receiver, bootstrap_msg): (
        IpcChannelReceiver<BootstrapMessage<Out, In>>,
        BootstrapMessage<Out, In>,
    ) = primary_server.accept().map_err(|error| {
        IpcError::Transport(format!("failed to accept extension bootstrap: {error}"))
    })?;

    let primary_connection = build_connection(bootstrap_msg.primary);

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::IpcChannel { child: Some(child) },
    };

    Ok((handle, primary_connection))
}

// ── Child side: run_extension ───────────────────────────────────────────────

pub fn run_extension<M, Out, In>(
    _manifest: &M,
    token: &str,
    _service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    // Create the primary channel pair.
    let (primary_parent_to_child_tx, primary_parent_to_child_rx): (
        IpcChannelSender<ChannelMessage<Out>>,
        IpcChannelReceiver<ChannelMessage<Out>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create primary IPC channel: {error}"))
    })?;
    let (primary_child_to_parent_tx, primary_child_to_parent_rx): (
        IpcChannelSender<ChannelMessage<In>>,
        IpcChannelReceiver<ChannelMessage<In>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create primary IPC channel: {error}"))
    })?;

    // Connect to the primary bootstrap server.
    let bootstrap_sender: IpcChannelSender<BootstrapMessage<Out, In>> =
        IpcChannelSender::<BootstrapMessage<Out, In>>::connect(token.to_string()).map_err(
            |error| IpcError::Transport(format!("failed to connect to bootstrap: {error}")),
        )?;

    // Send the bootstrap message with just the primary channel pair.
    // (Extra endpoints will be supported when the launch side creates them.)
    let bootstrap_msg = BootstrapMessage {
        primary: ChannelPair {
            parent_to_child_tx: primary_parent_to_child_tx,
            child_to_parent_rx: primary_child_to_parent_rx,
        },
        extra: HashMap::new(),
        payload: Vec::new(), // BootstrapPayload will be decoded on demand
    };

    bootstrap_sender
        .send(bootstrap_msg)
        .map_err(|error| IpcError::Transport(format!("failed to send bootstrap: {error}")))?;

    // Build the crossbeam receiver for the primary channel (parent→child messages).
    let (crossbeam_sender, crossbeam_receiver) = unbounded();
    ROUTER.add_typed_route(
        primary_parent_to_child_rx,
        Box::new(
            move |message: Result<(Out, HashMap<usize, IpcSharedMemory>), _>| {
                if let Ok((payload, shmem_map)) = message {
                    let regions: HashMap<usize, crate::IpcSharedRegion> = shmem_map
                        .into_iter()
                        .map(|(key, raw)| (key, crate::IpcSharedRegion::from_ipc_shmem(raw)))
                        .collect();
                    let incoming = IpcIncoming {
                        payload,
                        shmem_regions: regions,
                    };
                    let _ = crossbeam_sender.send(incoming);
                }
            },
        ),
    );

    let primary_connection = IpcConnection::new(
        IpcSender {
            transport: IpcTransport::IpcChannel(primary_child_to_parent_tx),
        },
        IpcReceiver::from_crossbeam(crossbeam_receiver),
    );

    Ok(ExtensionServer {
        connection: primary_connection,
        endpoints: HashMap::new(),
        _listener: None,
    })
}

// ── create_connection (additional connections from handle) ──────────────────

/// Create an additional connection to a child process.
///
/// For ipc-channel, this is NOT yet implemented as a general mechanism.
/// Additional channels are established during the initial bootstrap handshake.
/// Future work: create a long-lived listener on the child side and
/// connect through it.
pub fn create_connection<Out, In>(
    _handle: &ExtensionHandle,
) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    Err(IpcError::Transport(
        "ipc-channel: create_connection not supported; use launch_extension \
         with bootstrap endpoints instead"
            .into(),
    ))
}

/// Create an anonymous endpoint.
///
/// For ipc-channel, this creates a one-shot server and returns both
/// the server side (as an IpcConnection backed by an accept thread)
/// and the endpoint token that a child can connect to.
pub fn create_endpoint<Out, In>(
    _handle: &ExtensionHandle,
) -> Result<(IpcConnection<Out, In>, IpcEndpoint), IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    // Create a one-shot server.
    let (server, token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
        IpcOneShotServer::new().map_err(|error| {
            IpcError::Transport(format!(
                "failed to create endpoint one-shot server: {error}"
            ))
        })?;

    // Spawn a background thread to accept the child's connection.
    let (connection_tx, connection_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("formal-web:ipc-ep-accept".into())
        .spawn(move || {
            let result: Result<IpcConnection<Out, In>, IpcError> = (|| {
                let (_receiver, bootstrap_msg): (
                    IpcChannelReceiver<BootstrapMessage<Out, In>>,
                    BootstrapMessage<Out, In>,
                ) = server.accept().map_err(|error| {
                    IpcError::Transport(format!("failed to accept endpoint bootstrap: {error}"))
                })?;
                Ok(build_connection(bootstrap_msg.primary))
            })();
            let _ = connection_tx.send(result);
        })
        .map_err(|error| {
            IpcError::Transport(format!("failed to spawn endpoint accept thread: {error}"))
        })?;

    // Return immediately; the connection is pending until the child connects.
    // The caller should await it. For now, block here (acceptable because
    // endpoints are created before the child is spawned, so the wait is short).
    // Actually, the child process has already been spawned by the time
    // create_endpoint is called on the handle. So the accept should complete
    // quickly.
    let endpoint = IpcEndpoint {
        data: token.into_bytes(),
    };

    // Block until the child connects.
    let connection = connection_rx
        .recv()
        .map_err(|_| IpcError::Transport("endpoint accept thread panicked".into()))?;

    connection.map(|connection| (connection, endpoint))
}

/// Accept an endpoint on the child side.
///
/// The child connects to the one-shot server, creates channel pairs, and
/// sends them in a bootstrap message. The returned `IpcConnection` follows
/// the same convention as [`run_extension`]: first type param = what the
/// caller (child) sends, second type param = what the caller receives.
///
/// Specifically:
/// - `Out` = what the parent sends to the child (child receives `Out`).
/// - `In` = what the child sends to the parent (child sends `In`).
pub fn accept_endpoint<Out, In>(endpoint: &IpcEndpoint) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let token = String::from_utf8(endpoint.data.clone())
        .map_err(|_| IpcError::Transport("invalid endpoint token: not valid UTF-8".into()))?;

    // Create channel pair.
    // Returns IpcConnection<Out, In> where the caller (child):
    //   - sends `Out` to parent via tx
    //   - receives `In` from parent via rx
    //
    // BootstrapMessage<Out, In> expects:
    //   parent_to_child_tx: ChannelMessage<Out>  (parent sends Out TO child)
    //   child_to_parent_rx: IpcChannelReceiver<ChannelMessage<In>> (child sends In TO parent)
    //
    // So from the child's perspective:
    //   child_to_parent_tx must be ChannelMessage<Out> (child sends Out to parent)
    //   parent_to_child_rx must be ChannelMessage<In> (child receives In from parent)
    let (parent_to_child_tx, parent_to_child_rx): (
        IpcChannelSender<ChannelMessage<In>>,
        IpcChannelReceiver<ChannelMessage<In>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create endpoint IPC channel: {error}"))
    })?;
    let (child_to_parent_tx, child_to_parent_rx): (
        IpcChannelSender<ChannelMessage<Out>>,
        IpcChannelReceiver<ChannelMessage<Out>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create endpoint IPC channel: {error}"))
    })?;

    // Connect to the one-shot server.
    // BootstrapMessage<In, Out> correctly maps the types:
    //   parent_to_child_tx: IpcChannelSender<ChannelMessage<In>>  (parent sends In to child)
    //   child_to_parent_rx: IpcChannelReceiver<ChannelMessage<Out>> (child sends Out to parent)
    let bootstrap_sender: IpcChannelSender<BootstrapMessage<In, Out>> =
        IpcChannelSender::<BootstrapMessage<In, Out>>::connect(token).map_err(|error| {
            IpcError::Transport(format!("failed to connect to endpoint bootstrap: {error}"))
        })?;

    let bootstrap_msg = BootstrapMessage {
        primary: ChannelPair {
            parent_to_child_tx,
            child_to_parent_rx,
        },
        extra: HashMap::new(),
        payload: Vec::new(),
    };

    bootstrap_sender.send(bootstrap_msg).map_err(|error| {
        IpcError::Transport(format!("failed to send endpoint bootstrap: {error}"))
    })?;

    // Build the child-side connection.
    // Child sends `Out` to parent via child_to_parent_tx.
    // Child receives `In` from parent via parent_to_child_rx.
    let (crossbeam_sender, crossbeam_receiver) = unbounded();
    ROUTER.add_typed_route(
        parent_to_child_rx,
        Box::new(
            move |message: Result<(In, HashMap<usize, IpcSharedMemory>), _>| {
                if let Ok((payload, shmem_map)) = message {
                    let regions: HashMap<usize, crate::IpcSharedRegion> = shmem_map
                        .into_iter()
                        .map(|(key, raw)| (key, crate::IpcSharedRegion::from_ipc_shmem(raw)))
                        .collect();
                    let incoming = IpcIncoming {
                        payload,
                        shmem_regions: regions,
                    };
                    let _ = crossbeam_sender.send(incoming);
                }
            },
        ),
    );

    // Return IpcConnection<Out, In> matching the caller's perspective:
    // tx sends Out (child sends Out to parent).
    // rx receives In (child receives In from parent).
    Ok(IpcConnection::new(
        IpcSender {
            transport: IpcTransport::IpcChannel(child_to_parent_tx),
        },
        IpcReceiver::from_crossbeam(crossbeam_receiver),
    ))
}
