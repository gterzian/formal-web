use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, Sender, bounded, select, unbounded};
use ipc_messages::content::{
    ClipboardWriteRequested, ColorScheme as MessageColorScheme, Command as ContentCommand,
    ElementClickResult, Event as ContentEvent, EventLoopId, NavigableId, TraversableViewport,
    ViewportSnapshot, WebviewProviderMessage,
};
use log::{debug, error, warn};
use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Child;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use verification::TraceSender;

use crate::fetch::FetchCommand;
use crate::timer::{TimerCommand, TimerCompletion};
use crate::{Embedder, UserAgentCommand};

/// graceful shutdown of the content process owned by one HTML event loop.
const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// clipboard requests that cross the content/embedder boundary.
const CONTENT_CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(2);

/// Commands that the user-agent thread can send into one HTML event-loop/content pair.
#[derive(Clone)]
pub enum EventLoopCommand {
    FireAndForget {
        command: ContentCommand,
    },
    SendCommand {
        command: ContentCommand,
        reply: Sender<Result<Option<NavigableId>, String>>,
    },
    EvaluateScript {
        traversable_id: NavigableId,
        request_id: u64,
        source: String,
        reply: Sender<Result<serde_json::Value, String>>,
    },
    ClickElement {
        traversable_id: NavigableId,
        request_id: u64,
        selector: String,
        reply: Sender<Result<(), String>>,
    },
    Stop {
        reply: Sender<Result<(), String>>,
    },
}

/// Implementation detail: thread handle plus routing state for one
/// <https://html.spec.whatwg.org/multipage/#event-loop>.
pub struct EventLoopEntry {
    pub event_loop_id: EventLoopId,
    pub command_sender: Sender<EventLoopCommand>,
    pub join_handle: JoinHandle<()>,
    pub traversable_ids: HashSet<NavigableId>,
}

/// navigation debug output related to HTML navigation continuations.
fn log_navigation_debug(message: impl AsRef<str>) {
    let _ = message;
}

/// render-state debug output related to update-the-rendering work.
fn log_render_state_debug(message: impl AsRef<str>) {
    let _ = message;
}

/// timer debug output related to HTML timers and fetch watchdogs.
fn log_timer_debug(message: impl AsRef<str>) {
    let _ = message;
}

/// translating embedder color-scheme state into content IPC messages.
fn content_color_scheme(color_scheme: ColorScheme) -> MessageColorScheme {
    match color_scheme {
        ColorScheme::Light => MessageColorScheme::Light,
        ColorScheme::Dark => MessageColorScheme::Dark,
    }
}

/// viewport state delivered to content outside the HTML task queue.
pub fn viewport_command(snapshot: (u32, u32, f32, ColorScheme)) -> ContentCommand {
    let (width, height, scale, color_scheme) = snapshot;
    ContentCommand::SetViewport(ViewportSnapshot {
        width,
        height,
        scale,
        color_scheme: content_color_scheme(color_scheme),
    })
}

