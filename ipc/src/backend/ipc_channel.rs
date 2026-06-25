use std::collections::HashMap;

use ipc_channel::ipc::{
    self as ipc_ipc, IpcOneShotServer, IpcSender as IpcChannelSender, IpcSharedMemory,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::types::{
    BootstrapToken, ExtensionHandle, ExtensionHandleImpl, ExtensionManifest, ExtensionServer,
    IpcConnection, IpcReceiver, IpcSender, IpcSerialize, IpcTransport,
};
use crate::IpcError;

type ChannelMessage<T> = (T, HashMap<usize, IpcSharedMemory>);

#[derive(Serialize, Deserialize)]
struct BootstrapMessage<Out, In> {
    parent_to_child_tx: IpcChannelSender<ChannelMessage<Out>>,
    /// Child→parent receiver end.  The parent keeps the sender; the child
    /// holds the receiver (ipc_channel::ipc::IpcReceiver is actually the
    /// receiving end despite the name "IpcReceiver").
    child_to_parent_rx: ipc_ipc::IpcReceiver<ChannelMessage<In>>,
}

pub fn launch_extension<M, Out, In>(
    manifest: &M,
) -> Result<(ExtensionHandle, IpcConnection<Out, In>), IpcError>
where
    M: ExtensionManifest,
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    let (server, token): (IpcOneShotServer<BootstrapMessage<Out, In>>, String) =
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
    ) = server.accept().map_err(|error| {
        IpcError::Transport(format!("failed to accept extension bootstrap: {error}"))
    })?;

    let sender = IpcSender {
        transport: IpcTransport::IpcChannel(bootstrap.parent_to_child_tx),
    };

    let receiver = IpcReceiver::from_ipc_channel(bootstrap.child_to_parent_rx);

    let handle = ExtensionHandle {
        inner: ExtensionHandleImpl::IpcChannel {
            child: Some(child),
            bootstrap_token: token,
        },
    };

    Ok((handle, IpcConnection::new(sender, receiver)))
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

    let connection = IpcConnection::new(
        IpcSender {
            transport: IpcTransport::IpcChannel(child_to_parent_tx),
        },
        IpcReceiver::from_ipc_channel(parent_to_child_rx),
    );

    Ok(ExtensionServer {
        connection,
        _listener: None,
    })
}

pub fn create_connection<Out, In>(
    _bootstrap_token: &str,
) -> Result<IpcConnection<Out, In>, IpcError>
where
    Out: IpcSerialize + DeserializeOwned + Send + 'static,
    In: IpcSerialize + DeserializeOwned + Send + 'static,
{
    Err(IpcError::Transport(
        "ipc-channel: create_connection not yet implemented; \
         use the initial connection from launch_extension instead"
            .into(),
    ))
}
