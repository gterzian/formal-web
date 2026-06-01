mod compositor;
pub mod ui_event;

use anyrender::{PaintScene, Scene as RenderScene};
use blitz_traits::events::UiEvent;
use blitz_traits::shell::ColorScheme;
use compositor::Compositor;
use crossbeam_channel::{Receiver, unbounded};
use ipc_messages::content::{
    FontTransportReceiver, FrameId, NavigableId, NavigateRequest, PaintFrame,
    UserNavigationInvolvement, WebviewId, WebviewProviderMessage,
};
use kurbo::Affine;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use user_agent::UserAgent;
use verification::TraceSender;

pub use compositor::VisibleFrameViewport;
pub use user_agent::{Embedder, NavigationCompleted, NavigationCompletion};

#[derive(Clone)]
pub struct WebviewState {
    pub compositor: Compositor,
    pub current_navigable_id: Option<NavigableId>,
    focused_frame_id: Option<FrameId>,
}

impl Default for WebviewState {
    fn default() -> Self {
        Self {
            compositor: Compositor::default(),
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
    font_receiver: FontTransportReceiver,
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
            font_receiver: FontTransportReceiver::default(),
            viewport_snapshot: None,
            embedder,
            user_agent,
            provider_message_receiver,
        })
    }

    pub fn sync_pending_messages(&mut self) -> Result<(), String> {
        let message = self
            .provider_message_receiver
            .recv()
            .map_err(|error| format!("failed to receive webview provider message: {error}"))?;
        self.handle_provider_message(message)
    }

    fn handle_provider_message(&mut self, message: WebviewProviderMessage) -> Result<(), String> {
        match message {
            WebviewProviderMessage::PaintFrame(frame) => self.on_paint_frame(frame),
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
                })
            }
            None => self.user_agent.start_top_level_traversable(url.to_owned()),
        }
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
            eprintln!("failed to note rendering opportunity for webview {webview_id:?}: {error}");
        }
    }

    pub fn set_default_viewport(
        &mut self,
        snapshot: Option<(u32, u32, f32, ColorScheme)>,
    ) -> Result<(), String> {
        self.viewport_snapshot = snapshot;
        let result = self.user_agent.set_default_viewport(snapshot);

        // Refresh child traversable viewport publications as soon as viewport metadata is
        // available (or changes). Without this, iframe content can keep a fallback viewport
        // until the next input-driven composition pass.
        let mut visible_viewports = Vec::new();
        for state in self.webviews.values_mut() {
            visible_viewports.extend(
                state
                    .compositor
                    .visible_frame_viewports(&self.font_receiver),
            );
        }
        self.publish_visible_child_viewports(visible_viewports);

        result
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
            eprintln!(
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
            eprintln!(
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

        // A parent frame can already have composed iframe embed-site geometry before the child
        // traversable registration arrives. Publish immediately so the child gets the
        // correct viewport on first paint instead of waiting for later input-driven redraws.
        if let Some(state) = self.webviews.get_mut(&parent_traversable_id) {
            let viewports = state
                .compositor
                .visible_frame_viewports(&self.font_receiver);
            self.publish_visible_child_viewports(viewports);
        }

        // The parent may not have a composed frame tree yet when the child host registers.
        // Force a parent rendering opportunity so embed-site geometry becomes available and
        // child viewport publication does not wait for a later root scroll/input event.
        self.note_rendering_opportunity(parent_traversable_id, "child_host_registered");
        // Child frames can paint before host registration. Force a fresh child rendering
        // opportunity after registration so the next child paint is routed through the
        // parent-mapped content frame immediately, instead of waiting for later input.
        self.note_rendering_opportunity(child_host_webview_id, "child_host_registered");
        self.embedder.request_redraw(parent_traversable_id);
    }

    pub fn on_paint_frame(&mut self, mut frame: PaintFrame) -> Result<(), String> {
        let source_webview_id = frame.traversable_id;
        let is_root_candidate = !self
            .child_navigable_hosts_by_webview
            .contains_key(&frame.traversable_id);
        if let Some(child_navigable_host) = self
            .child_navigable_hosts_by_webview
            .get(&frame.traversable_id)
        {
            frame.traversable_id = child_navigable_host.parent_traversable_id;
            frame.frame_id = child_navigable_host.content_frame_id;
        }
        let traversable_id = frame.traversable_id;
        let frame_id = frame.frame_id;
        let viewport_width = frame.viewport_width;
        let viewport_height = frame.viewport_height;
        let composition = frame.composition.clone();
        if input_debug_enabled() {
            eprintln!(
                "[input-debug][webview] on_paint_frame traversable={} frame={} embeds={}",
                traversable_id.0,
                frame_id.0,
                composition.embed_sites.len()
            );
        }
        let recorded_scene = frame.into_recorded_scene(&mut self.font_receiver)?;

        let state = self.webviews.entry(traversable_id).or_default();
        state.compositor.store_frame(
            frame_id,
            viewport_width,
            viewport_height,
            composition,
            recorded_scene,
            is_root_candidate,
        );
        if state.compositor.committed_root_frame_id() == Some(frame_id) {
            state.current_navigable_id = Some(traversable_id.0);
        }

        // Publish child viewport bounds immediately after any paint update so iframe content
        // receives the correct viewport before the next user input-driven composition pass.
        let viewports = state
            .compositor
            .visible_frame_viewports(&self.font_receiver);
        self.publish_visible_child_viewports(viewports);

        if let Some(expected) = self.published_child_viewports.get(&source_webview_id)
            && (expected.width != viewport_width || expected.height != viewport_height)
        {
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] child_viewport_mismatch child_webview={} painted=({},{}) expected=({},{})",
                    source_webview_id.0,
                    viewport_width,
                    viewport_height,
                    expected.width,
                    expected.height,
                );
            }
            self.note_rendering_opportunity(source_webview_id, "child_viewport_mismatch");
        }

        self.embedder.request_redraw(traversable_id);
        Ok(())
    }

    pub fn on_navigation_committed(&mut self, webview_id: WebviewId) {
        if let Some(child_navigable_host) = self.child_navigable_hosts_by_webview.get(&webview_id) {
            let parent_traversable_id = child_navigable_host.parent_traversable_id;
            let content_frame_id = child_navigable_host.content_frame_id;
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] navigation_committed child_webview={} parent_webview={} content_frame={}",
                    webview_id.0, parent_traversable_id.0, content_frame_id.0,
                );
            }
            let state = self.webviews.entry(parent_traversable_id).or_default();
            state
                .compositor
                .note_child_navigation_finalized(content_frame_id);

            // Child frame caches are pruned on child navigation commit; request an immediate
            // child rendering opportunity so the refreshed child frame is available without
            // waiting for a later parent interaction to fan out rendering opportunities.
            self.note_rendering_opportunity(webview_id, "child_navigation_committed");

            // Child navigation commit can invalidate parent composition state. Request
            // immediate parent rendering so updated child geometry and scale are visible
            // without waiting for a manual root-page interaction.
            self.note_rendering_opportunity(parent_traversable_id, "child_navigation_committed");
            self.embedder.request_redraw(parent_traversable_id);
            return;
        }

        if input_debug_enabled() {
            eprintln!(
                "[input-debug][webview] navigation_committed top_level_webview={}",
                webview_id.0
            );
        }

        let state = self.webviews.entry(webview_id).or_default();
        state.compositor.note_navigation_finalized();
        state.current_navigable_id = None;
        state.focused_frame_id = None;
    }

    pub fn on_new_webview(&mut self, webview_id: WebviewId) {
        self.webviews.entry(webview_id).or_default();
    }

    fn composed_scene_for_webview(&mut self, webview_id: WebviewId) -> Option<RenderScene> {
        let (scene, viewports) = {
            let state = self.webviews.get_mut(&webview_id)?;
            let scene = state.compositor.compose_scene(&self.font_receiver);
            let viewports = state
                .compositor
                .visible_frame_viewports(&self.font_receiver);
            (scene, viewports)
        };
        self.publish_visible_child_viewports(viewports);
        scene
    }

    pub fn append_web_content_scene(
        &mut self,
        webview_id: WebviewId,
        target_scene: &mut impl PaintScene,
        transform: Affine,
    ) -> bool {
        let Some(scene) = self.composed_scene_for_webview(webview_id) else {
            return false;
        };
        if input_debug_enabled() {
            eprintln!(
                "[input-debug][webview] append_web_content_scene webview={}",
                webview_id.0
            );
        }
        target_scene.append_scene(scene, transform);
        true
    }

    pub fn current_scene(&mut self, webview_id: WebviewId) -> Option<RenderScene> {
        self.composed_scene_for_webview(webview_id)
    }

    pub fn visible_frame_viewports(&mut self, webview_id: WebviewId) -> Vec<VisibleFrameViewport> {
        let viewports = {
            let Some(state) = self.webviews.get_mut(&webview_id) else {
                return Vec::new();
            };
            state
                .compositor
                .visible_frame_viewports(&self.font_receiver)
        };
        self.publish_visible_child_viewports(viewports.clone());
        viewports
    }

    pub fn current_navigable_id(&self, webview_id: WebviewId) -> Option<NavigableId> {
        self.webviews
            .get(&webview_id)
            .and_then(|state| state.current_navigable_id)
    }

    fn route_ui_event(
        &mut self,
        root_webview_id: WebviewId,
        event: UiEvent,
    ) -> (WebviewId, UiEvent, Vec<FrameId>) {
        let viewport_scale = self.embedder.viewport_scale_factor().max(1.0);
        let is_pointer_down = matches!(&event, UiEvent::PointerDown(_));
        let Some((client_x, client_y)) = ui_event_client_position(&event) else {
            let (target_frame_id, root_frame_id, composed_frame_ids, viewports) = {
                let Some(state) = self.webviews.get_mut(&root_webview_id) else {
                    return (root_webview_id, event, Vec::new());
                };
                let root_frame_id = state.compositor.committed_root_frame_id();
                let viewports = state
                    .compositor
                    .visible_frame_viewports(&self.font_receiver);
                let composed_frame_ids = composition_frame_ids(root_frame_id, &viewports);
                let target_frame_id = state
                    .focused_frame_id
                    .filter(|frame_id| composed_frame_ids.contains(frame_id))
                    .or(root_frame_id);
                state.focused_frame_id = target_frame_id;
                (
                    target_frame_id,
                    root_frame_id,
                    composed_frame_ids,
                    viewports,
                )
            };
            self.publish_visible_child_viewports(viewports);
            let target_webview_id = target_frame_id
                .map(|frame_id| self.webview_id_for_frame(root_webview_id, frame_id))
                .unwrap_or(root_webview_id);
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] root={} frame={} child={} target={} nonpositional=true",
                    root_webview_id.0,
                    target_frame_id
                        .map(|frame_id| frame_id.0.to_string())
                        .unwrap_or_else(|| root_webview_id.0.to_string()),
                    target_frame_id.is_some_and(|frame_id| Some(frame_id) != root_frame_id),
                    target_webview_id.0,
                );
            }
            return (target_webview_id, event, composed_frame_ids);
        };

        let (root_frame_id, hit, viewports) = {
            let Some(state) = self.webviews.get_mut(&root_webview_id) else {
                return (root_webview_id, event, Vec::new());
            };
            let root_frame_id = state.compositor.committed_root_frame_id();
            let hit = state.compositor.hit_test(
                f64::from(client_x * viewport_scale),
                f64::from(client_y * viewport_scale),
                &self.font_receiver,
            );
            let viewports = state
                .compositor
                .visible_frame_viewports(&self.font_receiver);
            (root_frame_id, hit, viewports)
        };
        let composed_frame_ids = composition_frame_ids(root_frame_id, &viewports);

        let Some(hit) = hit else {
            if is_pointer_down && let Some(state) = self.webviews.get_mut(&root_webview_id) {
                state.focused_frame_id = root_frame_id;
            }
            self.publish_visible_child_viewports(viewports);
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] root={} client=({client_x:.1},{client_y:.1}) hit=none target={}",
                    root_webview_id.0, root_webview_id.0,
                );
            }
            return (root_webview_id, event, composed_frame_ids);
        };

        let target_webview_id = self.webview_id_for_frame(root_webview_id, hit.frame_id);
        let routed_event = retarget_ui_event_for_hit(event, hit, &viewports, viewport_scale);
        if is_pointer_down && let Some(state) = self.webviews.get_mut(&root_webview_id) {
            state.focused_frame_id = Some(hit.frame_id);
        }
        self.publish_visible_child_viewports(viewports);
        if input_debug_enabled() {
            let logical_local_x = hit.local_x / viewport_scale;
            let logical_local_y = hit.local_y / viewport_scale;
            eprintln!(
                "[input-debug][webview] root={} client=({client_x:.1},{client_y:.1}) frame={} child={} children={} target={} local=({:.1},{:.1})",
                root_webview_id.0,
                hit.frame_id.0,
                hit.is_child_frame,
                hit.has_child_frames,
                target_webview_id.0,
                logical_local_x,
                logical_local_y,
            );
        }
        (target_webview_id, routed_event, composed_frame_ids)
    }

    fn publish_visible_child_viewports(&mut self, viewports: Vec<VisibleFrameViewport>) {
        let Some((_, _, viewport_scale, color_scheme)) = self.viewport_snapshot else {
            self.published_child_viewports.clear();
            return;
        };
        let viewport_scale = viewport_scale.max(1.0);
        let mut next_published_child_viewports = HashMap::new();
        for viewport in viewports {
            let Some(child_webview_id) = self
                .child_host_webviews_by_content_navigable
                .get(&viewport.frame_id)
                .copied()
            else {
                continue;
            };

            let published_viewport = PublishedChildViewport {
                width: viewport.width,
                height: viewport.height,
                scale: viewport_scale,
                color_scheme: color_scheme.clone(),
                offset_x: viewport.offset_x / viewport_scale,
                offset_y: viewport.offset_y / viewport_scale,
            };
            let viewport_changed =
                self.published_child_viewports.get(&child_webview_id) != Some(&published_viewport);
            next_published_child_viewports.insert(child_webview_id, published_viewport.clone());

            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] publish_child_viewport child_webview={} frame={} changed={} size=({},{}) offset=({:.1},{:.1}) scale={:.2}",
                    child_webview_id.0,
                    viewport.frame_id.0,
                    viewport_changed,
                    published_viewport.width,
                    published_viewport.height,
                    published_viewport.offset_x,
                    published_viewport.offset_y,
                    published_viewport.scale,
                );
            }

            if !viewport_changed {
                continue;
            }

            if let Err(error) = self.set_traversable_viewport(
                child_webview_id,
                (
                    published_viewport.width,
                    published_viewport.height,
                    published_viewport.scale,
                    published_viewport.color_scheme.clone(),
                ),
                published_viewport.offset_x,
                published_viewport.offset_y,
            ) {
                eprintln!("[webview] failed to set traversable viewport: {error}");
            }
            self.note_rendering_opportunity(child_webview_id, "visible_child_viewport");

            // Also refresh the parent traversable. Embed-site clip/transform data is produced
            // by the parent frame paint, so parent composition must not wait for a later
            // root-page scroll/input event to pick up child viewport/layout changes.
            if let Some(child_host) = self.child_navigable_hosts_by_webview.get(&child_webview_id) {
                self.note_rendering_opportunity(
                    child_host.parent_traversable_id,
                    "visible_child_viewport_parent",
                );
                // Keep parent composition in lockstep with child viewport publication.
                // Without this immediate redraw request, the parent can keep showing a stale
                // child transform until a later parent interaction (for example, root scroll).
                self.embedder
                    .request_redraw(child_host.parent_traversable_id);
            }
        }
        self.published_child_viewports = next_published_child_viewports;
    }

    fn note_rendering_opportunities(
        &self,
        root_webview_id: WebviewId,
        composed_frame_ids: Vec<FrameId>,
        reason: &str,
    ) {
        let mut target_webview_ids = HashMap::new();
        target_webview_ids.insert(root_webview_id, ());
        for frame_id in composed_frame_ids {
            target_webview_ids.insert(self.webview_id_for_frame(root_webview_id, frame_id), ());
        }

        for webview_id in target_webview_ids.into_keys() {
            self.note_rendering_opportunity(webview_id, reason);
        }
    }

    pub fn webview_id_for_frame(&self, root_webview_id: WebviewId, frame_id: FrameId) -> WebviewId {
        self.child_host_webviews_by_content_navigable
            .get(&frame_id)
            .copied()
            .unwrap_or(root_webview_id)
    }
}

