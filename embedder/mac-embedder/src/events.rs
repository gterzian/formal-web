//! The user-event bus for the AppKit embedder: how the user agent (any
//! thread) hands events to the app's main-thread run loop.

use crate::platform::{clipboard_get_text, clipboard_set_text, window_viewport_snapshot};
use automation::AutomationCommand;
use ipc_messages::content::WebviewId;
use log::error;
use std::collections::HashMap;
use std::sync::{Arc, mpsc};
use webview::{ColorScheme, Embedder, NavigationCompleted};

use crate::app::MainThreadHandle;

/// The user-event bus: how the user agent (any thread) hands events to the
/// app's event loop (main thread).
pub trait UserEventSink: Send + Sync {
    fn send(&self, event: FormalWebUserEvent) -> Result<(), String>;
}

/// The user-event sink for the AppKit app: posts each event to the main
/// dispatch queue, where the app's run loop picks it up.
pub struct MacEventSink {
    pub(crate) handle: MainThreadHandle,
}

impl UserEventSink for MacEventSink {
    fn send(&self, event: FormalWebUserEvent) -> Result<(), String> {
        let handle = self.handle;
        dispatch::Queue::main().exec_async(move || {
            let app = unsafe { &mut *handle.app_ptr() };
            app.process_user_event(event);
        });
        Ok(())
    }
}

pub enum FormalWebUserEvent {
    RequestRedraw(WebviewId),
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
    Automation(AutomationCommand),
    NewWebview(WebviewId, String),
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
    /// The app was asked to terminate (e.g. via the CDP `Browser.close`
    /// command).
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
            "[mac-embedder] Embedder::new_webview webview={:?} target={}",
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
        clipboard_get_text(self.sink.as_ref())
    }

    fn clipboard_set_text(&self, text: String) -> Result<(), String> {
        clipboard_set_text(self.sink.as_ref(), text)
    }

    fn title_changed(&self, webview_id: WebviewId, title: String) -> Result<(), String> {
        self.sink
            .send(FormalWebUserEvent::TitleChanged { webview_id, title })
    }

    fn new_web_content_scene(
        &self,
        _webview_id: WebviewId,
        _scene_bytes: Vec<u8>,
        _font_registrations: Vec<ipc_messages::content::RegisteredFont>,
        _font_data: HashMap<usize, Vec<u8>>,
    ) -> Result<(), String> {
        // The scene is presented via the IOSurface surface path
        // (`new_web_content_layers`); the scene-bytes payload is unused.
        Ok(())
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
