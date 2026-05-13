use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use embedder::{self, FormalWebUserEvent};
use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use ipc_channel::router::ROUTER;
use ipc_messages::content::{
    Bootstrap, ClipboardReadRequest, ClipboardWriteRequest,
    ColorScheme as MessageColorScheme, Command as ContentCommand, Event as ContentEvent,
    TraversableViewport, ViewportSnapshot,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Command as ProcessCommand};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::fetch::FetchCommand;
use crate::timer::{TimerCommand, TimerCompletion};
use crate::UserAgentCommand;

const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);
const CONTENT_CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub enum EventLoopCommand {
    FireAndForget { command: ContentCommand },
    SendCommand {
        command: ContentCommand,
        reply: Sender<Result<Option<u64>, String>>,
    },
    EvaluateScript {
        traversable_id: u64,
        request_id: u64,
        source: String,
        reply: Sender<Result<serde_json::Value, String>>,
    },
    Stop {
        reply: Sender<Result<(), String>>,
    },
}

pub struct EventLoopEntry {
    pub event_loop_id: usize,
    pub command_sender: Sender<EventLoopCommand>,
    pub join_handle: JoinHandle<()>,
    pub traversable_ids: HashSet<u64>,
}

fn navigation_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_NAVIGATION").is_some()
}

fn log_navigation_debug(message: impl AsRef<str>) {
    if navigation_debug_enabled() {
        eprintln!("[navigation-debug][content-process] {}", message.as_ref());
    }
}

fn render_state_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_RENDER_STATE").is_some()
}

fn log_render_state_debug(message: impl AsRef<str>) {
    if render_state_debug_enabled() {
        eprintln!("[render-state][content-process] {}", message.as_ref());
    }
}

fn timer_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_TIMERS").is_some()
}

fn log_timer_debug(message: impl AsRef<str>) {
    if timer_debug_enabled() {
        eprintln!("[timer-debug][user-agent] {}", message.as_ref());
    }
}

fn content_color_scheme(color_scheme: ColorScheme) -> MessageColorScheme {
    match color_scheme {
        ColorScheme::Light => MessageColorScheme::Light,
        ColorScheme::Dark => MessageColorScheme::Dark,
    }
}

pub fn viewport_command(snapshot: (u32, u32, f32, ColorScheme)) -> ContentCommand {
    let (width, height, scale, color_scheme) = snapshot;
    ContentCommand::SetViewport(ViewportSnapshot {
        width,
        height,
        scale,
        color_scheme: content_color_scheme(color_scheme),
    })
}

pub fn traversable_viewport_command(
    traversable_id: u64,
    snapshot: (u32, u32, f32, ColorScheme),
    offset_x: f32,
    offset_y: f32,
) -> ContentCommand {
    let (width, height, scale, color_scheme) = snapshot;
    ContentCommand::SetTraversableViewport(TraversableViewport {
        traversable_id,
        viewport: ViewportSnapshot {
            width,
            height,
            scale,
            color_scheme: content_color_scheme(color_scheme),
        },
        offset_x,
        offset_y,
    })
}

fn traversable_id_from_command(command: &ContentCommand) -> Option<u64> {
    match command {
        ContentCommand::CreateEmptyDocument {
            traversable_id,
            document_id: _,
        }
        | ContentCommand::CreateLoadedDocument {
            traversable_id,
            document_id: _,
            ..
        } => Some(*traversable_id),
        _ => None,
    }
}

pub fn document_id_from_command(command: &ContentCommand) -> Option<u64> {
    match command {
        ContentCommand::CreateEmptyDocument { document_id, .. }
        | ContentCommand::CreateLoadedDocument { document_id, .. } => Some(*document_id),
        _ => None,
    }
}

pub fn destroyed_document_id(command: &ContentCommand) -> Option<u64> {
    match command {
        ContentCommand::DestroyDocument { document_id } => Some(*document_id),
        _ => None,
    }
}

fn content_executable_path() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))
}

struct PendingTaskCommand {
    command: ContentCommand,
    reply: Option<Sender<Result<Option<u64>, String>>>,
}

