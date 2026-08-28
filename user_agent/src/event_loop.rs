use blitz_traits::shell::ColorScheme;
use crossbeam_channel::Sender;
use ipc_messages::content::{
    ClipboardWriteRequested, ColorScheme as MessageColorScheme, Command as ContentCommand,
    ElementClickResult, Event as ContentEvent, EventLoopId, NavigableId, TitleChanged,
    TraversableViewport, ViewportSnapshot, WebviewId,
};
use ipc_messages::graphics::GraphicsCommand;
use log::error;
use std::collections::HashMap;
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;
use verification::TraceSender;

use crate::channel_messaging::PortEvent;
use crate::ipc_manifest::ContentExtensionManifest;
use crate::{Embedder, UserAgentCommand};

/// graceful shutdown of the content process owned by one HTML event loop.
const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// clipboard requests that cross the content/embedder boundary.
const CONTENT_CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(2);

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

/// Stateful handle to one HTML event loop and its dedicated content process,
/// owned directly by the user-agent thread.
#[derive(Debug)]
pub struct EventLoopState {
    /// <https://html.spec.whatwg.org/multipage/#event-loop>
    pub event_loop_id: EventLoopId,
    /// IPC sender for commands routed into the dedicated content process.
    pub command_sender: ipc::IpcSender<ContentCommand>,
    /// IPC receiver for content-originated events, including fetch requests,
    /// navigation continuations, and clipboards.
    pub event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<ContentEvent>>,
    /// Child process handle for the content process.
    pub child: Option<Child>,
    /// Pending script evaluation replies keyed by request ids.
    pub script_waiters: HashMap<u64, Sender<Result<serde_json::Value, String>>>,
    /// Pending selector-click replies keyed by request ids.
    pub click_waiters: HashMap<u64, Sender<Result<(), String>>>,
}

impl EventLoopState {
    /// Failing the outstanding automation waiters when the content process
    /// exits before replying.
    fn fail_pending_waiters(&mut self, message: &str) {
        for (_request_id, waiter) in self.script_waiters.drain().collect::<Vec<_>>() {
            let _ = waiter.send(Err(message.to_owned()));
        }
        for (_request_id, waiter) in self.click_waiters.drain().collect::<Vec<_>>() {
            let _ = waiter.send(Err(message.to_owned()));
        }
    }

    /// Waiting on the content process of an event loop that has already exited,
    /// so the child is not left as a zombie.
    pub(crate) fn reap_exited_child(&mut self) {
        if let Some(mut child) = self.child.take()
            && let Err(error) = child.wait()
        {
            error!("failed to wait for content process exit: {error}");
        }
        self.fail_pending_waiters("content process exited");
    }

