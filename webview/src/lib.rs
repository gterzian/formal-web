pub mod ui_event;

use anyrender::Scene as RenderScene;
use ipc_messages::content::{FontTransportReceiver, PaintFrame, ScrollOffset, WebviewId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Clone)]
pub struct WebviewState {
    pub scene: Option<RenderScene>,
    pub viewport_scroll: ScrollOffset,
    pub current_document_id: Option<u64>,
}

impl Default for WebviewState {
    fn default() -> Self {
        Self {
            scene: None,
            viewport_scroll: ScrollOffset { x: 0.0, y: 0.0 },
            current_document_id: None,
        }
    }
}

pub trait EmbedderApi: Send + Sync {
    fn request_redraw(&self, webview_id: WebviewId);
}

#[derive(Clone, Copy)]
pub struct RuntimeHooks {
    pub handle_runtime_message: fn(&str),
    pub start_navigation_parts: fn(usize, usize, &str, &str, &str, bool) -> Result<(), String>,
    pub note_rendering_opportunity: fn(&str),
}

static RUNTIME_HOOKS: LazyLock<Mutex<Option<RuntimeHooks>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn set_runtime_hooks(hooks: RuntimeHooks) {
    let mut guard = RUNTIME_HOOKS.lock().expect("runtime hooks mutex poisoned");
    *guard = Some(hooks);
}

fn runtime_hooks() -> Result<RuntimeHooks, String> {
    RUNTIME_HOOKS
        .lock()
        .expect("runtime hooks mutex poisoned")
        .as_ref()
        .copied()
        .ok_or_else(|| String::from("webview runtime hooks are not initialized"))
}

fn call_lean_runtime_message_handler(message: &str) {
    if let Ok(hooks) = runtime_hooks() {
        (hooks.handle_runtime_message)(message);
    }
}

fn call_lean_navigation_start_parts(
    event_loop_id: usize,
    webview_id: usize,
    destination_url: &str,
    target: &str,
    user_involvement: &str,
    noopener: bool,
) -> Result<(), String> {
    let hooks = runtime_hooks()?;
    (hooks.start_navigation_parts)(
        event_loop_id,
        webview_id,
        destination_url,
        target,
        user_involvement,
        noopener,
    )
}

fn user_agent_note_rendering_opportunity(message: &str) {
    if let Ok(hooks) = runtime_hooks() {
        (hooks.note_rendering_opportunity)(message);
    }
}

fn startup_runtime_message_for(startup_url: Option<&str>) -> Result<String, String> {
    let startup_url = match startup_url {
        Some(url) => url.to_owned(),
        None => startup_artifact_url()?,
    };
    Ok(format!("FreshTopLevelTraversable|{startup_url}"))
}

fn startup_artifact_url() -> Result<String, String> {
    const STARTUP_ARTIFACT_RELATIVE_PATH: &str = "artifacts/StartupExample.html";
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("failed to determine current directory: {error}"))?;
    let artifact_path: PathBuf = current_dir.join(STARTUP_ARTIFACT_RELATIVE_PATH);
    let artifact_path = artifact_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve startup artifact path: {error}"))?;
    Ok(format!("file://{}", artifact_path.display()))
}

pub struct WebviewProvider {
    webviews: HashMap<WebviewId, WebviewState>,
    font_receiver: FontTransportReceiver,
    embedder: Arc<dyn EmbedderApi>,
}

impl WebviewProvider {
    pub fn new(embedder: Arc<dyn EmbedderApi>) -> Self {
        Self {
            webviews: HashMap::new(),
            font_receiver: FontTransportReceiver::default(),
            embedder,
        }
    }

    pub fn start(&self, startup_url: Option<&str>) -> Result<(), String> {
        let message = startup_runtime_message_for(startup_url)?;
        call_lean_runtime_message_handler(&message);
        Ok(())
    }

    pub fn navigate(&self, webview_id: Option<WebviewId>, url: &str) -> Result<(), String> {
        match webview_id {
            Some(webview_id) => call_lean_navigation_start_parts(
                webview_id.0 as usize,
                webview_id.0 as usize,
                url,
                "",
                "browser-ui",
                false,
            ),
            None => {
                let message = format!("FreshTopLevelTraversable|{url}");
                call_lean_runtime_message_handler(&message);
                Ok(())
            }
        }
    }

    pub fn send_ui_event(&self, _webview_id: WebviewId, event: blitz_traits::events::UiEvent) -> Result<(), String> {
        const DISPATCH_EVENT_MESSAGE_PREFIX: &str = "DispatchEvent|";
        let event_message = ui_event::serialize_ui_event(&event)?;
        let message = format!("{DISPATCH_EVENT_MESSAGE_PREFIX}{event_message}");
        call_lean_runtime_message_handler(&message);
        user_agent_note_rendering_opportunity("ui_event");
        Ok(())
    }

    pub fn note_rendering_opportunity(&self, reason: &str) {
        user_agent_note_rendering_opportunity(reason);
    }

    pub fn on_paint_frame(&mut self, frame: PaintFrame) -> Result<(), String> {
        let traversable_id = frame.traversable_id;
        let document_id = frame.frame_id.0;
        let viewport_scroll = frame.viewport_scroll.clone();
        let recorded_scene = frame.into_recorded_scene(&mut self.font_receiver)?;
        let scene = recorded_scene.into_scene(&self.font_receiver);

        let state = self.webviews.entry(traversable_id).or_default();
        state.scene = Some(scene);
        state.viewport_scroll = viewport_scroll;
        state.current_document_id = Some(document_id);

        self.embedder.request_redraw(traversable_id);
        Ok(())
    }

    pub fn on_finalize_navigation(&mut self, webview_id: WebviewId, _url: &str) {
        self.webviews.entry(webview_id).or_default();
    }

    pub fn on_new_top_level_traversable(&mut self, webview_id: WebviewId) {
        self.webviews.entry(webview_id).or_default();
    }

    pub fn current_scene(&self, webview_id: WebviewId) -> Option<(&RenderScene, ScrollOffset)> {
        let state = self.webviews.get(&webview_id)?;
        let scene = state.scene.as_ref()?;
        Some((scene, state.viewport_scroll.clone()))
    }

    pub fn current_document_id(&self, webview_id: WebviewId) -> Option<u64> {
        self.webviews
            .get(&webview_id)
            .and_then(|state| state.current_document_id)
    }

    pub fn scroll_offset(&self, webview_id: WebviewId) -> ScrollOffset {
        self.webviews
            .get(&webview_id)
            .map(|state| state.viewport_scroll.clone())
            .unwrap_or(ScrollOffset { x: 0.0, y: 0.0 })
    }
}