/// Stateful owner of one HTML event loop thread and its dedicated content process.
///
/// The worker keeps the content subprocess IPC, pending task queue, and script waiters on the
/// thread-owned struct itself. That preserves the spec-facing event-loop model directly in Rust
/// instead of splitting the state across a separate bridge helper.
struct EventLoopWorker {
    /// https://html.spec.whatwg.org/multipage/webappapis.html#event-loop
    event_loop_id: usize,
    /// IPC sender for commands routed into the dedicated content process.
    command_sender: IpcSender<ContentCommand>,
    /// IPC receiver for content-originated events, including fetch requests, timers, and
    /// navigation continuations.
    event_receiver: Receiver<Result<ContentEvent, String>>,
    /// Child process handle for the content sidecar tied to this event loop.
    child: Option<Child>,
    /// Sender back into the owning user-agent worker for navigation and lifecycle coordination.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// Sender into the dedicated fetch worker for document fetch requests.
    fetch_command_sender: Sender<FetchCommand>,
    /// Sender into the dedicated timer worker for window timers and fetch timeouts.
    timer_command_sender: Sender<TimerCommand>,
    /// Pending script evaluation replies keyed by model-local request ids.
    script_waiters: HashMap<u64, Sender<Result<serde_json::Value, String>>>,
    /// Receiver for commands from the user-agent thread into this event-loop/content pair.
    command_receiver: Receiver<EventLoopCommand>,
    /// Deferred shutdown reply completed after the content process acknowledges shutdown.
    stop_reply: Option<Sender<Result<(), String>>>,
    /// Model-local flag that mirrors the single in-flight task step in the HTML event loop
    /// processing model.
    awaiting_task_completion: bool,
    /// FIFO queue of commands that must observe `CommandCompleted` before the next task-bearing
    /// step can run.
    pending_task_commands: VecDeque<PendingTaskCommand>,
}

fn requires_command_completed_wakeup(command: &ContentCommand) -> bool {
    matches!(
        command,
        ContentCommand::CreateEmptyDocument { .. }
            | ContentCommand::CreateLoadedDocument { .. }
            | ContentCommand::DestroyDocument { .. }
            | ContentCommand::DispatchEvent { .. }
            | ContentCommand::RunBeforeUnload { .. }
            | ContentCommand::UpdateTheRendering { .. }
            | ContentCommand::RunWindowTimer { .. }
            | ContentCommand::CompleteDocumentFetch { .. }
            | ContentCommand::FailDocumentFetch { .. }
    )
}

impl EventLoopWorker {
    fn new(
        event_loop_id: usize,
        user_agent_command_sender: Sender<UserAgentCommand>,
        fetch_command_sender: Sender<FetchCommand>,
        timer_command_sender: Sender<TimerCommand>,
        command_receiver: Receiver<EventLoopCommand>,
    ) -> Result<Self, String> {
        let executable_path = content_executable_path()?;
        let (server, token) = IpcOneShotServer::<Bootstrap>::new()
            .map_err(|error| format!("failed to create IPC one-shot server: {error}"))?;

        let mut child_process = ProcessCommand::new(&executable_path);
        child_process.arg("--content-token").arg(&token);

        let child = child_process
            .spawn()
            .map_err(|error| format!("failed to start content: {error}"))?;
        let (_receiver, bootstrap) = server
            .accept()
            .map_err(|error| format!("failed to accept content bootstrap: {error}"))?;

        let (event_sender, event_receiver) = unbounded();
        ROUTER.add_typed_route(
            bootstrap.event_receiver,
            Box::new(move |message| {
                let _ = event_sender.send(
                    message.map_err(|error| format!("failed to decode content IPC event: {error}")),
                );
            }),
        );

        let worker = Self {
            event_loop_id,
            command_sender: bootstrap.command_sender,
            event_receiver,
            child: Some(child),
            user_agent_command_sender,
            fetch_command_sender,
            timer_command_sender,
            script_waiters: HashMap::new(),
            command_receiver,
            stop_reply: None,
            awaiting_task_completion: false,
            pending_task_commands: VecDeque::new(),
        };

        if let Some(snapshot) = embedder::window_viewport_snapshot() {
            let command = viewport_command(snapshot);
            let _ = worker.send_command_inner(&command);
        }

        Ok(worker)
    }

