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
    EmbedderSchemeFetchId, FontTransportReceiver, RecordedScene, RegisteredFont, UserScript,
    WebviewId,
};
pub use ipc_messages::graphics::{CompositingLayerId, LayerFrame, SurfaceFrame};

use ipc_messages::content::{NavigateRequest, UserNavigationInvolvement};
use log::{debug, error, trace};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use user_agent::UserAgent;
use verification::TraceSender;

pub use user_agent::{
    Embedder, EmbedderConfig, EmbedderSchemeRequest, EmbedderSchemeResponse, NavigationCompleted,
    NavigationCompletion,
};

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
    /// Start the engine. `config` carries what the embedder settles once and
    /// for all: where the extension executables live, the URL schemes it
    /// serves itself, and the scripts a webview it did not ask for by name
    /// carries. None of it changes afterwards, so nothing a webview depends
    /// on can arrive after the webview does.
    pub fn new(
        embedder: Arc<dyn Embedder>,
        trace_sender: Option<TraceSender>,
        config: EmbedderConfig,
    ) -> Result<Self, String> {
        let user_agent = UserAgent::start(embedder.clone(), trace_sender, config)?;

        Ok(Self {
            embedder,
            user_agent,
        })
    }

    /// Create the first webview and navigate it to `startup_url`.
    ///
    /// `user_scripts` are run in each of this webview's documents before the
    /// document is populated, the first one included: they travel with the
    /// request that creates the webview, so there is no window in which a
    /// document could be created without them.
    pub fn start(
        &self,
        startup_url: Option<&str>,
        user_scripts: Vec<UserScript>,
    ) -> Result<(), String> {
        let destination_url = startup_destination_url(startup_url)?;
        self.user_agent
            .start_top_level_traversable(destination_url, user_scripts)
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
            // A webview created here was not asked for by name, so it
            // carries `EmbedderConfig::default_user_scripts`.
            None => self
                .user_agent
                .start_top_level_traversable(url.to_owned(), Vec::new()),
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

    /// The response to the embedder-scheme fetch named by `request_id`,
    /// which reached the embedder as [`EmbedderSchemeRequest::id`].
    ///
    /// Callable from any thread and at any time after the request: an
    /// embedder that has to go away and come back with the bytes keeps the
    /// id and answers here when it has them.
    pub fn complete_embedder_scheme_fetch(
        &self,
        request_id: EmbedderSchemeFetchId,
        response: EmbedderSchemeResponse,
    ) -> Result<(), String> {
        self.user_agent
            .complete_embedder_scheme_fetch(request_id, response)
    }

    /// Fail the embedder-scheme fetch named by `request_id`: the embedder
    /// has no response for it.
    ///
    /// A fetch that is neither completed nor failed stays pending, and the
    /// document waiting on it never loads, so an embedder that gives up on
    /// a request says so here.
    pub fn fail_embedder_scheme_fetch(
        &self,
        request_id: EmbedderSchemeFetchId,
        reason: String,
    ) -> Result<(), String> {
        self.user_agent
            .fail_embedder_scheme_fetch(request_id, reason)
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
