mod compositor;
pub mod ui_event;

use anyrender::Scene as RenderScene;
use blitz_traits::events::UiEvent;
use compositor::{Compositor, VisibleFrameViewport};
use ipc_messages::content::{
    FontTransportReceiver, FrameId, NavigateRequest, PaintFrame,
    UserNavigationInvolvement, WebviewId,
};
use std::env;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
pub struct WebviewState {
    pub compositor: Compositor,
    pub current_navigable_id: Option<u64>,
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
    content_navigable_id: FrameId,
}

pub trait EmbedderApi {
    fn request_redraw(&self, webview_id: WebviewId);
    fn viewport_scale_factor(&self) -> f32;
    fn update_traversable_viewport(
        &self,
        traversable_id: WebviewId,
        width: u32,
        height: u32,
        offset_x: f32,
        offset_y: f32,
    );
}

pub trait RuntimeClient {
    fn start_top_level_traversable(&self, destination_url: String) -> Result<(), String>;
    fn start_navigation(&self, request: NavigateRequest) -> Result<(), String>;
    fn dispatch_event_for(&self, traversable_id: u64, event: String) -> Result<(), String>;
    fn rendering_opportunity_for(&self, traversable_id: u64) -> Result<(), String>;
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
    font_receiver: FontTransportReceiver,
    embedder: Box<dyn EmbedderApi>,
    runtime_client: Box<dyn RuntimeClient>,
}

impl WebviewProvider {
    pub fn new(embedder: Box<dyn EmbedderApi>, runtime_client: Box<dyn RuntimeClient>) -> Self {
        Self {
            webviews: HashMap::new(),
            child_navigable_hosts_by_webview: HashMap::new(),
            child_host_webviews_by_content_navigable: HashMap::new(),
            font_receiver: FontTransportReceiver::default(),
            embedder,
            runtime_client,
        }
    }

    pub fn start(&self, startup_url: Option<&str>) -> Result<(), String> {
        let destination_url = startup_destination_url(startup_url)?;
        self.runtime_client
            .start_top_level_traversable(destination_url)
    }

    pub fn navigate(&self, webview_id: Option<WebviewId>, url: &str) -> Result<(), String> {
        match webview_id {
            Some(webview_id) => self.runtime_client.start_navigation(NavigateRequest {
                source_navigable_id: webview_id.0,
                destination_url: url.to_owned(),
                target: String::new(),
                user_involvement: UserNavigationInvolvement::BrowserUi,
                noopener: false,
            }),
            None => self
                .runtime_client
                .start_top_level_traversable(url.to_owned()),
        }
    }

    pub fn send_ui_event(&mut self, webview_id: WebviewId, event: UiEvent) -> Result<(), String> {
        self.embedder.request_redraw(webview_id);
        let (target_webview_id, routed_event, composed_frame_ids) =
            self.route_ui_event(webview_id, event);
        let event_message = ui_event::serialize_ui_event(&routed_event)?;
        self.runtime_client
            .dispatch_event_for(target_webview_id.0, event_message)?;
        self.note_rendering_opportunities(webview_id, composed_frame_ids, "ui_event");
        Ok(())
    }

    pub fn note_rendering_opportunity(&self, webview_id: WebviewId, reason: &str) {
        let _ = reason;
        let _ = self.runtime_client.rendering_opportunity_for(webview_id.0);
    }

    pub fn register_child_navigable_host(
        &mut self,
        child_host_webview_id: WebviewId,
        parent_traversable_id: WebviewId,
        content_navigable_id: FrameId,
    ) {
        self.child_navigable_hosts_by_webview.insert(
            child_host_webview_id,
            ChildNavigableHost {
                parent_traversable_id,
                content_navigable_id,
            },
        );
        self.child_host_webviews_by_content_navigable
            .insert(content_navigable_id, child_host_webview_id);
    }

    pub fn on_paint_frame(&mut self, mut frame: PaintFrame) -> Result<(), String> {
        if let Some(child_navigable_host) =
            self.child_navigable_hosts_by_webview.get(&frame.traversable_id)
        {
            frame.traversable_id = child_navigable_host.parent_traversable_id;
            frame.frame_id = child_navigable_host.content_navigable_id;
        }
        let traversable_id = frame.traversable_id;
        let frame_id = frame.frame_id;
        let viewport_width = frame.viewport_width;
        let viewport_height = frame.viewport_height;
        let recorded_scene = frame.into_recorded_scene(&mut self.font_receiver)?;

        let state = self.webviews.entry(traversable_id).or_default();
        state
            .compositor
            .store_frame(frame_id, viewport_width, viewport_height, recorded_scene);
        if state.compositor.committed_root_frame_id() == Some(frame_id) {
            state.current_navigable_id = Some(frame_id.0);
        }

        self.embedder.request_redraw(traversable_id);
        Ok(())
    }

