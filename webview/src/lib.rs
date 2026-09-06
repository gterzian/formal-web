pub mod ui_event;

pub use blitz_traits::SmolStr;
pub use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, PointerCoords, PointerDetails,
    UiEvent,
};
pub use blitz_traits::shell::ColorScheme;

// The embedder-facing type vocabulary: the IDs, payloads, and helpers whose
// definitions live in the ipc crates (the wire types of the content and
// graphics processes) are re-exported here so embedder crates depend on
// `webview` alone and never name the ipc crates directly.
#[cfg(target_os = "macos")]
pub use ipc_channel::platform::deallocate_mach_port;
pub use ipc_messages::content::deserialize_scene_from_slice;
pub use ipc_messages::content::{
    FontTransportReceiver, RecordedScene, RegisteredFont, UserScript, WebviewId,
};
pub use ipc_messages::graphics::{CompositingLayerId, LayerFrame, SurfaceFrame};

use ipc_messages::content::{NavigateRequest, UserNavigationInvolvement};
use log::{debug, error, trace};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use user_agent::{UserAgent, UserAgentHost};
use verification::TraceSender;

pub use user_agent::{NavigationCompleted, NavigationCompletion};

/// The embedder host interface: the callbacks the user agent makes into the
/// embedder (navigation, paint, clipboard, viewport, window title). The
/// embedder crates implement this trait; the webview crate adapts it to the
/// user agent's own host interface (`user_agent::UserAgentHost`).
pub trait Embedder: Send + Sync {
    fn navigation_requested(
        &self,
        webview_id: WebviewId,
        destination_url: String,
    ) -> Result<(), String>;
    fn navigation_completed(&self, completed: NavigationCompleted) -> Result<(), String>;
    fn new_webview(&self, webview_id: WebviewId, target_name: String) -> Result<(), String>;
    fn request_redraw(&self, webview_id: WebviewId);
    fn viewport_scale_factor(&self) -> f32;
    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)>;
    fn clipboard_get_text(&self) -> Result<String, String>;
    fn clipboard_set_text(&self, text: String) -> Result<(), String>;
    /// A message a document sent with
    /// `window.__formalWebPostHostMessage(body)`, with the sending
    /// document's URL. The embedder answers, when it answers at all, by
    /// evaluating a script in the same webview.
    fn host_message(&self, webview_id: WebviewId, url: String, body: String)
    -> Result<(), String>;
    /// The parsed title of a top-level document, reported by the content
    /// process after parsing; the embedder labels the tab and window with it.
    /// <https://html.spec.whatwg.org/#the-title-element>
    fn title_changed(&self, webview_id: WebviewId, title: String) -> Result<(), String>;
    /// Forward a composed web content scene from the graphics process to the
    /// embedder for rendering.
    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<RegisteredFont>,
        font_data: HashMap<usize, Vec<u8>>,
    ) -> Result<(), String>;
    /// Forward the per-layer rendered frame from the graphics process. Each
    /// layer carries its wire `topology` (transform, clip, z-order) plus the
    /// actual `frame` only when the layer was re-rendered this cycle; a clean
    /// layer keeps its last surface and carries `frame: None`. `animating`
    /// reports whether the composed scene contains animated content (video,
    /// CSS animations) that needs the next frame at display cadence.
    fn new_web_content_layers(
        &self,
        webview_id: WebviewId,
        layers: Vec<LayerFrame>,
        animating: bool,
    ) -> Result<(), String>;
}

/// Bridges the user agent's host interface to the embedder-facing
/// [`Embedder`] trait: the user agent is started with an adapter that
/// forwards every host call to the embedder's implementation.
struct UserAgentHostAdapter {
    embedder: Arc<dyn Embedder>,
}

impl UserAgentHost for UserAgentHostAdapter {
    fn navigation_requested(
        &self,
        webview_id: WebviewId,
        destination_url: String,
    ) -> Result<(), String> {
        self.embedder
            .navigation_requested(webview_id, destination_url)
    }

    fn navigation_completed(&self, completed: NavigationCompleted) -> Result<(), String> {
        self.embedder.navigation_completed(completed)
    }

    fn new_webview(&self, webview_id: WebviewId, target_name: String) -> Result<(), String> {
        self.embedder.new_webview(webview_id, target_name)
    }

    fn request_redraw(&self, webview_id: WebviewId) {
        self.embedder.request_redraw(webview_id);
    }

    fn viewport_scale_factor(&self) -> f32 {
        self.embedder.viewport_scale_factor()
    }

    fn window_viewport_snapshot(&self) -> Option<(u32, u32, f32, ColorScheme)> {
        self.embedder.window_viewport_snapshot()
    }

    fn clipboard_get_text(&self) -> Result<String, String> {
        self.embedder.clipboard_get_text()
    }

    fn clipboard_set_text(&self, text: String) -> Result<(), String> {
        self.embedder.clipboard_set_text(text)
    }

    fn host_message(
        &self,
        webview_id: WebviewId,
        url: String,
        body: String,
    ) -> Result<(), String> {
        self.embedder.host_message(webview_id, url, body)
    }

    fn title_changed(&self, webview_id: WebviewId, title: String) -> Result<(), String> {
        self.embedder.title_changed(webview_id, title)
    }

    fn new_web_content_scene(
        &self,
        webview_id: WebviewId,
        scene_bytes: Vec<u8>,
        font_registrations: Vec<RegisteredFont>,
        font_data: HashMap<usize, Vec<u8>>,
    ) -> Result<(), String> {
        self.embedder
            .new_web_content_scene(webview_id, scene_bytes, font_registrations, font_data)
    }

