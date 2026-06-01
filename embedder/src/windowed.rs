mod chrome;

use self::chrome::{ChromeAction, ChromeTabInfo, ChromeUi, ChromeViewState, WinitShellProvider};
use super::winit_integration::{
    event_loop_options, touch_pointer_details, viewport_of_snapshot, viewport_snapshot_for_window,
    winit_ime_to_blitz, winit_key_event_to_blitz, winit_modifiers_to_kbt_modifiers,
};
use super::{
    FormalWebUserEvent, NavigationCompletion, automation_screenshot_png,
    automation_visible_frame_viewports, normalize_browser_destination, read_clipboard_text,
    startup_destination_url, write_clipboard_text,
};
use anyrender::{PaintScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use automation::{
    AutomationController, AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport,
};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, ShellProvider};
use ipc_messages::content::WebviewId;
use kurbo::Affine;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use webview::WebviewProvider;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, PhysicalPosition};
use winit::event::{
    ElementState, Modifiers, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId as WinitWindowId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(Uuid);
impl WindowId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Per-tab state
pub(super) struct TabState {
    webview_id: WebviewId,
    pending_url: Option<String>,
    committed_url: Option<String>,
}
impl TabState {
    fn new(webview_id: WebviewId) -> Self {
        Self {
            webview_id,
            pending_url: None,
            committed_url: None,
        }
    }
    fn display_url(&self) -> String {
        self.pending_url
            .clone()
            .or_else(|| self.committed_url.clone())
            .unwrap_or_default()
    }
}

/// Per-window state: owns a winit window, a renderer, chrome, and tabs
pub(super) struct WindowState {
    pub(super) window_id: WindowId,
    pub(super) window: Option<Arc<Window>>,
    pub(super) renderer: VelloWindowRenderer,
    pub(super) chrome: Option<ChromeUi>,
    pub(super) tabs: HashMap<WebviewId, TabState>,
    pub(super) tab_order: Vec<WebviewId>,
    pub(super) active_tab: Option<WebviewId>,
    pub(super) automation: AutomationController,
    pub(super) window_occluded: bool,
    pub(super) animation_timer: Option<Instant>,
    pub(super) keyboard_modifiers: Modifiers,
    pub(super) buttons: MouseEventButtons,
    pub(super) pointer_pos: PhysicalPosition<f64>,
}
impl WindowState {
    fn new(window_id: WindowId) -> Self {
        Self {
            window_id,
            window: None,
            renderer: VelloWindowRenderer::new(),
            chrome: None,
            tabs: HashMap::new(),
            tab_order: Vec::new(),
            active_tab: None,
            automation: AutomationController::default(),
            window_occluded: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

pub(super) struct WindowedApp {
    pub(super) windows: HashMap<WindowId, WindowState>,
    pub(super) provider: Option<WebviewProvider>,
    pub(super) active_window_id: Option<WindowId>,
}
impl Default for WindowedApp {
    fn default() -> Self {
        Self {
            windows: HashMap::new(),
            provider: None,
            active_window_id: None,
        }
    }
}

pub(super) static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));
pub(super) fn update_window_viewport_snapshot(s: Option<(u32, u32, f32, ColorScheme)>) {
    *WINDOW_VIEWPORT_SNAPSHOT.lock().expect("poisoned") = s;
}
pub(super) fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    *WINDOW_VIEWPORT_SNAPSHOT.lock().expect("poisoned")
}

// ── Static helpers ─────────────────────────────────────────────────────────
impl WindowedApp {
    fn has_visible_viewport(s: &WindowState) -> bool {
        let Some(w) = s.window.as_ref() else {
            return false;
        };
        if s.window_occluded || matches!(w.is_visible(), Some(false)) {
            return false;
        }
        let size = w.inner_size();
        size.width > 0 && size.height > 0
    }
    fn chrome_height_css(s: &WindowState) -> f32 {
        s.chrome
            .as_ref()
            .map(ChromeUi::height_css)
            .unwrap_or_default()
    }
    fn chrome_height_physical(s: &WindowState) -> u32 {
        s.chrome
            .as_ref()
            .map(ChromeUi::height_physical)
            .unwrap_or_default()
    }
    fn content_has_visible_viewport(s: &WindowState) -> bool {
        Self::has_visible_viewport(s)
            && s.window
                .as_ref()
                .is_some_and(|w| w.inner_size().height > Self::chrome_height_physical(s))
    }
    fn pointer_in_viewport(s: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::has_visible_viewport(s)
            && s.window.as_ref().is_some_and(|w| {
                let sz = w.inner_size();
                pos.x >= 0.0
                    && pos.y >= 0.0
                    && pos.x < f64::from(sz.width)
                    && pos.y < f64::from(sz.height)
            })
    }
    fn pointer_in_chrome(s: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::pointer_in_viewport(s, pos) && pos.y < f64::from(Self::chrome_height_physical(s))
    }
    fn pointer_in_content(s: &WindowState, pos: PhysicalPosition<f64>) -> bool {
        Self::pointer_in_viewport(s, pos)
            && pos.y >= f64::from(Self::chrome_height_physical(s))
            && Self::content_has_visible_viewport(s)
    }
    fn request_window_redraw(s: &WindowState) {
        if let Some(w) = s.window.as_ref() {
            if Self::has_visible_viewport(s) {
                w.request_redraw();
            }
        }
    }
    fn request_visible_redraw(s: &WindowState, prov: Option<&WebviewProvider>, reason: &str) {
        Self::request_window_redraw(s);
        if let Some((p, id)) = prov.zip(s.active_tab) {
            p.note_rendering_opportunity(id, reason);
        }
    }
    fn tab_display_url(s: &WindowState) -> String {
        s.active_tab
            .and_then(|id| s.tabs.get(&id))
            .map(TabState::display_url)
            .unwrap_or_default()
    }
    fn tab_label(s: &WindowState, wid: &WebviewId) -> String {
        if let Some(tab) = s.tabs.get(wid) {
            if let Some(url) = &tab.committed_url {
                if !url.is_empty() {
                    return Self::truncate_url(url);
                }
            }
            if let Some(url) = &tab.pending_url {
                if !url.is_empty() {
                    return Self::truncate_url(url);
                }
            }
        }
        String::from("New Tab")
    }

    fn truncate_url(url: &str) -> String {
        let display = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .or_else(|| url.strip_prefix("file://"))
            .unwrap_or(url);
        if display.len() > 30 {
            format!("{}…", &display[..27])
        } else {
            display.to_owned()
        }
    }

    fn build_chrome_view_state(s: &WindowState) -> ChromeViewState {
        let tabs: Vec<ChromeTabInfo> = s
            .tab_order
            .iter()
            .map(|wid| ChromeTabInfo {
                label: Self::tab_label(s, wid),
                active: s.active_tab == Some(*wid),
            })
            .collect();
        ChromeViewState {
            address: Self::tab_display_url(s),
            tabs,
        }
    }
    fn sync_chrome(s: &mut WindowState) {
        let v = Self::build_chrome_view_state(s);
        if let Some(c) = s.chrome.as_mut() {
            c.sync_state(&v);
        }
    }
    fn update_provider_viewport(s: &WindowState, provider: &mut Option<WebviewProvider>) {
        let Some(window) = s.window.as_ref() else {
            return;
        };
        let (w, h, scale, cs) = viewport_snapshot_for_window(window);
        let viewport = (
            w,
            h.saturating_sub(Self::chrome_height_physical(s)),
            scale,
            cs,
        );
        update_window_viewport_snapshot(Some(viewport));
        if let Some(p) = provider.as_mut() {
            let _ = p.set_default_viewport(Some(viewport));
            if let Some(wid) = s.active_tab {
                let _ = p.set_traversable_viewport(wid, viewport, 0.0, 0.0);
            }
        }
    }
    fn logical_pos(s: &WindowState, pos: PhysicalPosition<f64>) -> LogicalPosition<f32> {
        let scale = s.window.as_ref().map(|w| w.scale_factor()).unwrap_or(1.0);
        pos.to_logical(scale)
    }
    fn c_coords(s: &WindowState, pos: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x, y } = Self::logical_pos(s, pos);
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            page_x: x,
            page_y: y,
        }
    }
    fn ct_coords(s: &WindowState, pos: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x, y } = Self::logical_pos(s, pos);
        let ch = Self::chrome_height_css(s);
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y - ch,
            page_x: x,
            page_y: y - ch,
        }
    }
    fn add_tab(st: &mut WindowState, wid: WebviewId) {
        if st.tabs.contains_key(&wid) {
            st.active_tab = Some(wid);
            return;
        }
        st.tabs.insert(wid, TabState::new(wid));
        st.tab_order.push(wid);
        st.active_tab = Some(wid);
    }
    fn paint_frame(st: &mut WindowState, prov: &mut Option<WebviewProvider>) {
        if !Self::has_visible_viewport(st) {
            return;
        }
        st.animation_timer.get_or_insert_with(Instant::now);
        let Some(window) = st.window.as_ref() else {
            return;
        };
        let ch = f64::from(Self::chrome_height_physical(st));
        let cs = st.chrome.as_mut().map(ChromeUi::paint_scene);
        if cs.is_none() && st.active_tab.is_none() {
            return;
        }
        let size = window.inner_size();
        if st.renderer.is_active() {
            st.renderer.set_size(size.width, size.height);
        } else {
            let wh: Arc<dyn anyrender::WindowHandle> = window.clone();
            st.renderer.resume(wh, size.width, size.height, || {});
            st.renderer.complete_resume();
        }
        let at = st.active_tab;
        st.renderer.render(|scene| {
            if let Some(wid) = at {
                if let Some(p) = prov.as_mut() {
                    let _ = p.append_web_content_scene(wid, scene, Affine::translate((0.0, ch)));
                }
            }
            if let Some(cs) = cs.clone() {
                scene.append_scene(cs, Affine::IDENTITY);
            }
        });
    }
    fn window_for_webview(app: &Self, wid: WebviewId) -> Option<WindowId> {
        app.windows.iter().find_map(|(id, s)| {
            if s.tabs.contains_key(&wid) {
                Some(*id)
            } else {
                None
            }
        })
    }

    fn auto_snapshot(s: &WindowState) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: s.active_tab,
            current_url: s
                .active_tab
                .and_then(|id| s.tabs.get(&id))
                .and_then(|t| t.committed_url.clone()),
            displayed_url: Self::tab_display_url(s),
            navigable_id: None,
            has_top_level_traversable: s.active_tab.is_some(),
        }
    }
    fn create_winit_window(el: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let title = event_loop_options()
            .window_title
            .unwrap_or_else(|| String::from("formal-web"));
        el.create_window(Window::default_attributes().with_title(title))
            .map(Arc::new)
            .map_err(|e| format!("failed to create winit window: {e}"))
    }
    fn resume_renderer(st: &mut WindowState, w: &Arc<Window>) {
        let sz = w.inner_size();
        if sz.width == 0 || sz.height == 0 {
            return;
        }
        if st.renderer.is_active() {
            st.renderer.set_size(sz.width, sz.height);
        } else {
            let wh: Arc<dyn anyrender::WindowHandle> = w.clone();
            st.renderer.resume(wh, sz.width, sz.height, || {});
            st.renderer.complete_resume();
        }
    }
    fn with_provider<R>(&mut self, f: impl FnOnce(&mut WebviewProvider) -> R) -> Option<R> {
        self.provider.as_mut().map(f)
    }
    fn dispatch_to_content(&mut self, window_id: WindowId, event: UiEvent) {
        let wid = self.windows.get(&window_id).and_then(|s| s.active_tab);
        let Some(wid) = wid else {
            return;
        };
        self.with_provider(|p| {
            if let Err(e) = p.send_ui_event(wid, event) {
                eprintln!("content event error: {e}");
            }
        });
        if let Some(s) = self.windows.get(&window_id) {
            Self::request_window_redraw(s);
        }
    }
    fn try_run_automation<R>(
        &mut self,
        f: impl FnOnce(&mut AutomationController, &mut WindowedApp) -> R,
    ) -> Option<R> {
        let wid = self.active_window_id?;
        let state = self.windows.get_mut(&wid)?;
        let mut automation = std::mem::take(&mut state.automation);
        let result = f(&mut automation, self);
        if let Some(s) = self.windows.get_mut(&wid) {
            s.automation = automation;
        }
        Some(result)
    }
}

