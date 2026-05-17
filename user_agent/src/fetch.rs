use crossbeam_channel::{Receiver, Sender, unbounded, select};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::content::{
    DocumentFetchId, EventLoopId, FetchRequest as ContentFetchRequest, NavigationFetchId,
};
use ipc_messages::network::{
    Bootstrap as NetworkBootstrap, Request as NetworkRequest, Response as NetworkResponse,
};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use crate::{UserAgentCommand, sidecar_executable_path};

/// graceful shutdown of the network sidecar owned by the fetch worker.
const FETCH_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// Commands that the user-agent thread can send into the dedicated fetch worker.
pub enum FetchCommand {
    StartDocumentFetch {
        event_loop_id: EventLoopId,
        request: ContentFetchRequest,
    },
    StartNavigationFetch {
        fetch_id: NavigationFetchId,
        request: ContentFetchRequest,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

/// a pending document fetch that must resume one event loop.
pub struct PendingDocumentFetch {
    /// Event loop that should receive the fetch completion/failure.
    pub event_loop_id: EventLoopId,
    /// Content-side handler id for the document fetch request.
    pub handler_id: DocumentFetchId,
}

/// a pending navigation fetch keyed by the user-agent's fetch id.
pub struct PendingNavigationFetch {
    /// identifier corresponding to <https://fetch.spec.whatwg.org/#fetch-controller>
    pub fetch_id: NavigationFetchId,
}

/// distinguishing document and navigation fetch continuations.
pub enum PendingFetch {
    Document(PendingDocumentFetch),
    Navigation(PendingNavigationFetch),
}

/// Stateful owner of the network-facing half of HTML's parallel fetch work plus document-fetch
/// plumbing that resumes event-loop-local fetch handlers.
struct FetchWorker {
    /// Receiver for user-agent fetch commands.
    command_receiver: Receiver<FetchCommand>,
    /// Sender back into the user-agent thread for navigation/document fetch completions.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// IPC sender to the dedicated network sidecar process.
    network_request_sender: IpcSender<NetworkRequest>,
    /// IPC receiver for network sidecar responses.
    network_event_receiver: Receiver<Result<NetworkResponse, String>>,
    /// Child process handle for the network sidecar.
    child: Option<Child>,
    /// Transport-local request id allocator for the network IPC bridge.
    next_request_id: u64,
    /// Pending fetches keyed by transport request id so sidecar responses can be mapped back into
    /// user-agent concepts.
    pending_fetches: HashMap<u64, PendingFetch>,
    /// Deferred shutdown reply completed after the network sidecar exits.
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

/// waiting on the network sidecar during shutdown.
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

/// gracefully shutting down the network sidecar owned by the fetch worker.
fn finish_shutdown(mut child: Option<Child>) {
    if let Some(child) = child.as_mut() {
        match wait_for_child_exit(child, FETCH_SHUTDOWN_GRACE_TIMEOUT) {
            Ok(true) => {}
            Ok(false) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Err(error) => {
                eprintln!("fetch shutdown poll error: {error}");
            }
        }
    }
}

/// bootstrapping the dedicated network sidecar process used by
/// fetch-backed navigation and document fetch continuations.
pub fn start_network_bridge(
) -> Result<(IpcSender<NetworkRequest>, Receiver<Result<NetworkResponse, String>>, Child), String> {
    let (server, token) = IpcOneShotServer::<NetworkBootstrap>::new()
        .map_err(|error| format!("failed to create network IPC one-shot server: {error}"))?;

    let executable_path = sidecar_executable_path("formal-web-net")?;

    let mut child_process = Command::new(&executable_path);
    #[cfg(unix)]
    child_process.arg0("formal-web-net");
    child_process.arg("--net-token").arg(&token);

    let child = child_process
        .spawn()
        .map_err(|error| format!("failed to start network process: {error}"))?;
    let (_receiver, bootstrap) = server
        .accept()
        .map_err(|error| format!("failed to accept network bootstrap: {error}"))?;

    let (event_sender, event_receiver) = unbounded();
    ROUTER.add_typed_route(
        bootstrap.response_receiver,
        Box::new(move |message| {
            let _ = event_sender.send(
                message.map_err(|error| format!("failed to decode network IPC response: {error}")),
            );
        }),
    );

    Ok((bootstrap.request_sender, event_receiver, child))
}

impl FetchWorker {
    /// starting the fetch worker with its owned network sidecar.
    fn new(
        command_receiver: Receiver<FetchCommand>,
        user_agent_command_sender: Sender<UserAgentCommand>,
    ) -> Result<Self, String> {
        let (network_request_sender, network_event_receiver, child) = start_network_bridge()?;
        Ok(Self {
            command_receiver,
            user_agent_command_sender,
            network_request_sender,
            network_event_receiver,
            child: Some(child),
            next_request_id: 1,
            pending_fetches: HashMap::new(),
            shutdown_reply: None,
        })
    }

    /// failing every pending fetch if the network sidecar stops before
    /// producing a response.
    fn fail_pending_fetches(&mut self) {
        // If the network bridge stops early, report every outstanding fetch back through the
        // same user-agent continuations that would have handled an ordinary network failure.
        for pending_fetch in self.pending_fetches.drain().map(|(_, pending)| pending) {
            match pending_fetch {
                PendingFetch::Document(pending_fetch) => {
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::DocumentFetchFailed {
                            event_loop_id: pending_fetch.event_loop_id,
                            handler_id: pending_fetch.handler_id,
                        });
                }
                PendingFetch::Navigation(pending_fetch) => {
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::NavigationFetchFailed {
                            fetch_id: pending_fetch.fetch_id,
                        });
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn handle_command(&mut self, command: FetchCommand) {
        match command {
            FetchCommand::StartDocumentFetch {
                event_loop_id,
                request,
            } => {
                // Document fetches reuse the same network sidecar, but the completion returns
                // directly to the owning event loop instead of the navigation finalization path.
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                self.pending_fetches.insert(
                    request_id,
                    PendingFetch::Document(PendingDocumentFetch {
                        event_loop_id,
                        handler_id: request.handler_id,
                    }),
                );
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request,
                }) {
                    if let Some(pending_fetch) = self.pending_fetches.remove(&request_id) {
                        match pending_fetch {
                            PendingFetch::Document(pending_fetch) => {
                                let _ = self.user_agent_command_sender.send(
                                    UserAgentCommand::DocumentFetchFailed {
                                        event_loop_id: pending_fetch.event_loop_id,
                                        handler_id: pending_fetch.handler_id,
                                    },
                                );
                            }
                            PendingFetch::Navigation(pending_fetch) => {
                                let _ = self.user_agent_command_sender.send(
                                    UserAgentCommand::NavigationFetchFailed {
                                        fetch_id: pending_fetch.fetch_id,
                                    },
                                );
                            }
                        }
                    }
                    eprintln!("failed to send document fetch request to network process: {error}");
                }
            }
            FetchCommand::StartNavigationFetch {
                fetch_id,
                request,
            } => {
                // Step 1: Assert: this is running in parallel.
                // The fetch worker is the concrete owner of the parallel navigation fetch
                // branch started by `UserAgentWorker::create_navigation_params_by_fetching`.
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                self.pending_fetches.insert(
                    request_id,
                    PendingFetch::Navigation(PendingNavigationFetch { fetch_id }),
                );
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request,
                }) {
                    self.pending_fetches.remove(&request_id);
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::NavigationFetchFailed { fetch_id });
                    eprintln!("failed to send navigation fetch request to network process: {error}");
                }
            }
            FetchCommand::Shutdown { reply } => {
                let _ = self.network_request_sender.send(NetworkRequest::Shutdown);
                self.shutdown_reply = Some(reply);
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn handle_network_response(&mut self, response: NetworkResponse) {
        let Some(pending_fetch) = self.pending_fetches.remove(&response.request_id) else {
            return;
        };

        match (pending_fetch, response.result) {
            (PendingFetch::Document(pending_fetch), Ok(fetch_response)) => {
                // Successful document fetches resume the owning event loop's content-side
                // continuation.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::DocumentFetchCompleted {
                        event_loop_id: pending_fetch.event_loop_id,
                        handler_id: pending_fetch.handler_id,
                        response: fetch_response,
                    });
            }
            (PendingFetch::Document(pending_fetch), Err(error)) => {
                eprintln!("document fetch failed: {error}");
                // Document fetch failures reenter the owning event loop so the content-side
                // fetch algorithm can fail the handler in place.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::DocumentFetchFailed {
                        event_loop_id: pending_fetch.event_loop_id,
                        handler_id: pending_fetch.handler_id,
                    });
            }
            (PendingFetch::Navigation(pending_fetch), Ok(fetch_response)) => {
                // Successful navigation fetches resume the user-agent-side document creation
                // and finalization continuation keyed by `fetch_id`.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchCompleted {
                        fetch_id: pending_fetch.fetch_id,
                        response: fetch_response,
                    });
            }
            (PendingFetch::Navigation(pending_fetch), Err(error)) => {
                eprintln!("navigation fetch failed: {error}");
                // Navigation fetch failures resume the same pending navigation record so the
                // user agent can clear `ongoing_navigation_id` and surface failure to the embedder.
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchFailed {
                        fetch_id: pending_fetch.fetch_id,
                    });
            }
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching>
    fn run(&mut self) {
        // The fetch worker owns the network-facing half of HTML's parallel fetch branch and
        // drains either new user-agent requests or sidecar responses until shutdown.
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
                    let response = match response {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => {
                            eprintln!("network process route error: {error}");
                            break;
                        }
                        Err(error) => {
                            eprintln!("network response route closed: {error}");
                            break;
                        }
                    };
                    self.handle_network_response(response);
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

/// spawning the dedicated fetch worker thread owned by `UserAgentWorker`.
pub fn run_fetch_thread(
    command_receiver: Receiver<FetchCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
) {
    let mut worker = match FetchWorker::new(command_receiver, user_agent_command_sender) {
        Ok(worker) => worker,
        Err(error) => {
            eprintln!("fetch thread startup failed: {error}");
            return;
        }
    };
    worker.run();
}