    fn send_command_inner(&self, command: &ContentCommand) -> Result<Option<u64>, String> {
        self.command_sender
            .send(command.clone())
            .map_err(|error| format!("failed to send content IPC message: {error}"))?;

        Ok(traversable_id_from_command(command))
    }

    fn send_immediate_command(
        &mut self,
        command: ContentCommand,
        reply: Option<Sender<Result<Option<u64>, String>>>,
    ) {
        let result = self.send_command_inner(&command);
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    fn flush_next_task_command(&mut self) {
        if self.awaiting_task_completion {
            return;
        }

        let Some(pending_task) = self.pending_task_commands.pop_front() else {
            return;
        };

        let result = self.send_command_inner(&pending_task.command);
        if result.is_ok() {
            self.awaiting_task_completion = true;
        }

        if let Some(reply) = pending_task.reply {
            let _ = reply.send(result);
        }
    }

    fn route_content_command(
        &mut self,
        command: ContentCommand,
        reply: Option<Sender<Result<Option<u64>, String>>>,
    ) {
        // The HTML event loop runs one task-bearing step at a time and resumes only after the
        // content side acknowledges completion. Viewport updates stay out-of-band because they do
        // not emit `CommandCompleted`.
        // Spec: https://html.spec.whatwg.org/multipage/webappapis.html#event-loop-processing-model
        if requires_command_completed_wakeup(&command) {
            self.pending_task_commands
                .push_back(PendingTaskCommand { command, reply });
            self.flush_next_task_command();
            return;
        }

        self.send_immediate_command(command, reply);
    }

    fn handle_command_message(&mut self, command: EventLoopCommand) -> Result<(), String> {
        match command {
            EventLoopCommand::FireAndForget { command } => {
                self.route_content_command(command, None);
            }
            EventLoopCommand::SendCommand { command, reply } => {
                self.route_content_command(command, Some(reply));
            }
            EventLoopCommand::EvaluateScript {
                traversable_id,
                request_id,
                source,
                reply,
            } => {
                self.script_waiters.insert(request_id, reply);
                let command = ContentCommand::EvaluateScript {
                    traversable_id,
                    request_id,
                    source,
                };
                if let Err(error) = self.send_command_inner(&command)
                    && let Some(reply) = self.script_waiters.remove(&request_id)
                {
                    let _ = reply.send(Err(error));
                }
            }
            EventLoopCommand::Stop { reply } => {
                if self.stop_reply.is_none() {
                    self.pending_task_commands.clear();
                    match self.send_command_inner(&ContentCommand::Shutdown) {
                        Ok(_) => {
                            self.stop_reply = Some(reply);
                        }
                        Err(error) => {
                            let _ = reply.send(Err(error));
                            return Err(String::from("content process shutdown command failed"));
                        }
                    }
                } else {
                    let _ = reply.send(Err(String::from("content process is already stopping")));
                }
            }
        }

        Ok(())
    }

    fn handle_content_event_message(&mut self, event: ContentEvent) -> Result<bool, String> {
        match event {
            ContentEvent::DocumentFetchRequested(request) => {
                self.fetch_command_sender
                    .send(FetchCommand::StartDocumentFetch {
                        event_loop_id: self.event_loop_id,
                        request: request.clone(),
                    })
                    .map_err(|error| format!("failed to start document fetch: {error}"))?;
                self.timer_command_sender
                    .send(TimerCommand::Schedule {
                        timer_key: request.handler_id,
                        delay: Duration::from_millis(5000),
                        completion: TimerCompletion::DocumentFetchTimeout {
                            event_loop_id: self.event_loop_id,
                            handler_id: request.handler_id,
                        },
                    })
                    .map_err(|error| format!("failed to schedule document fetch timeout: {error}"))?;
            }
            ContentEvent::WindowTimerRequested(request) => {
                log_timer_debug(format!(
                    "forward schedule document={} id={} key={} timeout_ms={} nesting={}",
                    request.document_id,
                    request.timer_id,
                    request.timer_key,
                    request.timeout_ms,
                    request.nesting_level
                ));
                self.timer_command_sender
                    .send(TimerCommand::Schedule {
                        timer_key: request.timer_key,
                        delay: Duration::from_millis(request.timeout_ms as u64),
                        completion: TimerCompletion::WindowTimerTask {
                            event_loop_id: self.event_loop_id,
                            document_id: request.document_id,
                            timer_id: request.timer_id,
                            timer_key: request.timer_key,
                            nesting_level: request.nesting_level,
                        },
                    })
                    .map_err(|error| format!("failed to schedule window timer: {error}"))?;
            }
            ContentEvent::WindowTimerCleared(request) => {
                log_timer_debug(format!(
                    "forward clear document={} key={}",
                    request.document_id, request.timer_key
                ));
                self.timer_command_sender
                    .send(TimerCommand::Clear {
                        timer_key: request.timer_key,
                    })
                    .map_err(|error| format!("failed to clear window timer: {error}"))?;
            }
            ContentEvent::NavigationRequested(request) => {
                log_navigation_debug(format!(
                    "queue navigation request from {} to {}",
                    request.source_navigable_id, request.destination_url
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::QueueNavigation { request })
                    .map_err(|error| format!("failed to queue navigation request: {error}"))?;
            }
            ContentEvent::BeforeUnloadCompleted(result) => {
                log_navigation_debug(format!(
                    "queue beforeunload completion check={} document={} canceled={}",
                    result.check_id, result.document_id, result.canceled
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::QueueCompleteBeforeUnload { result })
                    .map_err(|error| {
                        format!("failed to queue beforeunload completion: {error}")
                    })?;
            }
            ContentEvent::FinalizeNavigation(finalized) => {
                log_navigation_debug(format!(
                    "queue finalize navigation document={} url={}",
                    finalized.document_id, finalized.url
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::QueueFinalizeNavigation { finalized })
                    .map_err(|error| format!("failed to queue finalize navigation: {error}"))?;
            }
            ContentEvent::IframeTraversableRemoved(removal) => {
                let (reply_sender, reply_receiver) = bounded(1);
                self.user_agent_command_sender
                    .send(UserAgentCommand::IframeTraversableRemoved {
                        parent_traversable_id: removal.parent_traversable_id,
                        content_navigable_id: removal.content_navigable_id,
                        reply: reply_sender,
                    })
                    .map_err(|error| format!("failed to queue iframe traversable removal: {error}"))?;
                reply_receiver.recv().map_err(|error| {
                    format!("iframe traversable removal reply channel closed: {error}")
                })??;
            }
            ContentEvent::ChildNavigableCreated(creation) => {
                let (reply_sender, reply_receiver) = bounded(1);
                self.user_agent_command_sender
                    .send(UserAgentCommand::ChildNavigableCreated {
                        parent_traversable_id: creation.parent_traversable_id,
                        content_navigable_id: creation.content_navigable_id,
                        reply: reply_sender,
                    })
                    .map_err(|error| format!("failed to record child navigable: {error}"))?;
                reply_receiver
                    .recv()
                    .map_err(|error| format!("child navigable reply channel closed: {error}"))??;
            }
            ContentEvent::CommandCompleted => {
                self.awaiting_task_completion = false;
                self.flush_next_task_command();
            }
            ContentEvent::ScriptEvaluated(result) => {
                if let Some(waiter) = self.script_waiters.remove(&result.request_id) {
                    let send_result = match result.error {
                        Some(error) => Err(error),
                        None => serde_json::from_str(&result.value_json).map_err(|error| {
                            format!(
                                "failed to decode content script evaluation result: {error}"
                            )
                        }),
                    };
                    let _ = waiter.send(send_result);
                }
            }
            ContentEvent::ClipboardReadRequested(ClipboardReadRequest { reply_sender }) => {
                let response = embedder::clipboard_get_text(CONTENT_CLIPBOARD_TIMEOUT);
                let _ = reply_sender.send(response);
            }
            ContentEvent::ClipboardWriteRequested(ClipboardWriteRequest { text, reply_sender }) => {
                let response = embedder::clipboard_set_text(text, CONTENT_CLIPBOARD_TIMEOUT);
                let _ = reply_sender.send(response);
            }
            ContentEvent::PaintReady(snapshot) => {
                log_render_state_debug(format!(
                    "paint ready event_loop={} traversable={} frame={} size=({}, {})",
                    self.event_loop_id,
                    snapshot.traversable_id.0,
                    snapshot.frame_id.0,
                    snapshot.viewport_width,
                    snapshot.viewport_height,
                ));
                let _ = embedder::send_user_event(FormalWebUserEvent::Paint(snapshot));
            }
            ContentEvent::ShutdownCompleted => return Ok(false),
        }

        Ok(true)
    }

    fn fail_script_waiters(&mut self, message: &str) {
        let waiters = self.script_waiters.drain().collect::<Vec<_>>();
        for (_request_id, waiter) in waiters {
            let _ = waiter.send(Err(message.to_owned()));
        }
    }

    fn finish_shutdown(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match wait_for_child_exit(child, CONTENT_SHUTDOWN_GRACE_TIMEOUT) {
                Ok(true) => {}
                Ok(false) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(error) => {
                    eprintln!("content bridge shutdown poll error: {error}");
                }
            }
        }
        self.child.take();
    }

    fn run(&mut self) {
        loop {
            let command_receiver = &self.command_receiver;
            let event_receiver = &self.event_receiver;
            select! {
                recv(command_receiver) -> command => {
                    let Ok(command) = command else {
                        let _ = self.send_command_inner(&ContentCommand::Shutdown);
                        break;
                    };

                    if let Err(error) = self.handle_command_message(command) {
                        if let Some(reply) = self.stop_reply.take() {
                            let _ = reply.send(Err(error.clone()));
                        }
                        eprintln!("content bridge command handling error: {error}");
                        break;
                    }
                }
                recv(event_receiver) -> event => {
                    let event = match event {
                        Ok(Ok(event)) => event,
                        Ok(Err(error)) => {
                            if let Some(reply) = self.stop_reply.take() {
                                let _ = reply.send(Err(error.clone()));
                            }
                            eprintln!("content bridge route error: {error}");
                            break;
                        }
                        Err(error) => {
                            if let Some(reply) = self.stop_reply.take() {
                                let _ = reply.send(Err(format!("content event route closed: {error}")));
                            }
                            break;
                        }
                    };

                    match self.handle_content_event_message(event) {
                        Ok(true) => {}
                        Ok(false) => {
                            if let Some(reply) = self.stop_reply.take() {
                                let _ = reply.send(Ok(()));
                            }
                            break;
                        }
                        Err(error) => {
                            if let Some(reply) = self.stop_reply.take() {
                                let _ = reply.send(Err(error.clone()));
                            }
                            eprintln!("content bridge event handling error: {error}");
                            break;
                        }
                    }
                }
            }
        }

        self.fail_script_waiters("content process stopped");
        self.finish_shutdown();
    }
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
                return Err(format!("failed to poll content process exit: {error}"));
            }
        }
    }
}

