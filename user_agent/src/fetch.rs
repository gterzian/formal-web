use crossbeam_channel::{select, Receiver, Sender};
use ipc_messages::content::{FetchRequest as ContentFetchRequest, NavigationFetchId};
use ipc_messages::network::{Request as NetworkRequest, Response as NetworkResponse};
use log::error;
use std::collections::HashMap;
#[cfg(unix)]
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use verification::TraceSender;

use crate::ipc_manifest::NetExtensionManifest;
use crate::UserAgentCommand;

/// Graceful shutdown of the net process owned by the fetch worker.
const FETCH_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// Commands that the user-agent thread can send into the fetch worker.
pub enum FetchCommand {
    StartNavigationFetch {
        fetch_id: NavigationFetchId,
        request: ContentFetchRequest,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// A pending navigation fetch keyed by the user-agent's fetch id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingNavigationFetch {
    /// Formal-web navigation continuation id, not a Fetch Standard request/controller field.
    pub fetch_id: NavigationFetchId,
}

/// Stateful owner of the network-facing half of navigation fetches.
struct FetchWorker {
    /// Receiver for user-agent fetch commands.
    command_receiver: Receiver<FetchCommand>,
    /// Sender back into the user-agent thread for navigation fetch completions.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the net process.
    network_request_sender: ipc::IpcSender<NetworkRequest>,
    /// IPC receiver for net process responses.
    network_event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
    /// Child process handle for the net process.
    child: Option<Child>,
    /// Map from net request_id to pending navigation fetch id.
    pending_navigation_fetches: HashMap<Uuid, NavigationFetchId>,
    /// Deferred shutdown reply completed after the net process exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

/// Start the net extension using the new IPC abstraction layer.
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

/// Waiting on the net process during shutdown.
fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(true),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!("failed to poll network process exit: {error}"));
            }
        }
    }
}

/// Gracefully shutting down the net process owned by the fetch worker.
fn finish_shutdown(mut child: Option<Child>) {
    if let Some(child) = child.as_mut() {
        match wait_for_child_exit(child, FETCH_SHUTDOWN_GRACE_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = child.kill() {
                    error!("failed to kill network process: {error}");
                }
                if let Err(error) = child.wait() {
                    error!("failed to wait for network process exit: {error}");
                }
            }
            Err(error) => {
                error!("fetch shutdown poll error: {error}");
            }
        }
    }
}

impl FetchWorker {
    fn new(
        command_receiver: Receiver<FetchCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
        network_request_sender: ipc::IpcSender<NetworkRequest>,
        network_event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
        child: Option<std::process::Child>,
    ) -> Result<Self, String> {
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            network_request_sender,
            network_event_receiver,
            child,
            pending_navigation_fetches: HashMap::new(),
            shutdown_reply: None,
        })
    }

    /// Fail every pending navigation fetch if the net process stops before producing a response.
    fn fail_pending_fetches(&mut self) {
        for (_request_id, fetch_id) in self.pending_navigation_fetches.drain() {
            let _ = self
                .user_agent_command_sender
                .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
        }
    }

    fn handle_command(&mut self, command: FetchCommand) {
        match command {
            FetchCommand::StartNavigationFetch { fetch_id, request } => {
                let request_id = Uuid::new_v4();
                self.pending_navigation_fetches.insert(request_id, fetch_id);
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request,
                    reply_to: ipc_messages::network::ResponseRecipient::UserAgent,
                }) {
                    self.pending_navigation_fetches.remove(&request_id);
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
                    error!("failed to send navigation fetch request to network process: {error}");
                }
            }
            FetchCommand::Shutdown { reply } => {
                let _ = self.network_request_sender.send(NetworkRequest::Shutdown);
                self.shutdown_reply = Some(reply);
            }
        }
    }

    fn handle_network_response(&mut self, response: NetworkResponse) {
        let Some(fetch_id) = self.pending_navigation_fetches.remove(&response.request_id) else {
            return;
        };

        match response.result {
            Ok(fetch_response) => {
                let _ = self.user_agent_command_sender.send(
                    UserAgentCommand::NavigationFetchCompleted {
                        fetch_id,
                        response: fetch_response,
                    },
                );
            }
            Err(error) => {
                error!("navigation fetch failed: {error}");
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
            }
        }
    }

    fn run(&mut self) {
        loop {
            let command_receiver = &self.command_receiver;
            let network_event_receiver = &self.network_event_receiver;
            select! {
                recv(command_receiver) -> command => {
                    let Ok(command) = command else {
                        break;
                    };
                    let shutting_down = matches!(command, FetchCommand::Shutdown { .. });
                    self.handle_command(command);
                    if shutting_down {
                        break;
                    }
                }
                recv(network_event_receiver) -> response => {
                    match response {
                        Ok(incoming) => {
                            self.handle_network_response(incoming.payload);
                        }
                        Err(error) => {
                            error!("network response route closed: {error}");
                            break;
                        }
                    }
                }
            }
        }

        self.fail_pending_fetches();
        finish_shutdown(self.child.take());
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(Ok(()));
        }
    }
}

/// Spawn the dedicated fetch worker thread owned by `UserAgentWorker`.
pub fn run_fetch_thread(
    command_receiver: Receiver<FetchCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
    network_request_sender: ipc::IpcSender<NetworkRequest>,
    network_event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<NetworkResponse>>,
    child: Option<std::process::Child>,
) {
    let mut worker = match FetchWorker::new(
        command_receiver,
        user_agent_command_sender,
        network_request_sender,
        network_event_receiver,
        child,
    ) {
        Ok(worker) => worker,
        Err(error) => {
            error!("fetch thread startup failed: {error}");
            return;
        }
    };
    worker.run();
}
