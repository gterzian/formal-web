//! The user-event bus for the winit embedder: how the user agent (any
//! thread) hands events to the winit event loop (main thread).

use crate::shared::{clipboard_get_text, clipboard_set_text, window_viewport_snapshot};
use automation::AutomationCommand;
use ipc_messages::content::WebviewId;
use log::error;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use webview::{ColorScheme, Embedder, NavigationCompleted};
use winit::event_loop::EventLoopProxy;

/// The user-event bus: how the user agent (any thread) hands events to the
/// app's event loop (main thread).
pub trait UserEventSink: Send + Sync {
    fn send(&self, event: FormalWebUserEvent) -> Result<(), String>;
}

/// Winit-backed sink: forwards events through the winit event loop proxy.
#[derive(Clone)]
pub struct WinitEventSink {
    proxy: EventLoopProxy<FormalWebUserEvent>,
}

impl WinitEventSink {
    pub fn new(proxy: EventLoopProxy<FormalWebUserEvent>) -> Self {
        Self { proxy }
    }
}

impl UserEventSink for WinitEventSink {
    fn send(&self, event: FormalWebUserEvent) -> Result<(), String> {
        self.proxy
            .send_event(event)
            .map_err(|error| format!("failed to send user event: {error}"))
    }
}

static USER_EVENT_SINK: LazyLock<Mutex<Option<Arc<dyn UserEventSink>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn install_user_event_sink(sink: Arc<dyn UserEventSink>) {
    *USER_EVENT_SINK
        .lock()
        .expect("user event sink mutex poisoned") = Some(sink);
}

pub fn clear_user_event_sink() {
    *USER_EVENT_SINK
        .lock()
        .expect("user event sink mutex poisoned") = None;
}

pub fn send_user_event(event: FormalWebUserEvent) -> Result<(), String> {
    let guard = USER_EVENT_SINK
        .lock()
        .expect("user event sink mutex poisoned");
    match guard.as_ref() {
        Some(sink) => sink.send(event),
        None => Err(String::from("user event sink is not installed")),
    }
}

pub fn event_loop_is_ready() -> bool {
    USER_EVENT_SINK
        .lock()
        .expect("user event sink mutex poisoned")
        .is_some()
}

pub enum FormalWebUserEvent {
    RequestRedraw(WebviewId),
    NewWebContentScene {
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: HashMap<usize, Vec<u8>>,
    },
    NewWebContentLayers {
        webview_id: WebviewId,
        /// The per-layer frames: topology always, surface only for the
        /// layers re-rendered this cycle.
        layers: Vec<ipc_messages::graphics::LayerFrame>,
        /// Whether the composed scene contains animated content (video, CSS
        /// animations) that needs the next frame at display cadence.
        animating: bool,
    },
    NavigationRequested {
        webview_id: WebviewId,
        destination_url: String,
    },
    NavigationCompleted(NavigationCompleted),
    #[allow(dead_code)]
    NewWebview(WebviewId, String),
    CreateWindow,
    Automation(AutomationCommand),
    ClipboardRead {
        reply: mpsc::Sender<Result<String, String>>,
    },
    ClipboardWrite {
        text: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    /// The parsed title of a top-level document, for tab and window labels.
    TitleChanged {
        webview_id: WebviewId,
        title: String,
    },
    Exit,
}

/// Routes `webview::Embedder` callbacks into `FormalWebUserEvent` events on
/// the user-event sink.
pub struct EventLoopEmbedder {
    sink: Arc<dyn UserEventSink>,
}

impl EventLoopEmbedder {
    pub fn new(sink: Arc<dyn UserEventSink>) -> Self {
        Self { sink }
    }
}

impl Embedder for EventLoopEmbedder {
    fn navigation_requested(
        &self,
        webview_id: WebviewId,
        destination_url: String,
    ) -> Result<(), String> {
        self.sink.send(FormalWebUserEvent::NavigationRequested {
            webview_id,
            destination_url,
        })
    }

    fn navigation_completed(&self, completed: NavigationCompleted) -> Result<(), String> {
        self.sink
            .send(FormalWebUserEvent::NavigationCompleted(completed))
    }

    fn new_webview(&self, webview_id: WebviewId, target_name: String) -> Result<(), String> {
        log::debug!(
            "[embedder] Embedder::new_webview webview={:?} target={}",
            webview_id,
            target_name
        );
        self.sink
            .send(FormalWebUserEvent::NewWebview(webview_id, target_name))
    }

    fn request_redraw(&self, webview_id: WebviewId) {
        if let Err(error) = self
            .sink
            .send(FormalWebUserEvent::RequestRedraw(webview_id))
        {
            error!("failed to request redraw for webview {webview_id:?}: {error}");
        }
    }

    fn viewport_scale_factor(&self) -> f32 {
        window_viewport_snapshot()
            .map(|(_, _, scale, _)| scale)
            .unwrap_or(1.0)
    }

    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)> {
        window_viewport_snapshot()
    }

    fn clipboard_get_text(&self) -> Result<String, String> {
        clipboard_get_text()
    }

    fn clipboard_set_text(&self, text: String) -> Result<(), String> {
        clipboard_set_text(text)
    }

    fn title_changed(&self, webview_id: WebviewId, title: String) -> Result<(), String> {
        send_user_event(FormalWebUserEvent::TitleChanged { webview_id, title })
    }

    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        font_data: HashMap<usize, Vec<u8>>,
    ) -> Result<(), String> {
        self.sink.send(FormalWebUserEvent::NewWebContentScene {
            webview_id,
            scene_bytes,
            font_registrations,
            font_data,
        })
    }

    fn new_web_content_layers(
        &self,
        webview_id: WebviewId,
        layers: Vec<ipc_messages::graphics::LayerFrame>,
        animating: bool,
    ) -> Result<(), String> {
        self.sink
            .send(FormalWebUserEvent::NewWebContentLayers {
                webview_id,
                layers,
                animating,
            })
            .map_err(|error| format!("failed to send surface event: {error}"))
    }
}
