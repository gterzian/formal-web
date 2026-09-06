use std::collections::HashMap;
#[cfg(unix)]
use std::process::Child;
use std::time::Duration;

use ipc_messages::content::{EventLoopId, NavigationFetchId};
use ipc_messages::network::{
    NavigationFetchRequest, Request as NetworkRequest, Response as NetworkResponse,
    ResponseRecipient,
};
use log::error;
use verification::TraceSender;

use crate::ipc_manifest::NetExtensionManifest;

/// Graceful shutdown timeout for the net process.
const NET_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// Owns the IPC connection to the net extension, tracks pending navigation
/// fetches, and routes responses back to the user agent.
pub(crate) struct NetConnection {
    /// IPC sender to the net extension.
    sender: ipc::IpcSender<NetworkRequest>,
    /// Crossbeam proxy for net process responses (navigation fetch results).
    receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
    /// Child process handle for the net process.
    child: Option<Child>,
    /// Maps net request_id to NavigationFetchId for response routing.
    pending_fetches: HashMap<uuid::Uuid, NavigationFetchId>,
}

impl NetConnection {
    /// Launch the net extension and return a connected handle.
    pub(crate) fn new(
        trace_sender: Option<TraceSender>,
        helper_directory: Option<std::path::PathBuf>,
    ) -> Result<Self, String> {
        let manifest = NetExtensionManifest { helper_directory };
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
        Ok(Self {
            sender,
            receiver: ipc::crossbeam_proxy(receiver),
            child,
            pending_fetches: HashMap::new(),
        })
    }

    /// Sender for outbound requests.
    pub(crate) fn sender(&self) -> ipc::IpcSender<NetworkRequest> {
        self.sender.clone()
    }

    /// Receiver for incoming responses.
    pub(crate) fn receiver(
        &self,
    ) -> &crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>> {
        &self.receiver
    }

    /// Send a navigation fetch request to the net extension and track it.
    pub(crate) fn start_navigation_fetch(
        &mut self,
        fetch_id: NavigationFetchId,
        event_loop_id: EventLoopId,
        request: NavigationFetchRequest,
    ) -> Result<(), String> {
        let request_id = uuid::Uuid::new_v4();
        self.pending_fetches.insert(request_id, fetch_id);
        if let Err(error) = self.sender.send(NetworkRequest::NavigationFetch {
            event_loop_id,
            request_id,
            request,
            reply_to: ResponseRecipient::UserAgent,
        }) {
            self.pending_fetches.remove(&request_id);
            return Err(format!("failed to start navigation fetch: {error}"));
        }
        Ok(())
    }

    /// Tell the net extension which URL schemes the embedder serves itself.
    pub(crate) fn set_embedder_schemes(&self, schemes: Vec<String>) -> Result<(), String> {
        self.sender
            .send(NetworkRequest::SetEmbedderSchemes { schemes })
            .map_err(|error| format!("failed to send the embedder schemes to net: {error}"))
    }

    /// Hand the embedder's answer for an embedder-scheme fetch back to net,
    /// which routes it to the recipient the intercepted request named.
    pub(crate) fn complete_embedder_scheme_fetch(
        &self,
        request_id: uuid::Uuid,
        result: Result<ipc_messages::content::FetchResponse, String>,
    ) -> Result<(), String> {
        self.sender
            .send(NetworkRequest::CompleteEmbedderSchemeFetch { request_id, result })
            .map_err(|error| format!("failed to complete an embedder-scheme fetch: {error}"))
    }

    /// Resolve a fetch outcome from the net extension to the navigation
    /// fetch that started it.
    pub(crate) fn handle_response(
        &mut self,
        request_id: uuid::Uuid,
        result: Result<ipc_messages::content::FetchResponse, String>,
    ) -> Option<(
        NavigationFetchId,
        Result<ipc_messages::content::FetchResponse, String>,
    )> {
        let fetch_id = self.pending_fetches.remove(&request_id)?;
        Some((fetch_id, result))
    }

    /// Shut down the net extension gracefully.
    pub(crate) fn shutdown(&mut self) {
        if let Err(error) = self.sender.send(NetworkRequest::Shutdown) {
            error!("failed to send Shutdown to net extension: {error}");
        }
        if let Some(mut child) = self.child.take() {
            let deadline = std::time::Instant::now() + NET_SHUTDOWN_GRACE_TIMEOUT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if std::time::Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => {
                        error!("failed to poll net process exit: {error}");
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
}
