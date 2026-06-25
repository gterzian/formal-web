use std::collections::HashMap;

use crossbeam_channel::unbounded;
use ipc_channel::ipc::{
    self as ipc_ipc, IpcOneShotServer, IpcSender as IpcChannelSender, IpcSharedMemory,
};
use ipc_channel::router::ROUTER;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::types::{
    BootstrapToken, ExtensionHandle, ExtensionHandleImpl, ExtensionManifest, ExtensionServer,
    IpcConnection, IpcIncoming, IpcReceiver, IpcSender, IpcSerialize,
    IpcTransport,
};

use crate::IpcError;

type ChannelMessage<T> = (T, HashMap<usize, IpcSharedMemory>);

#[derive(Serialize, Deserialize)]
struct BootstrapMessage<Out, In> {
    parent_to_child_tx: IpcChannelSender<ChannelMessage<Out>>,
    child_to_parent_rx: ipc_ipc::IpcReceiver<ChannelMessage<In>>,
}

fn build_connection<Out, In>(pair: BootstrapMessage<Out, In>) -> IpcConnection<Out, In>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let sender = IpcSender {
        transport: IpcTransport::IpcChannel(pair.parent_to_child_tx),
    };

    let (crossbeam_tx, crossbeam_rx) = unbounded();
    ROUTER.add_typed_route(
        pair.child_to_parent_rx,
        Box::new(move |message: Result<(In, HashMap<usize, IpcSharedMemory>), _>| {
            if let Ok((payload, shmem_map)) = message {
                let regions: HashMap<usize, crate::IpcSharedRegion> = shmem_map
                    .into_iter()
                    .map(|(key, raw)| (key, crate::IpcSharedRegion::from_ipc_shmem(raw)))
                    .collect();
                let incoming = IpcIncoming {
                    payload,
                    shmem_regions: regions,
                };
                let _ = crossbeam_tx.send(incoming);
            }
        }),
    );

    IpcConnection::new(
        sender,
        IpcReceiver::from_crossbeam(crossbeam_rx),
    )
}

pub fn launch_extension<M, Out, In>(
    manifest: &M,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (server_name, token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
        IpcOneShotServer::new().map_err(|error| {
            IpcError::Transport(format!(
                "failed to create IPC one-shot server: {error}"
            ))
        })?;

    let bootstrap_token = BootstrapToken {
        inner: token.clone(),
    };

    let child = manifest.spawn(&bootstrap_token)?;

    let (_receiver, bootstrap): (
        ipc_ipc::IpcReceiver<BootstrapMessage<Out, In>>,
        BootstrapMessage<Out, In>,
    ) = server_name.accept().map_err(|error| {
        IpcError::Transport(format!("failed to accept extension bootstrap: {error}"))
    })?;

    let connection = build_connection(bootstrap);

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::IpcChannel {
            child: Some(child),
            bootstrap_token: token,
        },
    };

    Ok((handle, connection))
}

pub fn run_extension<Out, In>(
    token: &str,
    _service_name: &str,
) -> Result<ExtensionServer<In, Out>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (parent_to_child_tx, parent_to_child_rx): (
        IpcChannelSender<ChannelMessage<Out>>,
        ipc_ipc::IpcReceiver<ChannelMessage<Out>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create primary IPC channel: {error}"))
    })?;
    let (child_to_parent_tx, child_to_parent_rx): (
        IpcChannelSender<ChannelMessage<In>>,
        ipc_ipc::IpcReceiver<ChannelMessage<In>>,
    ) = ipc_ipc::channel().map_err(|error| {
        IpcError::Transport(format!("failed to create primary IPC channel: {error}"))
    })?;

    log::info!(
        "ipc-channel backend: child connecting to bootstrap token='{}'",
        token
    );
    let bootstrap_sender: IpcChannelSender<BootstrapMessage<Out, In>> =
        IpcChannelSender::<BootstrapMessage<Out, In>>::connect(token.to_owned()).map_err(
            |error| IpcError::Transport(format!("failed to connect to bootstrap: {error}")),
        )?;

    bootstrap_sender
        .send(BootstrapMessage {
            parent_to_child_tx,
            child_to_parent_rx,
        })
        .map_err(|error| IpcError::Transport(format!("failed to send bootstrap: {error}")))?;

    let (crossbeam_tx, crossbeam_rx) = unbounded();
    ROUTER.add_typed_route(
        parent_to_child_rx,
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
                    let _ = crossbeam_tx.send(incoming);
                }
            },
        ),
    );

    let connection = IpcConnection::new(
        IpcSender {
            transport: IpcTransport::IpcChannel(child_to_parent_tx),
        },
        IpcReceiver::from_crossbeam(crossbeam_rx),
    );

    Ok(ExtensionServer {
        connection,
        _listener: None,
    })
}

/// Create an additional connection to a child process.
///
/// Not yet implemented for the ipc-channel backend.  ipc-channel uses a
/// one-shot bootstrap handshake for the initial connection, and additional
/// connections require the child to run a persistent listener.  This will
/// be added in a future change.
///
/// On XPC this is supported natively (each call creates a new
/// `xpc_connection_t` to the same Mach service).
pub fn create_connection<Out, In>(
    _bootstrap_token: &str,
) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    Err(IpcError::Transport(
        "ipc-channel: create_connection not yet implemented; 
         use the initial connection from launch_extension instead"
            .into(),
    ))
}
