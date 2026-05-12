use crossbeam_channel::{Receiver, Sender, unbounded, select};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::content::FetchRequest as ContentFetchRequest;
use ipc_messages::network::{
    Bootstrap as NetworkBootstrap, Request as NetworkRequest, Response as NetworkResponse,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use crate::UserAgentCommand;

const FETCH_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

pub enum FetchCommand {
    StartDocumentFetch {
        event_loop_id: usize,
        request: ContentFetchRequest,
    },
    StartNavigationFetch {
        navigation_id: u64,
        request: ContentFetchRequest,
    },
    Shutdown {
        reply: Sender<Result<(), String>>,
    },
}

pub struct PendingDocumentFetch {
    pub event_loop_id: usize,
    pub handler_id: u64,
}

pub struct PendingNavigationFetch {
    pub navigation_id: u64,
}

pub enum PendingFetch {
    Document(PendingDocumentFetch),
    Navigation(PendingNavigationFetch),
}

struct FetchWorker {
    command_receiver: Receiver<FetchCommand>,
    user_agent_command_sender: Sender<UserAgentCommand>,
    network_request_sender: IpcSender<NetworkRequest>,
    network_event_receiver: Receiver<Result<NetworkResponse, String>>,
    child: Option<Child>,
    next_request_id: u64,
    pending_fetches: HashMap<u64, PendingFetch>,
    shutdown_reply: Option<Sender<Result<(), String>>>,
}

fn executable_file_name(stem: &str) -> String {
    if std::env::consts::EXE_EXTENSION.is_empty() {
        String::from(stem)
    } else {
        format!("{stem}.{}", std::env::consts::EXE_EXTENSION)
    }
}

fn network_executable_path() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| String::from("failed to resolve executable directory"))?;
    let dedicated_executable = parent.join(executable_file_name("network"));
    if dedicated_executable.is_file() {
        return Ok(dedicated_executable);
    }
    Ok(current_exe)
}

fn setup_network(command: &mut Command, token: &str) {
    command.arg("--network-token").arg(token);
}

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

pub fn start_network_bridge(
) -> Result<(IpcSender<NetworkRequest>, Receiver<Result<NetworkResponse, String>>, Child), String> {
    let executable_path = network_executable_path()?;
    let (server, token) = IpcOneShotServer::<NetworkBootstrap>::new()
        .map_err(|error| format!("failed to create network IPC one-shot server: {error}"))?;

    let mut child_process = Command::new(&executable_path);
    setup_network(&mut child_process, &token);

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

    fn fail_pending_fetches(&mut self) {
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
                            navigation_id: pending_fetch.navigation_id,
                        });
                }
            }
        }
    }

    fn handle_command(&mut self, command: FetchCommand) {
        match command {
            FetchCommand::StartDocumentFetch {
                event_loop_id,
                request,
            } => {
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
                                        navigation_id: pending_fetch.navigation_id,
                                    },
                                );
                            }
                        }
                    }
                    eprintln!("failed to send document fetch request to network process: {error}");
                }
            }
            FetchCommand::StartNavigationFetch {
                navigation_id,
                request,
            } => {
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                self.pending_fetches.insert(
                    request_id,
                    PendingFetch::Navigation(PendingNavigationFetch { navigation_id }),
                );
                if let Err(error) = self.network_request_sender.send(NetworkRequest::Fetch {
                    request_id,
                    request,
                }) {
                    self.pending_fetches.remove(&request_id);
                    let _ = self
                        .user_agent_command_sender
                        .send(UserAgentCommand::NavigationFetchFailed { navigation_id });
                    eprintln!("failed to send navigation fetch request to network process: {error}");
                }
            }
            FetchCommand::Shutdown { reply } => {
                let _ = self.network_request_sender.send(NetworkRequest::Shutdown);
                self.shutdown_reply = Some(reply);
            }
        }
    }

    fn handle_network_response(&mut self, response: NetworkResponse) {
        let Some(pending_fetch) = self.pending_fetches.remove(&response.request_id) else {
            return;
        };

        match (pending_fetch, response.result) {
            (PendingFetch::Document(pending_fetch), Ok(fetch_response)) => {
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
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::DocumentFetchFailed {
                        event_loop_id: pending_fetch.event_loop_id,
                        handler_id: pending_fetch.handler_id,
                    });
            }
            (PendingFetch::Navigation(pending_fetch), Ok(fetch_response)) => {
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchCompleted {
                        navigation_id: pending_fetch.navigation_id,
                        response: fetch_response,
                    });
            }
            (PendingFetch::Navigation(pending_fetch), Err(error)) => {
                eprintln!("navigation fetch failed: {error}");
                let _ = self
                    .user_agent_command_sender
                    .send(UserAgentCommand::NavigationFetchFailed {
                        navigation_id: pending_fetch.navigation_id,
                    });
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