use blitz_traits::shell::ColorScheme;
use crossbeam_channel::Sender;
use ipc_messages::content::{
    ColorScheme as MessageColorScheme, Command as ContentCommand, Event as ContentEvent,
    EventLoopId, NavigableId, TraversableViewport, ViewportSnapshot,
};
use ipc_messages::graphics::GraphicsCommand;
use log::error;
use std::collections::HashMap;
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;
use verification::TraceSender;

use crate::Embedder;
use crate::ipc_manifest::ContentExtensionManifest;

/// graceful shutdown of the content process owned by one HTML event loop.
const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

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

/// Stateful handle to the window event loop of one similar-origin window
/// agent
/// (<https://html.spec.whatwg.org/multipage/webappapis.html#similar-origin-window-agent>)
/// and its dedicated content process, owned directly by the user-agent
/// thread.  This is the window kind of the two agent event-loop kinds of
/// this implementation: the window event loop runs on the content process
/// main thread (the loop and its tasks live in `ContentProcess`), while a
/// dedicated worker agent's [`crate::WorkerEventLoop`] runs on its own
/// native thread in the same content process.
///
/// The content process is the agent cluster
/// (<https://html.spec.whatwg.org/multipage/webappapis.html#agent-cluster>)
/// of the worker agents it hosts, and its command channel is the cluster's
/// user-agent channel: the commands the user agent addresses to this
/// window event loop are sent over it, and the events of every agent of the
/// cluster (window and worker alike) arrive on its event channel.
#[derive(Debug)]
pub struct EventLoopState {
    /// <https://html.spec.whatwg.org/multipage/#concept-agent-event-loop>
    /// The window event loop id of this similar-origin window agent.
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