    /// Routing one content-originated event to the appropriate user-agent
    /// behavior.  Returns `Ok(false)` when the content process has shut down.
    pub(crate) fn handle_event(
        &mut self,
        incoming: ipc::IpcIncoming<ContentEvent>,
        user_agent_command_sender: &Sender<UserAgentCommand>,
        host: &Arc<dyn Embedder>,
    ) -> Result<bool, String> {
        match incoming.payload {
            ContentEvent::NavigationRequested(request) => {
                // Navigation start leaves the content event loop and reenters the
                // user-agent navigation algorithm immediately.
                user_agent_command_sender
                    .send(UserAgentCommand::Navigate {
                        event_loop_id: Some(self.event_loop_id),
                        request,
                    })
                    .map_err(|error| format!("failed to send navigation request: {error}"))?;
            }
            ContentEvent::PostMessageRequested(request) => {
                user_agent_command_sender
                    .send(UserAgentCommand::PostMessage { request })
                    .map_err(|error| format!("failed to send postMessage request: {error}"))?;
            }
            ContentEvent::PortChannelCreated { port1, port2 } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::ChannelCreated {
                            port1,
                            port2,
                            event_loop: self.event_loop_id,
                        },
                    })
                    .map_err(|error| format!("failed to forward port channel: {error}"))?;
            }
            ContentEvent::PortTransferStarted { port } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::TransferStarted { port },
                    })
                    .map_err(|error| format!("failed to forward port transfer: {error}"))?;
            }
            ContentEvent::PortTransferReceived { port } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::TransferReceived {
                            port,
                            event_loop: self.event_loop_id,
                        },
                    })
                    .map_err(|error| format!("failed to forward port receive: {error}"))?;
            }
            ContentEvent::PortMessageRouted { tgt, msg } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::MessageRouted { tgt, msg },
                    })
                    .map_err(|error| format!("failed to forward port message: {error}"))?;
            }
            ContentEvent::PortBufferReturned { tgt, buf } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::BufferReturned { tgt, buf },
                    })
                    .map_err(|error| format!("failed to forward returned buffer: {error}"))?;
            }
            ContentEvent::PortTransferCompleted { tgt } => {
                user_agent_command_sender
                    .send(UserAgentCommand::PortEvent {
                        event: PortEvent::TransferCompleted { tgt },
                    })
                    .map_err(|error| format!("failed to forward transfer completion: {error}"))?;
            }
            ContentEvent::BeforeUnloadCompleted(result) => {
                user_agent_command_sender
                    .send(UserAgentCommand::CompleteBeforeUnload { result })
                    .map_err(|error| format!("failed to send beforeunload completion: {error}"))?;
            }
            ContentEvent::FinalizeNavigation(finalized) => {
                user_agent_command_sender
                    .send(UserAgentCommand::FinalizeCrossDocumentNavigation { finalized })
                    .map_err(|error| format!("failed to send finalize navigation: {error}"))?;
            }
            ContentEvent::IframeTraversableRemoved(removal) => {
                user_agent_command_sender
                    .send(UserAgentCommand::IframeTraversableRemoved {
                        parent_traversable_id: removal.parent_traversable_id,
                        content_navigable_id: removal.content_navigable_id,
                        content_frame_id: removal.content_frame_id,
                    })
                    .map_err(|error| {
                        format!("failed to send iframe traversable removal: {error}")
                    })?;
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
                if let Err(error) = host.clipboard_set_text(text, CONTENT_CLIPBOARD_TIMEOUT) {
                    error!("clipboard write failed: {error}");
                }
            }
            ContentEvent::TitleChanged(TitleChanged {
                traversable_id,
                title,
            }) => {
                // The content process reports the parsed title of the
                // top-level document; forward it to the embedder.
                if let Err(error) = host.title_changed(WebviewId(traversable_id), title) {
                    error!("failed to forward document title: {error}");
                }
            }
            ContentEvent::RegisterMediaPipeline(_) => {
                // Content sends CreateMediaPipeline directly to the graphics process.
                // No UA bookkeeping needed.
            }
            ContentEvent::RenderingOpRequested(navigable_id) => {
                user_agent_command_sender
                    .send(UserAgentCommand::RenderingOpportunityFor { navigable_id })
                    .map_err(|error| format!("failed to forward rendering op request: {error}"))?;
            }
            ContentEvent::ShutdownCompleted => return Ok(false),
        }

        Ok(true)
    }

    /// Gracefully shutting down the content process owned by this event loop.
    pub(crate) fn shutdown(&mut self) {
        // Ask the content process to stop; it sends ShutdownCompleted and
        // exits its message loop.  The child exit is then awaited below.
        if let Err(error) = self.command_sender.send(ContentCommand::Shutdown) {
            error!("failed to send shutdown to content process: {error}");
        }
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
        self.fail_pending_waiters("content process stopped");
    }
}

/// bootstrapping a dedicated content process owned by one
/// <https://html.spec.whatwg.org/multipage/#event-loop>.
pub fn spawn_event_loop(
    event_loop_id: EventLoopId,
    process_label: String,
    host: Arc<dyn Embedder>,
    trace_sender: Option<TraceSender>,
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    graphics_sender_for_bootstrap: Option<ipc::IpcSender<GraphicsCommand>>,
) -> Result<EventLoopState, String> {
    let manifest = ContentExtensionManifest::new(process_label);
    let (mut handle, connection) =
        ipc::ExtensionHandle::launch::<ContentExtensionManifest, ContentCommand, ContentEvent>(
            &manifest,
        )
        .map_err(|error| format!("failed to start content extension: {error}"))?;

    let command_sender = connection.sender.clone();
    let event_receiver = ipc::crossbeam_proxy(connection.receiver);
    let child = handle.take_child();
    // Clone the content command sender for `DirectChannelsSetup` so net can
    // route responses directly via `ResponseRecipient::ContentProcess`.
    let content_command_sender = connection.sender.clone();
    // Clone senders for forwarding before they're moved into the state.
    let network_extension_sender_fwd = network_extension_sender.clone();
    let state = EventLoopState {
        event_loop_id,
        command_sender,
        event_receiver,
        child,
        script_waiters: HashMap::new(),
        click_waiters: HashMap::new(),
    };

    state
        .command_sender
        .send(ContentCommand::ContentBootstrap {
            event_loop_id,
            net_sender: network_extension_sender_fwd,
            graphics_sender: graphics_sender_for_bootstrap,
            content_command_sender,
            trace_sender,
        })
        .map_err(|error| format!("failed to send content bootstrap: {error}"))?;

    if let Some(snapshot) = host.window_viewport_snapshot() {
        let command = viewport_command(snapshot);
        if let Err(error) = state.command_sender.send(command) {
            error!("failed to send initial viewport command: {error}");
        }
    }

    Ok(state)
}

/// waiting on the owned content process during shutdown.
fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(true),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!("failed to poll content process exit: {error}"));
            }
        }
    }
}