pub fn spawn_event_loop_entry(
    event_loop_id: usize,
    user_agent_command_sender: Sender<UserAgentCommand>,
    fetch_command_sender: Sender<FetchCommand>,
    timer_command_sender: Sender<TimerCommand>,
) -> Result<EventLoopEntry, String> {
    let (command_sender, command_receiver) = unbounded();
    let mut worker = EventLoopWorker::new(
        event_loop_id,
        user_agent_command_sender,
        fetch_command_sender,
        timer_command_sender,
        command_receiver,
    )?;
    let join_handle = thread::spawn(move || worker.run());
    Ok(EventLoopEntry {
        event_loop_id,
        command_sender,
        join_handle,
        traversable_ids: HashSet::new(),
    })
}

pub fn stop_event_loop_entry(entry: EventLoopEntry) -> Result<(), String> {
    let (reply_sender, reply_receiver) = bounded(1);
    entry
        .command_sender
        .send(EventLoopCommand::Stop { reply: reply_sender })
        .map_err(|error| format!("failed to send event-loop stop command: {error}"))?;

    let stop_result = reply_receiver
        .recv()
        .map_err(|error| format!("event-loop shutdown reply channel closed: {error}"))?;

    entry
        .join_handle
        .join()
        .map_err(|_| String::from("event-loop thread panicked"))?;

    stop_result
}