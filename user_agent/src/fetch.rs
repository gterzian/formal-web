use ipc_messages::network::{Request as NetworkRequest, Response as NetworkResponse};
use verification::TraceSender;

use crate::ipc_manifest::NetExtensionManifest;

/// Start the net extension using the IPC abstraction layer.
pub fn start_net_extension(
    trace_sender: Option<TraceSender>,
) -> Result<
    (
        ipc::IpcSender<NetworkRequest>,
        crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
        Option<std::process::Child>,
    ),
    String,
> {
    let manifest = NetExtensionManifest;
    let (mut handle, connection) =
        ipc::ExtensionHandle::launch::<NetExtensionManifest, NetworkRequest, NetworkResponse>(
            &manifest,
        )
        .map_err(|error| format!("failed to start net extension: {error}"))?;

    // Send initial trace sender if set
    if let Some(trace_sender) = trace_sender {
        connection
            .sender
            .send(NetworkRequest::SetTraceSender(Some(trace_sender)))
            .map_err(|error| format!("failed to send trace sender to net: {error}"))?;
    }

    let sender = connection.sender.clone();
    let receiver = connection.receiver;
    let child = handle.take_child();
    Ok((sender, ipc::crossbeam_proxy(receiver), child))
}
