//! User-agent-side records of the two event-loop kinds of this
//! implementation — the window event loop of a similar-origin window agent
//! and the worker event loop of a dedicated worker agent — together with the
//! content-process bootstrap that runs a window event loop inside a content
//! process.  The event loops themselves run in the content process; these
//! records are what the user agent needs to address them.
//!
//! A content process is one agent cluster: it hosts exactly one
//! similar-origin window agent, whose window event loop runs on the
//! process's main thread, plus the dedicated worker agents of the workers
//! the cluster's realms create, each running its own worker event loop on
//! its own native thread inside the same process.  The user agent owns the
//! window agent's process directly, so the window event loop's record also
//! carries the process resources: its command channel doubles as the
//! cluster's channel (bootstrap, viewport, document lifecycle, shutdown)
//! and as the window event loop's queue-task channel; its event channel
//! carries the events of every agent of the cluster (window and worker
//! alike — worker events carry their own event loop id in the payload when
//! the sender cannot be inferred); and it owns the child process.  A
//! dedicated worker agent's event loop is additionally addressed over the
//! agent's own user-agent command channel, because its thread runs inside
//! the window agent's process.

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

/// Graceful shutdown of the content process owned by one window event loop.
const CONTENT_SHUTDOWN_GRACE_TIMEOUT: Duration = Duration::from_millis(150);

/// Translating embedder color-scheme state into content IPC messages.
fn content_color_scheme(color_scheme: ColorScheme) -> MessageColorScheme {
    match color_scheme {
        ColorScheme::Light => MessageColorScheme::Light,
        ColorScheme::Dark => MessageColorScheme::Dark,
    }
}

/// Viewport state delivered to content outside the HTML task queue.
pub fn viewport_command(snapshot: (u32, u32, f32, ColorScheme)) -> ContentCommand {
    let (width, height, scale, color_scheme) = snapshot;
    ContentCommand::SetViewport(ViewportSnapshot {
        width,
        height,
        scale,
        color_scheme: content_color_scheme(color_scheme),
    })
}

/// Per-traversable viewport state delivered to content.
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

/// <https://html.spec.whatwg.org/multipage/#window-event-loop>
///
/// The user-agent-side record of one similar-origin window agent's window
/// event loop, which runs on the main thread of the content process (the
/// agent cluster) this record owns.
#[derive(Debug)]
pub struct WindowEventLoop {
    /// The event loop id of this window event loop.
    pub event_loop_id: EventLoopId,
    /// IPC sender for commands routed into the dedicated content process.
    pub command_sender: ipc::IpcSender<ContentCommand>,
    /// IPC receiver for content-originated events: navigation requests and
    /// continuations, clipboard writes, rendering-opportunity requests,
    /// postMessage and message-port events, title reports, and
    /// dedicated-worker-agent obtained/closed reports.  Content fetches do
    /// not transit the user agent: subresource fetches go content→net over
    /// the content process's own net channel, and the user agent runs
    /// navigation fetches itself against the net process over its own
    /// connection, forwarding the response to content as a command.
    pub event_receiver: crossbeam_channel::Receiver<ipc::IpcIncoming<ContentEvent>>,
    /// Child process handle for the content process.
    pub child: Option<Child>,
    /// Pending script evaluation replies keyed by request ids.
    pub script_waiters: HashMap<u64, Sender<Result<serde_json::Value, String>>>,
    /// Pending selector-click replies keyed by request ids.
    pub click_waiters: HashMap<u64, Sender<Result<(), String>>>,
}

impl WindowEventLoop {
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

/// <https://html.spec.whatwg.org/multipage/webappapis.html#worker-event-loop-2>
/// The user-agent side of one dedicated worker agent's worker event loop:
/// the agent's own event loop id and its own user-agent command channel, so
/// the user agent can send commands addressed to the agent's event loop
/// (e.g. the port tasks of `Command::PortTask`) directly to the worker.
#[derive(Debug)]
pub struct WorkerEventLoop {
    /// The event loop id under which ports of this agent's realm are
    /// registered with the user agent, and the destination of port tasks
    /// routed to them.
    pub event_loop_id: EventLoopId,
    /// The agent's own command channel to the user agent: commands
    /// addressed to this agent's event loop are sent directly over it,
    /// bypassing the hosting content process's main-thread forwarding.
    pub command_sender: ipc::IpcSender<ContentCommand>,
}

/// Creating the agent cluster of a similar-origin window agent: spawning a
/// content process and bootstrapping the agent's window event loop inside
/// it.  The content process is this implementation's realization of the
/// agent cluster (<https://html.spec.whatwg.org/multipage/#agent-cluster>);
/// its main thread runs the window event loop of the window agent the
/// cluster hosts, and the dedicated worker agents of the workers the
/// documents of that cluster create run their worker event loops on nested
/// threads of the same process.  The returned record is the user-agent-side
/// handle to the window event loop created inside the process.
pub fn spawn_window_event_loop(
    event_loop_id: EventLoopId,
    process_label: String,
    host: Arc<dyn Embedder>,
    trace_sender: Option<TraceSender>,
    network_extension_sender: ipc::IpcSender<ipc_messages::network::Request>,
    graphics_sender_for_bootstrap: Option<ipc::IpcSender<GraphicsCommand>>,
) -> Result<WindowEventLoop, String> {
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
    let state = WindowEventLoop {
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

/// Waiting on the owned content process during shutdown.
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
