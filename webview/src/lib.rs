pub mod ui_event;

use anyrender::{PaintScene, Scene as RenderScene};
use blitz_traits::events::UiEvent;
use blitz_traits::shell::ColorScheme;
use crossbeam_channel::{Receiver, unbounded};
use ipc_messages::content::{
    FrameId, NavigableId, NavigateRequest,
    UserNavigationInvolvement, WebviewId, WebviewProviderMessage,
};
use ipc_messages::graphics::FrameHitInfo;
use ipc_messages::media::VideoPaintId;
use kurbo::Affine;
use log::{debug, error, trace};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use user_agent::UserAgent;
use verification::TraceSender;

pub use user_agent::{Embedder, NavigationCompleted, NavigationCompletion};

/// Viewport info for a visible child frame, published to the UA so the
/// child content process can determine its visible region.
/// In the new architecture this data comes from the graphics process.
#[derive(Clone, Debug)]
pub struct VisibleFrameViewport {
    pub frame_id: FrameId,
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub struct WebviewState {
    pub current_navigable_id: Option<NavigableId>,
    focused_frame_id: Option<FrameId>,
}

impl Default for WebviewState {
    fn default() -> Self {
        Self {
            current_navigable_id: None,
            focused_frame_id: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ChildNavigableHost {
    parent_traversable_id: WebviewId,
    content_frame_id: FrameId,
}

#[derive(Clone, PartialEq)]
struct PublishedChildViewport {
    width: u32,
    height: u32,
    scale: f32,
    color_scheme: ColorScheme,
    offset_x: f32,
    offset_y: f32,
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
    webviews: HashMap<WebviewId, WebviewState>,
    child_navigable_hosts_by_webview: HashMap<WebviewId, ChildNavigableHost>,
    child_host_webviews_by_content_navigable: HashMap<FrameId, WebviewId>,
    published_child_viewports: HashMap<WebviewId, PublishedChildViewport>,
    /// The latest frame hit info per webview, published by the graphics process.
    frame_hit_info: HashMap<WebviewId, Vec<FrameHitInfo>>,

    viewport_snapshot: Option<(u32, u32, f32, ColorScheme)>,
    embedder: Arc<dyn Embedder>,
    user_agent: UserAgent,
    provider_message_receiver: Receiver<WebviewProviderMessage>,
}

impl WebviewProvider {
    pub fn new(
        embedder: Arc<dyn Embedder>,
        trace_sender: Option<TraceSender>,
    ) -> Result<Self, String> {
        let (provider_message_sender, provider_message_receiver) = unbounded();
        let user_agent = UserAgent::start(embedder.clone(), provider_message_sender, trace_sender)?;

        Ok(Self {
            webviews: HashMap::new(),
            child_navigable_hosts_by_webview: HashMap::new(),
            child_host_webviews_by_content_navigable: HashMap::new(),
            published_child_viewports: HashMap::new(),
            frame_hit_info: HashMap::new(),
            viewport_snapshot: None,
            embedder,
            user_agent,
            provider_message_receiver,
        })
    }

    /// Store the latest frame hit info from the graphics process.
    /// Used by `route_ui_event` for hit-testing.
    pub fn store_frame_hit_info(
        &mut self,
        webview_id: WebviewId,
        hit_info: Vec<FrameHitInfo>,
    ) {
        self.frame_hit_info.insert(webview_id, hit_info);
    }

    /// Process ALL pending provider messages in one batch.
    ///
    /// Processes one message from the pending queue. The caller (`WebviewProviderSync`)
    /// is dispatched once per enqueued message, so we process exactly one message here.
    ///
    /// NOTE: We deliberately do NOT drain additional messages via `try_recv()`. While
    /// draining would let us batch multiple PaintFrames (e.g. during iframe viewport
    /// convergence), it creates a hang when the user-agent queues two
    /// `WebviewProviderSync` events before the embedder processes the first one.
    /// The first sync drains both messages, leaving the second sync with nothing and
    /// blocking `recv()` forever. Processing one message per sync avoids this.
    pub fn sync_pending_messages(&mut self) -> Result<(), String> {
        let message = self
            .provider_message_receiver
            .recv()
            .map_err(|error| format!("failed to receive webview provider message: {error}"))?;
        self.handle_provider_message(message)?;

        Ok(())
    }

    fn handle_provider_message(&mut self, message: WebviewProviderMessage) -> Result<(), String> {
        match message {
            WebviewProviderMessage::PaintFrame {
                ..
            } => {
                // PaintFrames go directly to the graphics process.
                // The UA receives PaintReady for bookkeeping only.
                Ok(())
            }
            WebviewProviderMessage::RegisterChildNavigableHost {
                child_webview_id,
                parent_traversable_id,
                content_frame_id,
            } => {
                self.register_child_navigable_host(
                    child_webview_id,
                    parent_traversable_id,
                    content_frame_id,
                );
                Ok(())
            }
            WebviewProviderMessage::NewWebview { webview_id } => {
                self.on_new_webview(webview_id);
                Ok(())
            }
            WebviewProviderMessage::VideoFrameReady {
                ..
            } => {
                // Video frames go directly to the graphics process.
                Ok(())
            }
        }
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

    /// Route a UI event to the correct frame using frame hit info from the
    /// graphics process. If no hit info is available, route directly to the
    /// root webview (which is correct for single-frame pages).
    fn route_ui_event(
        &self,
        root_webview_id: WebviewId,
        event: UiEvent,
    ) -> (WebviewId, UiEvent, Vec<FrameId>) {
        // Try to find the target frame via hit-testing using FrameHitInfo.
        // For pointer events, find which frame's clip bounds contain the pointer.
        if let Some(hit_info_list) = self.frame_hit_info.get(&root_webview_id) {
            if let Some((coords_x, coords_y)) = pointer_coords(&event) {
                // Walk the hit info list in root-to-leaf order, checking clip bounds.
                for info in hit_info_list.iter().rev() {
                    // Check if pointer falls within this frame's root clip bounds.
                    if coords_x >= info.root_clip_bounds[0]
                        && coords_y >= info.root_clip_bounds[1]
                        && coords_x <= info.root_clip_bounds[2]
                        && coords_y <= info.root_clip_bounds[3]
                    {
                        // Convert coordinates to the frame's local space.
                        let local_x = coords_x - info.root_clip_bounds[0];
                        let local_y = coords_y - info.root_clip_bounds[1];
                        let routed_event = translate_event_coords(&event, local_x as f32, local_y as f32);

                        // Check if this frame belongs to a child webview.
                        if let Some(child_host) = info
                            .child_frame_ids
                            .iter()
                            .find_map(|child_id| {
                                self.child_host_webviews_by_content_navigable
                                    .get(child_id)
                            })
                        {
                            return (*child_host, routed_event, Vec::new());
                        }

                        return (root_webview_id, routed_event, vec![info.frame_id]);
                    }
                }
            }
        }

        // Fallback: route directly to the root webview.
        (root_webview_id, event, Vec::new())
    }

    pub fn send_ui_event(&mut self, webview_id: WebviewId, event: UiEvent) -> Result<(), String> {
        self.embedder.request_redraw(webview_id);
        let (target_webview_id, routed_event, composed_frame_ids) =
            self.route_ui_event(webview_id, event);
        let event_message = ui_event::serialize_ui_event(&routed_event)?;
        self.user_agent
            .dispatch_event_for(target_webview_id.0, event_message)?;
        self.note_rendering_opportunities(webview_id, composed_frame_ids, "ui_event");
        Ok(())
    }

    pub fn note_rendering_opportunity(&self, webview_id: WebviewId, reason: &str) {
        let _ = reason;
        if let Err(error) = self.user_agent.note_rendering_opportunity(webview_id.0) {
            error!("failed to note rendering opportunity for webview {webview_id:?}: {error}");
        }
    }

    pub fn set_default_viewport(
        &mut self,
        snapshot: Option<(u32, u32, f32, ColorScheme)>,
    ) -> Result<(), String> {
        self.viewport_snapshot = snapshot;
        self.user_agent.set_default_viewport(snapshot)
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

    pub fn register_child_navigable_host(
        &mut self,
        child_host_webview_id: WebviewId,
        parent_traversable_id: WebviewId,
        content_frame_id: FrameId,
    ) {
        self.child_navigable_hosts_by_webview.insert(
            child_host_webview_id,
            ChildNavigableHost {
                parent_traversable_id,
                content_frame_id,
            },
        );
        self.child_host_webviews_by_content_navigable
            .insert(content_frame_id, child_host_webview_id);
    }

    pub fn on_new_webview(&mut self, webview_id: WebviewId) {
        if self.webviews.contains_key(&webview_id) {
            return;
        }
        debug!("[webview] new webview {:?}", webview_id);
        self.webviews.insert(webview_id, WebviewState::default());
    }

    pub fn note_rendering_opportunities(
        &self,
        root_webview_id: WebviewId,
        frame_ids: Vec<FrameId>,
        reason: &str,
    ) {
        self.note_rendering_opportunity(root_webview_id, reason);
        for frame_id in frame_ids {
            if let Some(child_webview_id) = self
                .child_host_webviews_by_content_navigable
                .get(&frame_id)
            {
                self.note_rendering_opportunity(*child_webview_id, reason);
            }
        }
    }

    pub fn composed_scene_for_webview(
        &mut self,
        _webview_id: WebviewId,
    ) -> Option<RenderScene> {
        // Scene composition is now handled by the graphics process.
        // This method should not be called — the embedder uses
        // WindowedApp::composed_scenes directly.
        None
    }

    pub fn append_web_content_scene(
        &mut self,
        _webview_id: WebviewId,
        _target_scene: &mut impl PaintScene,
        _transform: Affine,
    ) -> bool {
        // Scenes are composed by the graphics process and stored directly
        // in the embedder. This path is obsolete.
        false
    }

    pub fn current_scene(&mut self, _webview_id: WebviewId) -> Option<RenderScene> {
        None
    }

    pub fn visible_frame_viewports(&mut self, _webview_id: WebviewId) -> Vec<VisibleFrameViewport> {
        // Viewport computation is now done in the graphics process compositor.
        Vec::new()
    }

    pub fn current_navigable_id(&self, webview_id: WebviewId) -> Option<NavigableId> {
        self.webviews
            .get(&webview_id)
            .and_then(|state| state.current_navigable_id)
    }

    fn publish_visible_child_viewports(&mut self, _viewports: Vec<VisibleFrameViewport>) {
        // Child viewport publishing is now handled by the graphics process
        // via FrameHitInfo.
    }

    pub fn on_navigation_committed(&mut self, webview_id: WebviewId) {
        if let Some(child_navigable_host) = self.child_navigable_hosts_by_webview.get(&webview_id)
        {
            let parent_traversable_id = child_navigable_host.parent_traversable_id;
            if input_debug_enabled() {
                trace!(
                    "[input-debug][webview] navigation_committed child_webview={} parent_webview={}",
                    webview_id.0, parent_traversable_id.0,
                );
            }

            // Child frame navigation is handled by the graphics process.
            // Request rendering opportunities so the new content appears promptly.
            self.note_rendering_opportunity(webview_id, "child_navigation_committed");
            self.embedder.request_redraw(webview_id);
            return;
        }

        if input_debug_enabled() {
            trace!(
                "[input-debug][webview] navigation_committed top_level_webview={}",
                webview_id.0
            );
        }
    }

    pub fn embedder(&self) -> &Arc<dyn Embedder> {
        &self.embedder
    }
}

/// Extract pointer coordinates from a UI event, if applicable.
fn pointer_coords(event: &UiEvent) -> Option<(f64, f64)> {
    match event {
        UiEvent::PointerMove(e)
        | UiEvent::PointerUp(e)
        | UiEvent::PointerDown(e) => {
            Some((f64::from(e.coords.client_x), f64::from(e.coords.client_y)))
        }
        UiEvent::Wheel(e) => {
            Some((f64::from(e.coords.client_x), f64::from(e.coords.client_y)))
        }
        _ => None,
    }
}

/// Translate event coordinates by an offset (for hit-tested child frames).
fn translate_event_coords(event: &UiEvent, dx: f32, dy: f32) -> UiEvent {
    let mut translated = event.clone();
    match &mut translated {
        UiEvent::PointerMove(e)
        | UiEvent::PointerUp(e)
        | UiEvent::PointerDown(e) => {
            e.coords.page_x -= dx;
            e.coords.page_y -= dy;
            e.coords.client_x -= dx;
            e.coords.client_y -= dy;
        }
        UiEvent::Wheel(e) => {
            e.coords.page_x -= dx;
            e.coords.page_y -= dy;
            e.coords.client_x -= dx;
            e.coords.client_y -= dy;
        }
        _ => {}
    }
    translated
}