fn composition_frame_ids(
    root_frame_id: Option<FrameId>,
    viewports: &[VisibleFrameViewport],
) -> Vec<FrameId> {
    let mut frame_ids = Vec::with_capacity(viewports.len() + usize::from(root_frame_id.is_some()));
    if let Some(root_frame_id) = root_frame_id {
        frame_ids.push(root_frame_id);
    }
    frame_ids.extend(viewports.iter().map(|viewport| viewport.frame_id));
    frame_ids
}

fn retarget_ui_event_for_hit(
    mut event: UiEvent,
    hit: compositor::HitTestResult,
    viewports: &[VisibleFrameViewport],
    viewport_scale: f32,
) -> UiEvent {
    let Some(viewport) = viewports
        .iter()
        .find(|viewport| viewport.frame_id == hit.frame_id)
    else {
        return event;
    };

    let routed_client_x = (viewport.offset_x + hit.local_x) / viewport_scale;
    let routed_client_y = (viewport.offset_y + hit.local_y) / viewport_scale;
    match &mut event {
        UiEvent::PointerMove(event) | UiEvent::PointerUp(event) | UiEvent::PointerDown(event) => {
            event.coords.client_x = routed_client_x;
            event.coords.client_y = routed_client_y;
            event.coords.page_x = routed_client_x;
            event.coords.page_y = routed_client_y;
        }
        UiEvent::Wheel(event) => {
            event.coords.client_x = routed_client_x;
            event.coords.client_y = routed_client_y;
            event.coords.page_x = routed_client_x;
            event.coords.page_y = routed_client_y;
        }
        UiEvent::KeyUp(_)
        | UiEvent::KeyDown(_)
        | UiEvent::Ime(_)
        | UiEvent::AppleStandardKeybinding(_) => {}
    }

    event
}

fn ui_event_client_position(event: &UiEvent) -> Option<(f32, f32)> {
    match event {
        UiEvent::PointerMove(event) | UiEvent::PointerUp(event) | UiEvent::PointerDown(event) => {
            Some((event.coords.client_x, event.coords.client_y))
        }
        UiEvent::Wheel(event) => Some((event.coords.client_x, event.coords.client_y)),
        UiEvent::KeyUp(_)
        | UiEvent::KeyDown(_)
        | UiEvent::Ime(_)
        | UiEvent::AppleStandardKeybinding(_) => None,
    }
}