    fn new_web_content_layers(
        &self,
        webview_id: WebviewId,
        layers: Vec<LayerFrame>,
        animating: bool,
    ) -> Result<(), String> {
        self.embedder
            .new_web_content_layers(webview_id, layers, animating)
    }
}

fn startup_destination_url(startup_url: Option<&str>) -> Result<String, String> {
    match startup_url {
        Some(url) => Ok(url.to_owned()),
        None => startup_artifact_url(),
    }
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

fn input_debug_enabled() -> bool {
    env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

pub struct WebviewProvider {
    embedder: Arc<dyn Embedder>,
    user_agent: UserAgent,
}

impl WebviewProvider {
    /// `helper_directory` is where the embedder keeps `formal-web-content`,
    /// `formal-web-net` and `formal-web-graphics`; it is searched before the
    /// directory of the current executable. `None` leaves the engine's own
    /// search in charge.
    pub fn new(
        embedder: Arc<dyn Embedder>,
        trace_sender: Option<TraceSender>,
        helper_directory: Option<PathBuf>,
    ) -> Result<Self, String> {
        let user_agent = UserAgent::start(
            Arc::new(UserAgentHostAdapter {
                embedder: embedder.clone(),
            }),
            trace_sender,
            helper_directory,
        )?;

        Ok(Self {
            embedder,
            user_agent,
        })
    }

    pub fn start(&self, startup_url: Option<&str>) -> Result<(), String> {
        let destination_url = startup_destination_url(startup_url)?;
        self.user_agent.start_top_level_traversable(destination_url)
    }

    pub fn navigate(&self, webview_id: Option<WebviewId>, url: &str) -> Result<(), String> {
        match webview_id {
            Some(webview_id) => {
                let navigable_id = webview_id.0;
                self.user_agent.start_navigation(NavigateRequest {
                    navigation_id: None,
                    source_navigable_id: navigable_id,
                    chosen_navigable_id: None,
                    destination_url: url.to_owned(),
                    target: String::new(),
                    user_involvement: UserNavigationInvolvement::BrowserUi,
                    noopener: false,
                    referrer_policy: None,
                    features_json: None,
                    new_traversable_info: None,
                    new_child_navigable: None,
                })
            }
            None => self.user_agent.start_top_level_traversable(url.to_owned()),
        }
    }

    pub fn send_ui_event(&self, webview_id: WebviewId, event: UiEvent) -> Result<(), String> {
        match ui_event::serialize_ui_event(&event) {
            Ok(event_message) => {
                let _ = self.user_agent.send_ui_event(webview_id, event_message);
            }
            Err(error) => {
                error!("failed to serialize ui event: {error}");
            }
        }
        Ok(())
    }

    pub fn set_default_viewport(
        &self,
        snapshot: Option<(u32, u32, f32, ColorScheme)>,
    ) -> Result<(), String> {
        self.user_agent.set_default_viewport(snapshot)
    }

    /// The scripts every webview created from now on starts with.
    ///
    /// Publish them before asking for a webview: the traversable's first
    /// document is created as part of that call, and only scripts already
    /// published run in it.
    pub fn set_default_user_scripts(&self, scripts: Vec<UserScript>) -> Result<(), String> {
        self.user_agent.set_default_user_scripts(scripts)
    }

    /// The scripts run in each of a webview's documents before the document
    /// is populated. The list replaces the previous one.
    pub fn set_user_scripts(
        &self,
        webview_id: WebviewId,
        scripts: Vec<UserScript>,
    ) -> Result<(), String> {
        self.user_agent.set_user_scripts(webview_id.0, scripts)
    }

    pub fn set_traversable_viewport(
        &self,
        traversable_id: WebviewId,
        snapshot: (u32, u32, f32, ColorScheme),
        offset_x: f32,
        offset_y: f32,
    ) -> Result<(), String> {
        self.user_agent
            .set_traversable_viewport(traversable_id.0, snapshot, offset_x, offset_y)
    }

    /// Notify the UA that the embedder is about to paint a frame for
    /// `webview_id`; the UA gates render cycles on this.
    pub fn frame_needed(&self, webview_id: WebviewId) -> Result<(), String> {
        self.user_agent.frame_needed(webview_id)
    }

    pub fn evaluate_script(
        &self,
        traversable_id: WebviewId,
        source: String,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let cdp_debug_enabled = std::env::var_os("FORMAL_WEB_DEBUG_CDP").is_some();
        if cdp_debug_enabled {
            debug!(
                "[cdp][webview] evaluate enter traversable={:?} len={} timeout_ms={}",
                traversable_id,
                source.len(),
                timeout.as_millis()
            );
        }
        let result = self
            .user_agent
            .evaluate_script(traversable_id.0, source, timeout);
        if cdp_debug_enabled {
            debug!(
                "[cdp][webview] evaluate exit ok={} traversable={:?}",
                result.is_ok(),
                traversable_id
            );
        }
        result
    }

    pub fn click_element(&self, traversable_id: WebviewId, selector: String) -> Result<(), String> {
        self.user_agent.click_element(traversable_id.0, selector)
    }

    pub fn on_navigation_committed(&mut self, webview_id: WebviewId) {
        if input_debug_enabled() {
            trace!(
                "[input-debug][webview] navigation_committed webview={}",
                webview_id.0
            );
        }
        self.embedder.request_redraw(webview_id);
    }
}