// ── ApplicationHandler ─────────────────────────────────────────────────────
impl ApplicationHandler<FormalWebUserEvent> for WindowedApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        let wid = WindowId::new();
        let window = match Self::create_winit_window(el) {
            Ok(w) => w,
            Err(_) => {
                el.exit();
                return;
            }
        };
        let fvp = viewport_of_snapshot(viewport_snapshot_for_window(&window));
        let cp: Arc<dyn ShellProvider> = Arc::new(WinitShellProvider::new(window.clone()));
        let chrome = match ChromeUi::new(fvp, cp) {
            Ok(c) => c,
            Err(_) => {
                el.exit();
                return;
            }
        };
        let mut st = WindowState::new(wid);
        st.chrome = Some(chrome);
        st.window = Some(window.clone());
        self.active_window_id = Some(wid);
        Self::sync_chrome(&mut st);
        // Set default viewport so new traversables know initial dimensions.
        // Don't call set_traversable_viewport — no tab exists yet.
        if let Some(w) = st.window.as_ref() {
            let (w_, h_, sc, cs) = viewport_snapshot_for_window(w);
            let vp = (
                w_,
                h_.saturating_sub(Self::chrome_height_physical(&st)),
                sc,
                cs,
            );
            update_window_viewport_snapshot(Some(vp));
            if let Some(p) = self.provider.as_mut() {
                let _ = p.set_default_viewport(Some(vp));
            }
        }
        Self::resume_renderer(&mut st, &window);
        // Determine destination URL: provided startup URL, artifact, or fallback.
        let destination = startup_destination_url(event_loop_options().startup_url.as_deref())
            .unwrap_or_else(|_| String::from("about:blank"));
        if let Some(p) = self.provider.as_ref() {
            let _ = p.navigate(None, &destination);
        }
        self.windows.insert(wid, st);
        if let Some(s) = self.windows.get(&wid) {
            Self::request_window_redraw(s);
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, ww_id: WinitWindowId, event: WindowEvent) {
        let window_id = match self
            .windows
            .iter()
            .find(|(_, s)| s.window.as_ref().map(|w| w.id()) == Some(ww_id))
            .map(|(id, _)| *id)
        {
            Some(id) => id,
            None => return,
        };
        self.active_window_id = Some(window_id);

        match event {
            WindowEvent::RedrawRequested => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    if self.provider.is_some() || st.chrome.is_some() {
                        Self::paint_frame(st, &mut self.provider);
                    }
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    st.window_occluded = occluded;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    if let Some(w) = st.window.as_ref() {
                        let fvp = viewport_of_snapshot(viewport_snapshot_for_window(w));
                        if let Some(c) = st.chrome.as_mut() {
                            c.set_viewport(fvp);
                        }
                        Self::sync_chrome(st);
                    }
                    if st.renderer.is_active() {
                        st.renderer.set_size(size.width, size.height);
                    }
                }
                // Update provider viewport (separate borrow from state)
                if let Some(st) = self.windows.get(&window_id) {
                    Self::update_provider_viewport(st, &mut self.provider);
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    if let Some(w) = st.window.as_ref() {
                        w.set_visible(false);
                    }
                    st.automation
                        .abort_pending_navigation(String::from("window closed"));
                    st.renderer.suspend();
                    st.animation_timer = None;
                    st.chrome = None;
                    st.tabs.clear();
                    st.tab_order.clear();
                    st.active_tab = None;
                    st.window_occluded = false;
                    st.window = None;
                }
                // Update active window if the closed one was active
                if self.active_window_id == Some(window_id) {
                    self.active_window_id = self.windows.keys().next().copied();
                }
                self.windows.remove(&window_id);
                if self.windows.is_empty() {
                    let _ = super::send_user_event(FormalWebUserEvent::Exit);
                }
            }
            WindowEvent::Ime(event) => {
                let ui_event = UiEvent::Ime(winit_ime_to_blitz(event));
                if Self::is_chrome_focused(&self.windows, window_id) {
                    Self::chrome_event(self, window_id, ui_event);
                } else {
                    self.dispatch_to_content(window_id, ui_event);
                }
            }
            WindowEvent::ModifiersChanged(ms) => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    st.keyboard_modifiers = ms;
                }
            }
            WindowEvent::KeyboardInput { event: ke, .. } => {
                let mods = self
                    .windows
                    .get(&window_id)
                    .map(|s| s.keyboard_modifiers.state())
                    .unwrap_or_default();
                let key = winit_key_event_to_blitz(&ke, mods);
                let ui_event = if ke.state.is_pressed() {
                    UiEvent::KeyDown(key)
                } else {
                    UiEvent::KeyUp(key)
                };
                if Self::is_chrome_focused(&self.windows, window_id) {
                    Self::chrome_event(self, window_id, ui_event);
                } else {
                    self.dispatch_to_content(window_id, ui_event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let ch_phys = self
                    .windows
                    .get(&window_id)
                    .map(|s| Self::chrome_height_physical(s))
                    .unwrap_or(0);
                if Self::pointer_in_chrome_st(&self.windows, window_id, position) {
                    if let Some(st) = self.windows.get_mut(&window_id) {
                        st.pointer_pos = position;
                        let ev = UiEvent::PointerMove(BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::c_coords(st, position),
                            button: Default::default(),
                            buttons: st.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state()),
                            details: PointerDetails::default(),
                        });
                        drop(st);
                        Self::chrome_event(self, window_id, ev);
                    }
                } else if Self::pointer_in_content_st(&self.windows, window_id, position) {
                    if let Some(st) = self.windows.get_mut(&window_id) {
                        st.pointer_pos = position;
                        let coords = Self::ct_coords(st, position);
                        let buttons = st.buttons;
                        let mods = winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state());
                        drop(st);
                        self.dispatch_to_content(
                            window_id,
                            UiEvent::PointerMove(BlitzPointerEvent {
                                id: BlitzPointerId::Mouse,
                                is_primary: true,
                                coords,
                                button: Default::default(),
                                buttons,
                                mods,
                                details: PointerDetails::default(),
                            }),
                        );
                    }
                }
            }
            WindowEvent::MouseInput {
                button, state: bs, ..
            } => {
                if let Some(st) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(st, st.pointer_pos) {
                        return;
                    }
                    let mb = Self::map_button(button);
                    match bs {
                        ElementState::Pressed => st.buttons |= mb.into(),
                        ElementState::Released => st.buttons.remove(mb.into()),
                    }
                    if Self::pointer_in_chrome(st, st.pointer_pos) {
                        let ev = BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::c_coords(st, st.pointer_pos),
                            button: mb,
                            buttons: st.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state()),
                            details: PointerDetails::default(),
                        };
                        drop(st);
                        Self::chrome_event(
                            self,
                            window_id,
                            match bs {
                                ElementState::Pressed => UiEvent::PointerDown(ev),
                                ElementState::Released => UiEvent::PointerUp(ev),
                            },
                        );
                    } else if Self::pointer_in_content(st, st.pointer_pos) {
                        if bs.is_pressed() {
                            if let Some(c) = st.chrome.as_mut() {
                                c.clear_focus();
                            }
                            Self::request_window_redraw(st);
                        }
                        let ev = BlitzPointerEvent {
                            id: BlitzPointerId::Mouse,
                            is_primary: true,
                            coords: Self::ct_coords(st, st.pointer_pos),
                            button: mb,
                            buttons: st.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state()),
                            details: PointerDetails::default(),
                        };
                        let webview_id = st.active_tab;
                        drop(st);
                        if let Some(wid) = webview_id {
                            self.with_provider(|p| {
                                let r = match bs {
                                    ElementState::Pressed => {
                                        p.send_ui_event(wid, UiEvent::PointerDown(ev))
                                    }
                                    ElementState::Released => {
                                        p.send_ui_event(wid, UiEvent::PointerUp(ev))
                                    }
                                };
                                if let Err(e) = r {
                                    eprintln!("content event error: {e}");
                                }
                            });
                        }
                        if let Some(st) = self.windows.get(&window_id) {
                            Self::request_window_redraw(st);
                        }
                    }
                }
            }
            WindowEvent::Touch(Touch {
                phase,
                location,
                force,
                id,
                ..
            }) => {
                let in_chrome = Self::pointer_in_chrome_st(&self.windows, window_id, location);
                if let Some(st) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(st, location) {
                        return;
                    }
                    let ev = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: if in_chrome {
                            Self::c_coords(st, location)
                        } else {
                            Self::ct_coords(st, location)
                        },
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    if in_chrome {
                        drop(st);
                        Self::chrome_event(
                            self,
                            window_id,
                            match phase {
                                TouchPhase::Started => UiEvent::PointerDown(ev),
                                TouchPhase::Moved => UiEvent::PointerMove(ev),
                                TouchPhase::Ended | TouchPhase::Cancelled => UiEvent::PointerUp(ev),
                            },
                        );
                    } else if Self::pointer_in_content(st, location) {
                        if phase == TouchPhase::Started {
                            if let Some(c) = st.chrome.as_mut() {
                                c.clear_focus();
                            }
                            Self::request_window_redraw(st);
                        }
                        let wid = st.active_tab;
                        drop(st);
                        if let Some(wid) = wid {
                            self.with_provider(|p| {
                                let r = match phase {
                                    TouchPhase::Started => {
                                        p.send_ui_event(wid, UiEvent::PointerDown(ev))
                                    }
                                    TouchPhase::Moved => {
                                        p.send_ui_event(wid, UiEvent::PointerMove(ev))
                                    }
                                    TouchPhase::Ended | TouchPhase::Cancelled => {
                                        p.send_ui_event(wid, UiEvent::PointerUp(ev))
                                    }
                                };
                                if let Err(e) = r {
                                    eprintln!("touch event error: {e}");
                                }
                            });
                        }
                        if let Some(st) = self.windows.get(&window_id) {
                            Self::request_window_redraw(st);
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let wd = match delta {
                    MouseScrollDelta::LineDelta(x, y) => BlitzWheelDelta::Lines(x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(p) => BlitzWheelDelta::Pixels(p.x, p.y),
                };
                if Self::pointer_in_chrome_st(
                    &self.windows,
                    window_id,
                    self.windows
                        .get(&window_id)
                        .map_or(PhysicalPosition::default(), |s| s.pointer_pos),
                ) {
                    if let Some(st) = self.windows.get_mut(&window_id) {
                        if !Self::pointer_in_viewport(st, st.pointer_pos) {
                            return;
                        }
                        let ev = UiEvent::Wheel(BlitzWheelEvent {
                            delta: wd,
                            coords: Self::c_coords(st, st.pointer_pos),
                            buttons: st.buttons,
                            mods: winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state()),
                        });
                        drop(st);
                        Self::chrome_event(self, window_id, ev);
                    }
                } else if let Some(st) = self.windows.get_mut(&window_id) {
                    if !Self::pointer_in_viewport(st, st.pointer_pos) {
                        return;
                    }
                    let wid = st.active_tab;
                    let coords = Self::ct_coords(st, st.pointer_pos);
                    let buttons = st.buttons;
                    let mods = winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state());
                    drop(st);
                    if let Some(wid) = wid {
                        self.with_provider(|p| {
                            if let Err(e) = p.send_ui_event(
                                wid,
                                UiEvent::Wheel(BlitzWheelEvent {
                                    delta: wd,
                                    coords,
                                    buttons,
                                    mods,
                                }),
                            ) {
                                eprintln!("wheel event error: {e}");
                            }
                        });
                    }
                    if let Some(st) = self.windows.get(&window_id) {
                        Self::request_window_redraw(st);
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {}

    fn user_event(&mut self, el: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::WebviewProviderSync => {
                if let Some(p) = self.provider.as_mut() {
                    if let Err(e) = p.sync_pending_messages() {
                        eprintln!("provider sync error: {e}");
                    }
                }
            }
            FormalWebUserEvent::NewFrameRendered => {
                self.try_run_automation(|a, app| a.note_rendering_update(app));
                // Don't request redraw here — the per-webview RequestRedraw event
                // (dispatched by Embedder::request_redraw) handles targeted redraws.
                // Requesting redraw for all windows here causes a high-FPS cycle
                // since every RedrawRequested can trigger another NewFrameRendered.
            }
            FormalWebUserEvent::RequestRedraw(wid) => {
                if let Some(w) = Self::window_for_webview(self, wid) {
                    if let Some(s) = self.windows.get(&w) {
                        if s.active_tab == Some(wid) {
                            Self::request_window_redraw(s);
                        }
                    }
                }
            }
            FormalWebUserEvent::NavigationRequested {
                webview_id,
                destination_url,
            } => {
                // Update pending URL for any known webview (active tab or not).
                // If the webview is not in any window, create a new tab for it.
                if let Some(w) = Self::window_for_webview(self, webview_id) {
                    if let Some(st) = self.windows.get_mut(&w) {
                        if let Some(t) = st.tabs.get_mut(&webview_id) {
                            t.pending_url = Some(destination_url.clone());
                        }
                        if st.active_tab == Some(webview_id) {
                            Self::sync_chrome(st);
                            Self::update_provider_viewport(st, &mut self.provider);
                            Self::request_window_redraw(st);
                        }
                    }
                } else if let Some(a) = self.active_window_id {
                    if let Some(st) = self.windows.get_mut(&a) {
                        Self::add_tab(st, webview_id);
                        Self::sync_chrome(st);
                        Self::update_provider_viewport(st, &mut self.provider);
                    }
                }
            }
            FormalWebUserEvent::NavigationCompleted(c) => {
                let w_opt = Self::window_for_webview(self, c.webview_id);
                let Some(w) = w_opt else {
                    // Ignore: child traversables (iframes) fire their own
                    // NavigationCompleted — those don't create tabs.
                    return;
                };
                let is_current = self
                    .windows
                    .get(&w)
                    .map_or(false, |s| s.active_tab == Some(c.webview_id));
                match &c.status {
                    NavigationCompletion::Committed { url } => {
                        if let Some(st) = self.windows.get_mut(&w) {
                            if let Some(t) = st.tabs.get_mut(&c.webview_id) {
                                t.pending_url = None;
                                t.committed_url = Some(url.clone());
                            }
                        }
                        // Clear compositor first so new paint frames populate it.
                        if let Some(p) = self.provider.as_mut() {
                            p.on_navigation_committed(c.webview_id);
                            // Request a rendering opportunity so the content process
                            // sends a new paint frame. on_navigation_committed only does
                            // this for child navigables, not the top-level traversable.
                            p.note_rendering_opportunity(c.webview_id, "navigation_committed");
                        }
                        if is_current {
                            if let Some(st) = self.windows.get_mut(&w) {
                                Self::sync_chrome(st);
                                Self::update_provider_viewport(st, &mut self.provider);
                                Self::request_window_redraw(st);
                            }
                            self.try_run_automation(|a, app| a.note_navigation_committed(app));
                        }
                    }
                    NavigationCompletion::Aborted { message } => {
                        if is_current {
                            if let Some(st) = self.windows.get_mut(&w) {
                                let mut a = std::mem::take(&mut st.automation);
                                a.abort_pending_navigation(message.clone());
                                st.automation = a;
                                if let Some(t) = st.tabs.get_mut(&c.webview_id) {
                                    t.pending_url = None;
                                }
                                Self::sync_chrome(st);
                                Self::request_window_redraw(st);
                            }
                        }
                    }
                }
            }
            FormalWebUserEvent::NewWebview(wid, _) => {
                if let Some(w) = self.active_window_id {
                    if let Some(st) = self.windows.get_mut(&w) {
                        Self::add_tab(st, wid);
                        Self::sync_chrome(st);
                        Self::update_provider_viewport(st, &mut self.provider);
                        Self::request_visible_redraw(st, self.provider.as_ref(), "request_redraw");
                    }
                }
            }
            FormalWebUserEvent::CreateWindow => {
                let wid = WindowId::new();
                let window = match Self::create_winit_window(el) {
                    Ok(w) => w,
                    Err(_) => return,
                };
                let fvp = viewport_of_snapshot(viewport_snapshot_for_window(&window));
                let cp: Arc<dyn ShellProvider> = Arc::new(WinitShellProvider::new(window.clone()));
                let chrome = match ChromeUi::new(fvp, cp) {
                    Ok(c) => c,
                    Err(_) => return,
                };
                let mut st = WindowState::new(wid);
                st.chrome = Some(chrome);
                st.window = Some(window.clone());
                self.active_window_id = Some(wid);
                Self::sync_chrome(&mut st);
                Self::update_provider_viewport(&st, &mut self.provider);
                Self::resume_renderer(&mut st, &window);
                if let Some(p) = self.provider.as_ref() {
                    let _ = p.navigate(None, "about:blank");
                }
                self.windows.insert(wid, st);
            }
            FormalWebUserEvent::Automation(cmd) => {
                self.try_run_automation(|a, app| a.handle_command(app, cmd));
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::Exit => el.exit(),
        }
    }
}

// ── Chrome helpers ─────────────────────────────────────────────────────────
impl WindowedApp {
    fn is_chrome_focused(windows: &HashMap<WindowId, WindowState>, wid: WindowId) -> bool {
        windows
            .get(&wid)
            .and_then(|s| s.chrome.as_ref())
            .map_or(false, ChromeUi::takes_text_input_focus)
    }
    fn pointer_in_chrome_st(
        windows: &HashMap<WindowId, WindowState>,
        wid: WindowId,
        pos: PhysicalPosition<f64>,
    ) -> bool {
        windows
            .get(&wid)
            .map_or(false, |s| Self::pointer_in_chrome(s, pos))
    }
    fn pointer_in_content_st(
        windows: &HashMap<WindowId, WindowState>,
        wid: WindowId,
        pos: PhysicalPosition<f64>,
    ) -> bool {
        windows
            .get(&wid)
            .map_or(false, |s| Self::pointer_in_content(s, pos))
    }
    fn map_button(b: MouseButton) -> MouseEventButton {
        match b {
            MouseButton::Left => MouseEventButton::Main,
            MouseButton::Right => MouseEventButton::Secondary,
            MouseButton::Middle => MouseEventButton::Auxiliary,
            MouseButton::Back => MouseEventButton::Fourth,
            MouseButton::Forward => MouseEventButton::Fifth,
            MouseButton::Other(_) => MouseEventButton::Auxiliary,
        }
    }
    fn chrome_event(app: &mut Self, wid: WindowId, event: UiEvent) {
        let Some(st) = app.windows.get_mut(&wid) else {
            return;
        };
        if !Self::has_visible_viewport(st) {
            return;
        }
        let action = st.chrome.as_mut().and_then(|c| c.handle_ui_event(event));
        if let Some(ref a) = action {
        } else {
        }
        Self::request_window_redraw(st);
        if let Some(action) = action {
            Self::handle_chrome_action(app, wid, action);
        }
    }
    fn handle_chrome_action(app: &mut Self, wid: WindowId, action: ChromeAction) {
        match action {
            ChromeAction::Navigate => {
                let url = app.windows.get_mut(&wid).and_then(|st| {
                    st.chrome
                        .as_ref()
                        .and_then(|c| normalize_browser_destination(&c.address_value()))
                });
                let Some(url) = url else {
                    return;
                };
                if let Some(st) = app.windows.get_mut(&wid) {
                    if let Some(p) = app.provider.as_ref() {
                        if let Some(w) = st.active_tab {
                            let _ = p.navigate(Some(w), &url);
                            if let Some(t) = st.tabs.get_mut(&w) {
                                t.pending_url = Some(url.clone());
                            }
                        }
                    }
                    Self::sync_chrome(st);
                    Self::request_window_redraw(st);
                }
            }
            ChromeAction::NewTab => {
                if let Some(p) = app.provider.as_ref() {
                    let _ = p.navigate(None, "about:blank");
                }
                if let Some(st) = app.windows.get_mut(&wid) {
                    Self::sync_chrome(st);
                    Self::request_window_redraw(st);
                }
            }
            ChromeAction::NewWindow => {
                let _ = super::send_user_event(FormalWebUserEvent::CreateWindow);
            }
            ChromeAction::SwitchTab(index) => {
                if let Some(st) = app.windows.get_mut(&wid) {
                    if let Some(&id) = st.tab_order.get(index) {
                        st.active_tab = Some(id);
                        Self::update_provider_viewport(st, &mut app.provider);
                    }
                    Self::sync_chrome(st);
                    Self::request_window_redraw(st);
                }
            }
        }
    }
}

// ── AutomationHost ─────────────────────────────────────────────────────────
impl AutomationHost for WindowedApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        self.active_window_id
            .and_then(|id| self.windows.get(&id))
            .map(Self::auto_snapshot)
            .unwrap_or(AutomationSnapshot {
                webview_id: None,
                current_url: None,
                displayed_url: String::new(),
                navigable_id: None,
                has_top_level_traversable: false,
            })
    }
    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        let wid = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|s| s.active_tab);
        automation_visible_frame_viewports(&mut self.provider, wid)
    }
    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String> {
        let wid = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|s| s.active_tab);
        automation_screenshot_png(&mut self.provider, wid)
    }
    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        let (_wid, st) = match self
            .active_window_id
            .and_then(|id| self.windows.get_mut(&id).map(|s| (id, s)))
        {
            Some((wid, st)) => (wid, st),
            None => return Err(String::from("no window")),
        };
        let p = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?;
        Self::navigate_active_tab(p, st, &url)
    }
    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let wid = self
            .active_window_id
            .ok_or_else(|| String::from("no window"))?;
        let st = self
            .windows
            .get_mut(&wid)
            .ok_or_else(|| String::from("no state"))?;
        Self::auto_click(st, &mut self.provider, x, y)
    }
    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        let wid = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|s| s.active_tab)
            .ok_or_else(|| String::from("no tab"))?;
        self.with_provider(|p| {
            p.click_element(wid, selector).ok();
            p.note_rendering_opportunity(wid, "automation_element_click");
        })
        .ok_or_else(|| String::from("no provider"))
    }
    fn automation_scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), String> {
        let wid = self
            .active_window_id
            .ok_or_else(|| String::from("no window"))?;
        let st = self
            .windows
            .get_mut(&wid)
            .ok_or_else(|| String::from("no state"))?;
        Self::auto_scroll(st, &mut self.provider, x, y, dx, dy)
    }
    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        let wid = self
            .active_window_id
            .and_then(|id| self.windows.get(&id))
            .and_then(|s| s.active_tab)
            .ok_or_else(|| String::from("no tab"))?;
        self.provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?
            .evaluate_script(wid, source, timeout)
    }
}