/// per-traversable viewport state delivered to content.
pub fn traversable_viewport_command(
    traversable_id: NavigableId,
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

/// commands that create a traversable on the content side.
fn traversable_id_from_command(command: &ContentCommand) -> Option<NavigableId> {
    match command {
        ContentCommand::CreateEmptyDocument {
            traversable_id,
            document_id: _,
            frame_id: _,
            ..
        }
        | ContentCommand::CreateLoadedDocument {
            traversable_id,
            document_id: _,
            ..
        } => Some(*traversable_id),
        _ => None,
    }
}

/// the queued task-bearing commands in one HTML event loop.
struct PendingTaskCommand {
    command: ContentCommand,
    reply: Option<Sender<Result<Option<NavigableId>, String>>>,
}

/// Stateful owner of one HTML event loop thread and its dedicated content process.
///
/// The worker keeps the content subprocess IPC, pending task queue, and script waiters on the
/// thread-owned struct itself. That preserves the spec-facing event-loop model directly in Rust
/// instead of splitting the state across a separate bridge helper.
struct EventLoopWorker {
    /// <https://html.spec.whatwg.org/multipage/#event-loop>
    event_loop_id: EventLoopId,
    /// IPC sender for commands routed into the dedicated content process.
    command_sender: ipc::IpcSender<ContentCommand>,
    /// IPC receiver for content-originated events, including fetch requests, timers, and
    /// navigation continuations.
    event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<ContentEvent>>,
    /// Child process handle for the content sidecar tied to this event loop.
    child: Option<Child>,
    /// Sender back into the owning user-agent worker for navigation and lifecycle coordination.
    user_agent_command_sender: Sender<UserAgentCommand>,
    /// Sender into the dedicated fetch worker for document fetch requests.
    fetch_command_sender: Sender<FetchCommand>,
    /// Sender into the dedicated timer worker for window timers and fetch timeouts.
    timer_command_sender: Sender<TimerCommand>,
    /// Pending script evaluation replies keyed by request ids.
    script_waiters: HashMap<u64, Sender<Result<serde_json::Value, String>>>,
    /// Pending selector-click replies keyed by request ids.
    click_waiters: HashMap<u64, Sender<Result<(), String>>>,
    /// Receiver for commands from the user-agent thread into this event-loop/content pair.
    command_receiver: Receiver<EventLoopCommand>,
    /// Host integration for paint, clipboard, and initial viewport state.
    host: Arc<dyn Embedder>,
    /// Sender for queued webview-provider updates drained by embedder sync calls.
    webview_provider_sender: Sender<WebviewProviderMessage>,
    /// Deferred shutdown reply completed after the content process acknowledges shutdown.
    stop_reply: Option<Sender<Result<(), String>>>,
    /// flag that mirrors the single in-flight task step in the HTML event loop
    /// processing model.
    awaiting_task_completion: bool,
    pending_task_commands: VecDeque<PendingTaskCommand>,
    /// IPC sender to the net extension (for forwarding response channels).
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    /// IPC sender to the media extension.
    #[allow(dead_code)]
    media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
}

/// <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
fn requires_command_completed_wakeup(command: &ContentCommand) -> bool {
    // These commands correspond to task-bearing steps whose next dequeue must wait for the
    // content side to emit `CommandCompleted`.
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
    /// bootstrapping the content process owned by one
    /// <https://html.spec.whatwg.org/multipage/#event-loop>.
    fn new(
        event_loop_id: EventLoopId,
        process_label: String,
        user_agent_command_sender: Sender<UserAgentCommand>,
        fetch_command_sender: Sender<FetchCommand>,
        timer_command_sender: Sender<TimerCommand>,
        host: Arc<dyn Embedder>,
        webview_provider_sender: Sender<WebviewProviderMessage>,
        command_receiver: Receiver<EventLoopCommand>,
        trace_sender: Option<TraceSender>,
        network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
        media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
    ) -> Result<Self, String> {
        let manifest = crate::ipc_manifest::ContentExtensionManifest::new(process_label);
        let (mut handle, connection) = ipc::ExtensionHandle::launch::<
            crate::ipc_manifest::ContentExtensionManifest,
            ContentCommand,
            ContentEvent,
        >(&manifest)
        .map_err(|error| format!("failed to start content extension: {error}"))?;

        let command_sender = connection.sender.clone();
        let event_receiver = ipc::crossbeam_proxy(connection.receiver);
        let child = handle.take_child();
        // Clone the content command sender for `DirectChannelsSetup` so net can
        // route responses directly via `ResponseRecipient::ContentProcess`.
        let content_command_sender = connection.sender.clone();
        // Clone senders for forwarding before they're moved into Self.
        let network_extension_sender_fwd = network_extension_sender.clone();
        let media_extension_sender_fwd = media_extension_sender.clone();
        let worker = Self {
            event_loop_id,
            command_sender,
            event_receiver,
            child,
            user_agent_command_sender,
            fetch_command_sender,
            timer_command_sender,
            script_waiters: HashMap::new(),
            click_waiters: HashMap::new(),
            command_receiver,
            host,
            webview_provider_sender,
            stop_reply: None,
            awaiting_task_completion: false,
            pending_task_commands: VecDeque::new(),
            network_extension_sender,
            media_extension_sender,
        };

        worker.send_command_inner(&ContentCommand::DirectChannelsSetup {
            net_sender: network_extension_sender_fwd,
            media_sender: media_extension_sender_fwd,
            content_command_sender,
        })?;

        if let Some(snapshot) = worker.host.window_viewport_snapshot() {
            let command = viewport_command(snapshot);
            if let Err(error) = worker.send_command_inner(&command) {
                error!("failed to send initial viewport command: {error}");
            }
        }

        Ok(worker)
    }

    /// sending one command across the content IPC boundary.
    fn send_command_inner(&self, command: &ContentCommand) -> Result<Option<NavigableId>, String> {
        self.command_sender
            .send(command.clone())
            .map_err(|error| format!("failed to send content IPC message: {error}"))?;

        Ok(traversable_id_from_command(command))
    }

    /// immediately sending a non-task-bearing command to content.
    fn send_immediate_command(
        &mut self,
        command: ContentCommand,
        reply: Option<Sender<Result<Option<NavigableId>, String>>>,
    ) {
        // Commands that do not emit `CommandCompleted` stay out-of-band relative to the
        // task queue.
        let result = self.send_command_inner(&command);
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    /// <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
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

    /// <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
    fn route_content_command(
        &mut self,
        command: ContentCommand,
        reply: Option<Sender<Result<Option<NavigableId>, String>>>,
    ) {
        // The HTML event loop runs one task-bearing step at a time and resumes only after the
        // content side acknowledges completion. Viewport updates stay out-of-band because they do
        // not emit `CommandCompleted`.
        // Spec: <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
        if requires_command_completed_wakeup(&command) {
            self.pending_task_commands
                .push_back(PendingTaskCommand { command, reply });
            self.flush_next_task_command();
            return;
        }

        self.send_immediate_command(command, reply);
    }

    /// routing one user-agent command into the event loop's owned
    /// content process and shutdown state.
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
            EventLoopCommand::ClickElement {
                traversable_id,
                request_id,
                selector,
                reply,
            } => {
                self.click_waiters.insert(request_id, reply);
                let command = ContentCommand::ClickElement {
                    traversable_id,
                    request_id,
                    selector,
                };
                if let Err(error) = self.send_command_inner(&command)
                    && let Some(reply) = self.click_waiters.remove(&request_id)
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

    /// <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
    fn handle_content_event_message(
        &mut self,
        event: ContentEvent,
        incoming_shmem: &HashMap<usize, ipc::IpcSharedRegion>,
    ) -> Result<bool, String> {
        match event {
            ContentEvent::DocumentFetchRequested(_request) => {
                // Content now sends fetch requests directly to net via
                // `ResponseRecipient::ContentProcess`. This event is no longer
                // emitted by content and should not be received here.
                warn!(
                    "unexpected DocumentFetchRequested — content should now send directly to net"
                );
            }
            ContentEvent::WindowTimerRequested(request) => {
                // Content already ran the timer initialization algorithm far enough to assign
                // the timer id, key, and nesting level; the timer worker owns the host-side wait.
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
                        timer_key: request.timer_key.0,
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
                // Clearing a timer removes the host-side deadline so no later task can be
                // re-enqueued for this timer key.
                log_timer_debug(format!(
                    "forward clear document={} key={}",
                    request.document_id, request.timer_key
                ));
                self.timer_command_sender
                    .send(TimerCommand::Clear {
                        timer_key: request.timer_key.0,
                    })
                    .map_err(|error| format!("failed to clear window timer: {error}"))?;
            }
            ContentEvent::NavigationRequested(request) => {
                // Navigation start leaves the content event loop and reenters the user-agent
                // navigation algorithm immediately; it does not wait for a `CommandCompleted` wakeup.
                log_navigation_debug(format!(
                    "forward navigation request from {} to {}",
                    request.source_navigable_id, request.destination_url
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::Navigate {
                        event_loop_id: Some(self.event_loop_id),
                        request,
                    })
                    .map_err(|error| format!("failed to send navigation request: {error}"))?;
            }

            ContentEvent::BeforeUnloadCompleted(result) => {
                // Resume HTML's `checking if unloading is canceled` continuation in
                // `UserAgentWorker`.
                log_navigation_debug(format!(
                    "forward beforeunload completion check={} document={} canceled={}",
                    result.check_id, result.document_id, result.canceled
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::CompleteBeforeUnload { result })
                    .map_err(|error| format!("failed to send beforeunload completion: {error}"))?;
            }
            ContentEvent::FinalizeNavigation(finalized) => {
                // Resume HTML's `finalize a cross-document navigation` continuation in
                // `UserAgentWorker`.
                log_navigation_debug(format!(
                    "forward finalize navigation document={} url={}",
                    finalized.document_id, finalized.url
                ));
                self.user_agent_command_sender
                    .send(UserAgentCommand::FinalizeCrossDocumentNavigation { finalized })
                    .map_err(|error| format!("failed to send finalize navigation: {error}"))?;
            }
            ContentEvent::IframeTraversableRemoved(removal) => {
                // Keep child-navigable target-name bookkeeping in the user-agent so event-loop
                // teardown and retargeting share one source of truth.
                let (reply_sender, reply_receiver) = bounded(1);
                self.user_agent_command_sender
                    .send(UserAgentCommand::IframeTraversableRemoved {
                        parent_traversable_id: removal.parent_traversable_id,
                        content_navigable_id: removal.content_navigable_id,
                        content_frame_id: removal.content_frame_id,
                        reply: reply_sender,
                    })
                    .map_err(|error| {
                        format!("failed to send iframe traversable removal: {error}")
                    })?;
                reply_receiver.recv().map_err(|error| {
                    format!("iframe traversable removal reply channel closed: {error}")
                })??;
            }
            ContentEvent::CommandCompleted => {
                // The currently running task-bearing command finished, so the next queued task
                // can run.
                self.awaiting_task_completion = false;
                self.flush_next_task_command();
            }
            ContentEvent::ScriptEvaluated(result) => {
                if let Some(waiter) = self.script_waiters.remove(&result.request_id) {
                    let send_result = match result.error {
                        Some(error) => Err(error),
                        None => serde_json::from_str(&result.value_json).map_err(|error| {
                            format!("failed to decode content script evaluation result: {error}")
                        }),
                    };
                    let _ = waiter.send(send_result);
                }
            }
            ContentEvent::ElementClicked(ElementClickResult { request_id, error }) => {
                if let Some(waiter) = self.click_waiters.remove(&request_id) {
                    let _ = waiter.send(error.map_or(Ok(()), Err));
                }
            }
            ContentEvent::ClipboardWriteRequested(ClipboardWriteRequested { text }) => {
                // Fire-and-forget: write to system clipboard, no reply expected.
                // The host writes the text; any error is logged but not propagated
                // since content does not wait for a response.
                if let Err(error) = self
                    .host
                    .clipboard_set_text(text, CONTENT_CLIPBOARD_TIMEOUT)
                {
                    log::error!("clipboard write failed: {error}");
                }
            }
            ContentEvent::PaintReady(frame) => {
                log_render_state_debug(format!(
                    "paint ready event_loop={} traversable={} frame={} size=({}, {})",
                    self.event_loop_id,
                    frame.traversable_id.0,
                    frame.frame_id.0,
                    frame.viewport_width,
                    frame.viewport_height,
                ));
                if let Err(error) =
                    self.webview_provider_sender
                        .send(WebviewProviderMessage::PaintFrame {
                            frame,
                            shmem_regions: incoming_shmem.clone(),
                        })
                {
                    error!("failed to enqueue webview-provider paint frame: {error}");
                } else {
                    // Silently ignore send failures during shutdown — the event
                    // loop may have already closed.
                    let _ = self.host.webview_provider_sync();
                    let _ = self.host.new_frame_rendered();
                }
            }
            ContentEvent::MediaLoadRequested(request) => {
                debug!(
                    "[media] event loop forwarding MediaLoadRequested url={}",
                    request.url
                );
                self.user_agent_command_sender
                    .send(UserAgentCommand::MediaLoadRequested {
                        url: request.url,
                        document_id: request.document_id,
                        traversable_id: request.traversable_id,
                        video_paint_id: request.video_paint_id,
                    })
                    .map_err(|error| format!("failed to send media load request: {error}"))?;
            }

            ContentEvent::ShutdownCompleted => return Ok(false),
        }

        Ok(true)
    }

    /// failing outstanding script-evaluation waiters when the content
    /// process exits before replying.
    fn fail_script_waiters(&mut self, message: &str) {
        let waiters = self.script_waiters.drain().collect::<Vec<_>>();
        for (_request_id, waiter) in waiters {
            let _ = waiter.send(Err(message.to_owned()));
        }
    }

    /// gracefully shutting down the content process owned by this event loop.
    fn finish_shutdown(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match wait_for_child_exit(child, CONTENT_SHUTDOWN_GRACE_TIMEOUT) {
                Ok(true) => {}
                Ok(false) => {
                    if let Err(error) = child.kill() {
                        error!("failed to kill content process: {error}");
                    }
                    if let Err(error) = child.wait() {
                        error!("failed to wait for content process exit: {error}");
                    }
                }
                Err(error) => {
                    error!("content bridge shutdown poll error: {error}");
                }
            }
        }
        self.child.take();
    }

    /// <https://html.spec.whatwg.org/multipage/#event-loop-processing-model>
    fn run(&mut self) {
        // This loop is the dispatcher for one HTML event loop, interleaving
        // user-agent commands with content-generated completion and continuation events.
        loop {
            let command_receiver = &self.command_receiver;
            let event_receiver = &self.event_receiver;
            select! {
                recv(command_receiver) -> command => {
                    let Ok(command) = command else {
                        error!(
                            "event loop command channel closed for event loop {}; sending shutdown to content",
                            self.event_loop_id
                        );
                        if let Err(error) = self.send_command_inner(&ContentCommand::Shutdown) {
                            error!("failed to send shutdown command to content: {error}");
                        }
                        break;
                    };

                    if let Err(error) = self.handle_command_message(command) {
                        if let Some(reply) = self.stop_reply.take() {
                            let _ = reply.send(Err(error.clone()));
                        }
                        error!("content bridge command handling error: {error}");
                        break;
                    }
                }
                recv(event_receiver) -> event => {
                    let incoming = match event {
                        Ok(incoming) => incoming,
                        Err(error) => {
                            let child_status = if let Some(child) = self.child.as_mut() {
                                let deadline = Instant::now() + Duration::from_millis(500);
                                let mut status = child.try_wait().ok().flatten();
                                while status.is_none() && Instant::now() < deadline {
                                    std::thread::sleep(Duration::from_millis(25));
                                    status = child.try_wait().ok().flatten();
                                }
                                status
                                    .map(|status| status.to_string())
                                    .unwrap_or_else(|| String::from("still running"))
                            } else {
                                String::from("missing child handle")
                            };
                            error!(
                                "content event route closed for event loop {}: {error}; child status: {child_status}",
                                self.event_loop_id
                            );
                            if let Some(reply) = self.stop_reply.take() {
                                let _ = reply.send(Err(format!("content event route closed: {error}")));
                            }
                            break;
                        }
                    };

                    match self
                        .handle_content_event_message(incoming.payload, &incoming.shmem_regions)
                    {
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
                            error!("content bridge event handling error: {error}");
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

/// waiting on the owned content process during shutdown.
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

/// <https://html.spec.whatwg.org/multipage/#event-loop>
pub fn spawn_event_loop_entry(
    event_loop_id: EventLoopId,
    process_label: String,
    user_agent_command_sender: Sender<UserAgentCommand>,
    fetch_command_sender: Sender<FetchCommand>,
    timer_command_sender: Sender<TimerCommand>,
    host: Arc<dyn Embedder>,
    webview_provider_sender: Sender<WebviewProviderMessage>,
    trace_sender: Option<TraceSender>,
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    media_extension_sender: Option<ipc::IpcSender<ipc_messages::media::MediaCommand>>,
) -> Result<EventLoopEntry, String> {
    let (command_sender, command_receiver) = unbounded();
    let mut worker = EventLoopWorker::new(
        event_loop_id,
        process_label,
        user_agent_command_sender,
        fetch_command_sender,
        timer_command_sender,
        host,
        webview_provider_sender,
        command_receiver,
        trace_sender,
        network_extension_sender,
        media_extension_sender,
    )?;
    let join_handle = thread::Builder::new()
        .name(format!("formal-web-event-loop-{event_loop_id}"))
        .spawn(move || worker.run())
        .map_err(|error| format!("failed to spawn event-loop thread {event_loop_id}: {error}"))?;
    Ok(EventLoopEntry {
        event_loop_id,
        command_sender,
        join_handle,
        traversable_ids: HashSet::new(),
    })
}

/// <https://html.spec.whatwg.org/multipage/#event-loop>
pub fn stop_event_loop_entry(entry: EventLoopEntry) -> Result<(), String> {
    let (reply_sender, reply_receiver) = bounded(1);
    entry
        .command_sender
        .send(EventLoopCommand::Stop {
            reply: reply_sender,
        })
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