    pub fn on_finalize_navigation(&mut self, webview_id: WebviewId, _url: &str) {
        if let Some(child_navigable_host) = self.child_navigable_hosts_by_webview.get(&webview_id)
        {
            let state = self
                .webviews
                .entry(child_navigable_host.parent_traversable_id)
                .or_default();
            state
                .compositor
                .note_child_navigation_finalized(child_navigable_host.content_navigable_id);
            return;
        }

        let state = self.webviews.entry(webview_id).or_default();
        state.compositor.note_navigation_finalized();
        state.current_navigable_id = None;
        state.focused_frame_id = None;
    }

    pub fn on_new_top_level_traversable(&mut self, webview_id: WebviewId) {
        self.webviews.entry(webview_id).or_default();
    }

    pub fn current_scene(&mut self, webview_id: WebviewId) -> Option<RenderScene> {
        let (scene, viewports) = {
            let state = self.webviews.get_mut(&webview_id)?;
            let scene = state.compositor.compose_scene(&self.font_receiver);
            let viewports = state.compositor.visible_frame_viewports(&self.font_receiver);
            (scene, viewports)
        };
        self.publish_visible_child_viewports(viewports);
        scene
    }

    pub fn current_navigable_id(&self, webview_id: WebviewId) -> Option<u64> {
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
            let (target_frame_id, composed_frame_ids, viewports) = {
                let Some(state) = self.webviews.get_mut(&root_webview_id) else {
                    return (root_webview_id, event, Vec::new());
                };
                let root_frame_id = state.compositor.committed_root_frame_id();
                let viewports = state.compositor.visible_frame_viewports(&self.font_receiver);
                let composed_frame_ids = composition_frame_ids(root_frame_id, &viewports);
                let target_frame_id = state
                    .focused_frame_id
                    .filter(|frame_id| composed_frame_ids.contains(frame_id))
                    .or(root_frame_id);
                state.focused_frame_id = target_frame_id;
                (target_frame_id, composed_frame_ids, viewports)
            };
            self.publish_visible_child_viewports(viewports);
            let target_webview_id = target_frame_id
                .map(|frame_id| self.webview_id_for_frame(root_webview_id, frame_id))
                .unwrap_or(root_webview_id);
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] root={} frame={} child={} target={} nonpositional=true",
                    root_webview_id.0,
                    target_frame_id.map(|frame_id| frame_id.0).unwrap_or(root_webview_id.0),
                    target_frame_id.is_some_and(|frame_id| frame_id != FrameId(root_webview_id.0)),
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
            let viewports = state.compositor.visible_frame_viewports(&self.font_receiver);
            (root_frame_id, hit, viewports)
        };
        let composed_frame_ids = composition_frame_ids(root_frame_id, &viewports);

        let Some(hit) = hit else {
            if is_pointer_down
                && let Some(state) = self.webviews.get_mut(&root_webview_id)
            {
                state.focused_frame_id = root_frame_id;
            }
            self.publish_visible_child_viewports(viewports);
            if input_debug_enabled() {
                eprintln!(
                    "[input-debug][webview] root={} client=({client_x:.1},{client_y:.1}) hit=none target={}",
                    root_webview_id.0,
                    root_webview_id.0,
                );
            }
            return (root_webview_id, event, composed_frame_ids);
        };

        let target_webview_id = self.webview_id_for_frame(root_webview_id, hit.frame_id);
        let routed_event = retarget_ui_event_for_hit(event, hit, &viewports, viewport_scale);
        if is_pointer_down
            && let Some(state) = self.webviews.get_mut(&root_webview_id)
        {
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

    fn publish_visible_child_viewports(&self, viewports: Vec<VisibleFrameViewport>) {
        let viewport_scale = self.embedder.viewport_scale_factor().max(1.0);
        for viewport in viewports {
            let Some(child_webview_id) = self
                .child_host_webviews_by_content_navigable
                .get(&viewport.frame_id)
                .copied()
            else {
                continue;
            };

            self.embedder.update_traversable_viewport(
                child_webview_id,
                viewport.width,
                viewport.height,
                viewport.offset_x / viewport_scale,
                viewport.offset_y / viewport_scale,
            );
            self.note_rendering_opportunity(child_webview_id, "visible_child_viewport");
        }
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

    fn webview_id_for_frame(&self, root_webview_id: WebviewId, frame_id: FrameId) -> WebviewId {
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
    let Some(viewport) = viewports.iter().find(|viewport| viewport.frame_id == hit.frame_id) else {
        return event;
    };

    let routed_client_x = (viewport.offset_x + hit.local_x) / viewport_scale;
    let routed_client_y = (viewport.offset_y + hit.local_y) / viewport_scale;
    match &mut event {
        UiEvent::PointerMove(event)
        | UiEvent::PointerUp(event)
        | UiEvent::PointerDown(event) => {
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
        UiEvent::PointerMove(event)
        | UiEvent::PointerUp(event)
        | UiEvent::PointerDown(event) => Some((event.coords.client_x, event.coords.client_y)),
        UiEvent::Wheel(event) => Some((event.coords.client_x, event.coords.client_y)),
        UiEvent::KeyUp(_)
        | UiEvent::KeyDown(_)
        | UiEvent::Ime(_)
        | UiEvent::AppleStandardKeybinding(_) => None,
    }
}