// ── Automation and navigation helpers ─────────────────────────────────────
impl WindowedApp {
    fn navigate_active_tab(
        p: &WebviewProvider,
        st: &mut WindowState,
        url: &str,
    ) -> Result<(), String> {
        let wid = st.active_tab.ok_or_else(|| String::from("no active tab"))?;
        p.navigate(Some(wid), url)?;
        if let Some(t) = st.tabs.get_mut(&wid) {
            t.pending_url = Some(url.to_owned());
        }
        Ok(())
    }
    fn auto_click(
        st: &mut WindowState,
        prov: &mut Option<WebviewProvider>,
        x: f32,
        y: f32,
    ) -> Result<(), String> {
        let Some(w) = st.window.as_ref() else {
            return Err(String::from("no window"));
        };
        let scale = w.scale_factor();
        let ch = f64::from(Self::chrome_height_css(st));
        let pos = PhysicalPosition::new(f64::from(x) * scale, (f64::from(y) + ch) * scale);
        st.pointer_pos = pos;
        if let Some(c) = st.chrome.as_mut() {
            c.clear_focus();
        }
        Self::request_window_redraw(st);
        let mods = winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state());
        let coords = Self::ct_coords(st, pos);
        let do_send = |p: &mut WebviewProvider, w: WebviewId, e| {
            p.send_ui_event(w, e).ok();
        };
        let mk = |b: MouseEventButton, bt: MouseEventButtons| BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: coords.clone(),
            button: b,
            buttons: bt,
            mods,
            details: PointerDetails::default(),
        };
        let Some(wid) = st.active_tab else {
            return Ok(());
        };
        if let Some(p) = prov.as_mut() {
            do_send(
                p,
                wid,
                UiEvent::PointerMove(mk(Default::default(), st.buttons)),
            );
        }
        st.buttons |= MouseEventButton::Main.into();
        if let Some(p) = prov.as_mut() {
            do_send(
                p,
                wid,
                UiEvent::PointerDown(mk(MouseEventButton::Main, st.buttons)),
            );
        }
        st.buttons.remove(MouseEventButton::Main.into());
        if let Some(p) = prov.as_mut() {
            do_send(
                p,
                wid,
                UiEvent::PointerUp(mk(MouseEventButton::Main, st.buttons)),
            );
        }
        Ok(())
    }
    fn auto_scroll(
        st: &mut WindowState,
        prov: &mut Option<WebviewProvider>,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Result<(), String> {
        let Some(w) = st.window.as_ref() else {
            return Err(String::from("no window"));
        };
        let scale = w.scale_factor();
        let ch = f64::from(Self::chrome_height_css(st));
        let pos = PhysicalPosition::new(f64::from(x) * scale, (f64::from(y) + ch) * scale);
        st.pointer_pos = pos;
        let mods = winit_modifiers_to_kbt_modifiers(st.keyboard_modifiers.state());
        let coords = Self::ct_coords(st, pos);
        let mk_move = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: coords.clone(),
            button: Default::default(),
            buttons: st.buttons,
            mods,
            details: PointerDetails::default(),
        };
        let Some(wid) = st.active_tab else {
            return Ok(());
        };
        if let Some(p) = prov.as_mut() {
            let _ = p.send_ui_event(wid, UiEvent::PointerMove(mk_move));
        }
        if let Some(p) = prov.as_mut() {
            let _ = p.send_ui_event(
                wid,
                UiEvent::Wheel(BlitzWheelEvent {
                    delta: BlitzWheelDelta::Pixels(f64::from(dx), f64::from(dy)),
                    coords,
                    buttons: st.buttons,
                    mods,
                }),
            );
        }
        Ok(())
    }
}
