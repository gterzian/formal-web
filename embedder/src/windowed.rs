#[path = "windowed/chrome.rs"]
mod chrome;

use super::{
    BrowserState, FormalWebUserEvent, NavigationCompleted, NavigationCompletion,
    PendingNavigation, automation_screenshot_png, automation_visible_frame_viewports,
    normalize_browser_destination, parse_child_navigable_host_target, read_clipboard_text,
    startup_destination_url, write_clipboard_text,
};
use self::chrome::{ChromeAction, ChromeUi, ChromeViewState};
use super::winit_integration::{
    WinitShellProvider, event_loop_options, touch_pointer_details, viewport_of_snapshot,
    viewport_snapshot_for_window, winit_ime_to_blitz, winit_key_event_to_blitz,
    winit_modifiers_to_kbt_modifiers,
};
use automation::{AutomationController, AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport};
use anyrender::{PaintScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, ShellProvider};
use kurbo::Affine;
use ipc_messages::content::WebviewId;
use serde_json::Value;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use webview::WebviewProvider;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, PhysicalPosition};
use winit::event::{
    ElementState, Modifiers, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

pub(super) struct HeadedEmbedderApp {
    pub(super) window: Option<Arc<Window>>,
    pub(super) renderer: VelloWindowRenderer,
    pub(super) chrome: Option<ChromeUi>,
    pub(super) browser: BrowserState,
    pub(super) automation: AutomationController,
    pub(super) provider: Option<WebviewProvider>,
    pub(super) current_webview_id: Option<WebviewId>,
    pub(super) has_top_level_traversable: bool,
    pub(super) window_occluded: bool,
    pub(super) animation_timer: Option<Instant>,
    pub(super) keyboard_modifiers: Modifiers,
    pub(super) buttons: MouseEventButtons,
    pub(super) pointer_pos: PhysicalPosition<f64>,
}

pub(super) static WINDOW_VIEWPORT_SNAPSHOT: LazyLock<Mutex<Option<(u32, u32, f32, ColorScheme)>>> =
    LazyLock::new(|| Mutex::new(None));

pub(super) fn update_window_viewport_snapshot(snapshot: Option<(u32, u32, f32, ColorScheme)>) {
    let mut guard = WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned");
    *guard = snapshot;
}

pub(super) fn window_viewport_snapshot() -> Option<(u32, u32, f32, ColorScheme)> {
    WINDOW_VIEWPORT_SNAPSHOT
        .lock()
        .expect("window viewport snapshot mutex poisoned")
        .as_ref()
        .copied()
}

impl Default for HeadedEmbedderApp {
    fn default() -> Self {
        Self {
            window: None,
            renderer: VelloWindowRenderer::new(),
            chrome: None,
            browser: BrowserState::default(),
            automation: AutomationController::default(),
            provider: None,
            current_webview_id: None,
            has_top_level_traversable: false,
            window_occluded: false,
            animation_timer: None,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
        }
    }
}

impl HeadedEmbedderApp {
    fn has_visible_viewport(&self) -> bool {
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        if self.window_occluded {
            return false;
        }
        if matches!(window.is_visible(), Some(false)) {
            return false;
        }
        let size = window.inner_size();
        size.width > 0 && size.height > 0
    }

    fn pointer_position_in_viewport(&self, position: PhysicalPosition<f64>) -> bool {
        if !self.has_visible_viewport() {
            return false;
        }
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        let size = window.inner_size();
        position.x >= 0.0
            && position.y >= 0.0
            && position.x < f64::from(size.width)
            && position.y < f64::from(size.height)
    }

    fn chrome_height_css(&self) -> f32 {
        self.chrome
            .as_ref()
            .map(ChromeUi::height_css)
            .unwrap_or_default()
    }

    fn chrome_height_physical(&self) -> u32 {
        self.chrome
            .as_ref()
            .map(ChromeUi::height_physical)
            .unwrap_or_default()
    }

    fn content_has_visible_viewport(&self) -> bool {
        if !self.has_visible_viewport() {
            return false;
        }
        let Some(window) = self.window.as_ref() else {
            return false;
        };
        window.inner_size().height > self.chrome_height_physical()
    }

    fn pointer_position_in_chrome(&self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position_in_viewport(position)
            && position.y < f64::from(self.chrome_height_physical())
    }

    fn pointer_position_in_content_viewport(&self, position: PhysicalPosition<f64>) -> bool {
        self.pointer_position_in_viewport(position)
            && position.y >= f64::from(self.chrome_height_physical())
            && self.content_has_visible_viewport()
    }

    fn request_visible_redraw(&self, reason: &str) {
        if !self.has_visible_viewport() {
            return;
        }
        self.request_window_redraw();
        if let Some((provider, webview_id)) = self.provider.as_ref().zip(self.current_webview_id)
        {
            provider.note_rendering_opportunity(webview_id, reason);
        }
    }

    fn request_window_redraw(&self) {
        if !self.has_visible_viewport() {
            return;
        }
        let Some(window) = self.window.as_ref() else {
            return;
        };
        window.request_redraw();
    }

    fn content_viewport_snapshot(&self, window: &Window) -> (u32, u32, f32, ColorScheme) {
        let (width, height, scale, color_scheme) = viewport_snapshot_for_window(window);
        (
            width,
            height.saturating_sub(self.chrome_height_physical()),
            scale,
            color_scheme,
        )
    }

    fn update_content_viewport_snapshot(&mut self, window: &Window) {
        let viewport_snapshot = self.content_viewport_snapshot(window);
        update_window_viewport_snapshot(Some(viewport_snapshot));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(viewport_snapshot));
            if let Some(webview_id) = self.current_webview_id {
                let (width, height, scale, color_scheme) = viewport_snapshot;
                let _ = provider.set_traversable_viewport(
                    webview_id,
                    (width, height, scale, color_scheme),
                    0.0,
                    0.0,
                );
            }
        }
    }

    fn current_chrome_view_state(&self) -> ChromeViewState {
        ChromeViewState {
            address: self.browser.displayed_url(),
        }
    }

    fn sync_chrome_state(&mut self) {
        let chrome_view_state = self.current_chrome_view_state();
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.sync_state(&chrome_view_state);
        }
        if let Some(window) = self.window.clone() {
            self.update_content_viewport_snapshot(&window);
        }
    }

    fn resume_renderer_for_window(&mut self, window: &Arc<Window>) {
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height, || {});
            self.renderer.complete_resume();
        }
    }

    fn current_animation_time(&mut self) -> f64 {
        match self.animation_timer {
            Some(start) => Instant::now().duration_since(start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }

    fn create_window(event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let options = event_loop_options();
        let title = options
            .window_title
            .unwrap_or_else(|| String::from("formal-web"));
        let attributes: WindowAttributes = Window::default_attributes().with_title(title);
        event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("failed to create winit window: {error}"))
    }

    fn paint_current_frame(&mut self) {
        if !self.has_visible_viewport() {
            return;
        }
        let _ = self.current_animation_time();
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let chrome_height = f64::from(self.chrome_height_physical());
        let chrome_scene = self.chrome.as_mut().map(ChromeUi::paint_scene);

        if chrome_scene.is_none() && self.current_webview_id.is_none() {
            return;
        }

        let size = window.inner_size();

        if self.renderer.is_active() {
            self.renderer.set_size(size.width, size.height);
        } else {
            let window_handle: Arc<dyn anyrender::WindowHandle> = window.clone();
            self.renderer.resume(window_handle, size.width, size.height, || {});
            self.renderer.complete_resume();
        }

        let mut provider = self.provider.take();
        let current_webview_id = self.current_webview_id;
        self.renderer.render(|scene| {
            if let Some(webview_id) = current_webview_id
                && let Some(provider) = provider.as_mut()
            {
                let _ = provider.append_web_content_scene(
                    webview_id,
                    scene,
                    Affine::translate((0.0, chrome_height)),
                );
            }
            if let Some(chrome_scene) = chrome_scene.clone() {
                scene.append_scene(chrome_scene, Affine::IDENTITY);
            }
        });
        self.provider = provider;
    }

    fn logical_position(&self, position: PhysicalPosition<f64>) -> LogicalPosition<f32> {
        let scale = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        position.to_logical(scale)
    }

    fn chrome_pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x: screen_x, y: screen_y } = self.logical_position(position);
        PointerCoords {
            screen_x,
            screen_y,
            client_x: screen_x,
            client_y: screen_y,
            page_x: screen_x,
            page_y: screen_y,
        }
    }

    fn content_pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        let LogicalPosition::<f32> { x: screen_x, y: screen_y } = self.logical_position(position);
        let client_x = screen_x;
        let client_y = screen_y - self.chrome_height_css();
        PointerCoords {
            screen_x,
            screen_y,
            client_x,
            client_y,
            page_x: client_x,
            page_y: client_y,
        }
    }

    fn send_content_ui_event(
        &mut self,
        event: UiEvent,
        require_visible_viewport: bool,
    ) -> Result<(), String> {
        if require_visible_viewport {
            if !self.content_has_visible_viewport() {
                return Err(String::from("content viewport is not visible"));
            }
        } else {
            let Some(window) = self.window.as_ref() else {
                return Err(String::from("window is not initialized"));
            };
            if window.inner_size().height <= self.chrome_height_physical() {
                return Err(String::from("content viewport is not ready for automation clicks"));
            }
        }

        if !self.has_top_level_traversable {
            return Err(String::from("no top-level traversable is active"));
        }

        let Some(provider) = self.provider.as_mut() else {
            return Err(String::from("webview provider is not initialized"));
        };
        let Some(webview_id) = self.current_webview_id else {
            return Err(String::from("no current webview is active"));
        };

        provider.send_ui_event(webview_id, event)?;
        if require_visible_viewport {
            self.request_window_redraw();
        }
        Ok(())
    }

    fn dispatch_content_ui_event(&mut self, event: UiEvent) {
        let _ = self.send_content_ui_event(event, true);
    }

    fn handle_chrome_ui_event(&mut self, event: UiEvent) {
        if !self.has_visible_viewport() {
            return;
        }

        let action = self
            .chrome
            .as_mut()
            .and_then(|chrome| chrome.handle_ui_event(event));
        self.request_window_redraw();
        if let Some(action) = action {
            self.handle_chrome_action(action);
        }
    }

    fn start_navigation_request(&self, destination_url: &str) -> Result<(), String> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("webview provider is not initialized"))?;
        provider.navigate(self.current_webview_id, destination_url)
    }

    fn begin_navigation(&mut self, pending_navigation: PendingNavigation) -> Result<(), String> {
        self.start_navigation_request(&pending_navigation.url)?;
        self.browser.begin_navigation(pending_navigation);
        self.sync_chrome_state();
        self.request_window_redraw();
        Ok(())
    }

    fn handle_chrome_action(&mut self, action: ChromeAction) {
        let result = match action {
            ChromeAction::Navigate => {
                let Some(chrome) = self.chrome.as_ref() else {
                    return;
                };
                let Some(destination_url) = normalize_browser_destination(&chrome.address_value()) else {
                    return;
                };
                self.begin_navigation(PendingNavigation { url: destination_url })
            }
        };

        if let Err(error) = result {
            eprintln!("{error}");
        }
    }

    fn handle_navigation_requested(&mut self, webview_id: WebviewId, destination_url: String) {
        if self.current_webview_id == Some(webview_id) {
            self.browser.begin_navigation(PendingNavigation {
                url: destination_url,
            });
            self.sync_chrome_state();
            self.request_window_redraw();
        }
    }

    fn sync_browser_navigable_id_from_provider(&mut self) {
        let navigable_id = self
            .provider
            .as_ref()
            .zip(self.current_webview_id)
            .and_then(|(provider, webview_id)| provider.current_navigable_id(webview_id));
        self.browser.set_current_navigable_id(navigable_id);
    }

    fn with_automation_controller<R>(
        &mut self,
        f: impl FnOnce(&mut AutomationController, &mut Self) -> R,
    ) -> R {
        let mut automation = std::mem::take(&mut self.automation);
        let result = f(&mut automation, self);
        self.automation = automation;
        result
    }

    fn handle_navigation_completed(&mut self, completed: NavigationCompleted) {
        let is_current = self.current_webview_id == Some(completed.webview_id);

        match &completed.status {
            NavigationCompletion::Committed { url } => {
                if is_current {
                    self.browser.commit_navigation(url.clone());
                    self.sync_chrome_state();
                    self.request_window_redraw();
                    self.with_automation_controller(|automation, app| {
                        automation.note_navigation_committed(app)
                    });
                }

                if let Some(provider) = self.provider.as_mut() {
                    provider.on_navigation_committed(completed.webview_id);
                }
            }
            NavigationCompletion::Aborted { message } => {
                if is_current {
                    self.with_automation_controller(|automation, _app| {
                        automation.abort_pending_navigation(message.clone())
                    });
                    self.browser.cancel_pending_navigation();
                    self.sync_chrome_state();
                }
            }
        }

        self.request_window_redraw();
    }

    fn dispatch_automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Err(String::from("window is not initialized"));
        };

        let scale = window.scale_factor();
        let chrome_height_css = f64::from(self.chrome_height_css());
        let position = PhysicalPosition::new(
            f64::from(x) * scale,
            (f64::from(y) + chrome_height_css) * scale,
        );
        self.pointer_pos = position;

        if let Some(chrome) = self.chrome.as_mut() {
            chrome.clear_focus();
        }
        self.request_window_redraw();

        let modifiers = winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state());
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event), false)?;

        self.buttons |= MouseEventButton::Main.into();
        let down_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerDown(down_event), false)?;

        self.buttons.remove(MouseEventButton::Main.into());
        let up_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerUp(up_event), false)?;

        Ok(())
    }

    fn dispatch_automation_click_element(&self, selector: String) -> Result<(), String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((provider, webview_id)) => {
                provider.click_element(webview_id, selector)?;
                provider.note_rendering_opportunity(webview_id, "automation_element_click");
                Ok(())
            }
            None => Err(String::from(
                "no active top-level traversable is available for element click",
            )),
        }
    }

    fn dispatch_automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Err(String::from("window is not initialized"));
        };

        let scale = window.scale_factor();
        let chrome_height_css = f64::from(self.chrome_height_css());
        let position = PhysicalPosition::new(
            f64::from(x) * scale,
            (f64::from(y) + chrome_height_css) * scale,
        );
        self.pointer_pos = position;

        let modifiers = winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state());
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.content_pointer_coords(position),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event), false)?;

        self.send_content_ui_event(
            UiEvent::Wheel(BlitzWheelEvent {
                delta: BlitzWheelDelta::Pixels(f64::from(delta_x), f64::from(delta_y)),
                coords: self.content_pointer_coords(position),
                buttons: self.buttons,
                mods: modifiers,
            }),
            false,
        )?;

        Ok(())
    }
}

impl AutomationHost for HeadedEmbedderApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        self.sync_browser_navigable_id_from_provider();
        self.browser.automation_snapshot(
            self.current_webview_id,
            self.has_top_level_traversable,
        )
    }

    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        automation_visible_frame_viewports(&mut self.provider, self.current_webview_id)
    }

    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String> {
        automation_screenshot_png(&mut self.provider, self.current_webview_id)
    }

    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        self.begin_navigation(PendingNavigation { url })
    }

    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        self.dispatch_automation_click(x, y)
    }

    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        self.dispatch_automation_click_element(selector)
    }

    fn automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        self.dispatch_automation_scroll(x, y, delta_x, delta_y)
    }

    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((provider, webview_id)) => provider.evaluate_script(webview_id, source, timeout),
            None => Err(String::from(
                "no active top-level traversable is available for script execution",
            )),
        }
    }
}

impl ApplicationHandler<FormalWebUserEvent> for HeadedEmbedderApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match Self::create_window(event_loop) {
                Ok(window) => {
                    let full_viewport = viewport_of_snapshot(viewport_snapshot_for_window(&window));
                    let chrome_shell_provider: Arc<dyn ShellProvider> =
                        Arc::new(WinitShellProvider::new(window.clone()));
                    let chrome = match ChromeUi::new(full_viewport, chrome_shell_provider) {
                        Ok(chrome) => chrome,
                        Err(_error) => {
                            event_loop.exit();
                            return;
                        }
                    };
                    self.chrome = Some(chrome);
                    self.window = Some(window.clone());
                    self.sync_chrome_state();
                    self.update_content_viewport_snapshot(&window);
                    self.resume_renderer_for_window(&window);
                    let startup_url = event_loop_options().startup_url;
                    match startup_destination_url(startup_url.as_deref()) {
                        Ok(destination_url) => {
                            self.browser.begin_navigation(PendingNavigation {
                                url: destination_url,
                            });
                            self.sync_chrome_state();
                            if let Some(provider) = self.provider.as_ref() {
                                if provider.start(startup_url.as_deref()).is_err() {
                                    event_loop.exit();
                                }
                            }
                        }
                        Err(_error) => event_loop.exit(),
                    }
                    self.request_window_redraw();
                }
                Err(_error) => event_loop.exit(),
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(current_window_id) = self.window.as_ref().map(|window| window.id()) else {
            return;
        };
        if current_window_id != window_id {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                if self.provider.is_some() || self.chrome.is_some() {
                    self.paint_current_frame();
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.as_ref() {
                    let full_viewport = viewport_of_snapshot(viewport_snapshot_for_window(window));
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.set_viewport(full_viewport);
                    }
                    self.sync_chrome_state();
                }
                if self.renderer.is_active() {
                    self.renderer.set_size(size.width, size.height);
                }
                if self.has_top_level_traversable {
                    self.request_visible_redraw("request_redraw");
                } else {
                    self.request_window_redraw();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                if let Some(window) = self.window.as_ref() {
                    window.set_visible(false);
                }
                self.with_automation_controller(|automation, _app| {
                    automation.abort_pending_navigation(String::from(
                        "window closed before navigation completed",
                    ))
                });
                self.renderer.suspend();
                self.animation_timer = None;
                self.chrome = None;
                self.browser = BrowserState::default();
                self.provider = None;
                self.current_webview_id = None;
                self.has_top_level_traversable = false;
                self.window_occluded = false;
                update_window_viewport_snapshot(None);
                self.window = None;
                event_loop.exit();
            }
            WindowEvent::Ime(ime_event) => {
                let event = UiEvent::Ime(winit_ime_to_blitz(ime_event));
                if self.chrome.as_ref().is_some_and(ChromeUi::takes_text_input_focus) {
                    self.handle_chrome_ui_event(event);
                } else {
                    self.dispatch_content_ui_event(event);
                }
            }
            WindowEvent::ModifiersChanged(new_state) => {
                self.keyboard_modifiers = new_state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key_event = winit_key_event_to_blitz(&event, self.keyboard_modifiers.state());
                let event = if event.state.is_pressed() {
                    UiEvent::KeyDown(key_event)
                } else {
                    UiEvent::KeyUp(key_event)
                };
                if self.chrome.as_ref().is_some_and(ChromeUi::takes_text_input_focus) {
                    self.handle_chrome_ui_event(event);
                } else {
                    self.dispatch_content_ui_event(event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer_pos = position;
                if self.pointer_position_in_chrome(position) {
                    self.handle_chrome_ui_event(UiEvent::PointerMove(BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.chrome_pointer_coords(position),
                        button: Default::default(),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    }));
                } else if self.pointer_position_in_content_viewport(position) {
                    self.dispatch_content_ui_event(UiEvent::PointerMove(BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.content_pointer_coords(position),
                        button: Default::default(),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    }));
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if !self.pointer_position_in_viewport(self.pointer_pos) {
                    return;
                }
                let mapped_button = match button {
                    MouseButton::Left => MouseEventButton::Main,
                    MouseButton::Right => MouseEventButton::Secondary,
                    MouseButton::Middle => MouseEventButton::Auxiliary,
                    MouseButton::Back => MouseEventButton::Fourth,
                    MouseButton::Forward => MouseEventButton::Fifth,
                    MouseButton::Other(_) => MouseEventButton::Auxiliary,
                };
                match state {
                    ElementState::Pressed => self.buttons |= mapped_button.into(),
                    ElementState::Released => self.buttons.remove(mapped_button.into()),
                }
                if self.pointer_position_in_chrome(self.pointer_pos) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.chrome_pointer_coords(self.pointer_pos),
                        button: mapped_button,
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    };
                    self.handle_chrome_ui_event(match state {
                        ElementState::Pressed => UiEvent::PointerDown(event),
                        ElementState::Released => UiEvent::PointerUp(event),
                    });
                } else if self.pointer_position_in_content_viewport(self.pointer_pos) {
                    if state.is_pressed() {
                        if let Some(chrome) = self.chrome.as_mut() {
                            chrome.clear_focus();
                        }
                        self.request_window_redraw();
                    }
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Mouse,
                        is_primary: true,
                        coords: self.content_pointer_coords(self.pointer_pos),
                        button: mapped_button,
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default(),
                    };
                    self.dispatch_content_ui_event(match state {
                        ElementState::Pressed => UiEvent::PointerDown(event),
                        ElementState::Released => UiEvent::PointerUp(event),
                    });
                }
            }
            WindowEvent::Touch(Touch { phase, location, force, id, .. }) => {
                if !self.pointer_position_in_viewport(location) {
                    return;
                }
                if self.pointer_position_in_chrome(location) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: self.chrome_pointer_coords(location),
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    match phase {
                        TouchPhase::Started => self.handle_chrome_ui_event(UiEvent::PointerDown(event)),
                        TouchPhase::Moved => self.handle_chrome_ui_event(UiEvent::PointerMove(event)),
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.handle_chrome_ui_event(UiEvent::PointerUp(event))
                        }
                    }
                } else if self.pointer_position_in_content_viewport(location) {
                    let event = BlitzPointerEvent {
                        id: BlitzPointerId::Finger(id),
                        is_primary: true,
                        coords: self.content_pointer_coords(location),
                        button: Default::default(),
                        buttons: MouseEventButtons::None,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: touch_pointer_details(force),
                    };
                    match phase {
                        TouchPhase::Started => {
                            if let Some(chrome) = self.chrome.as_mut() {
                                chrome.clear_focus();
                            }
                            self.request_window_redraw();
                            self.dispatch_content_ui_event(UiEvent::PointerDown(event))
                        }
                        TouchPhase::Moved => self.dispatch_content_ui_event(UiEvent::PointerMove(event)),
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.dispatch_content_ui_event(UiEvent::PointerUp(event))
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !self.pointer_position_in_viewport(self.pointer_pos) {
                    return;
                }
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => BlitzWheelDelta::Lines(x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(pos) => BlitzWheelDelta::Pixels(pos.x, pos.y),
                };
                if self.pointer_position_in_chrome(self.pointer_pos) {
                    self.handle_chrome_ui_event(UiEvent::Wheel(BlitzWheelEvent {
                        delta,
                        coords: self.chrome_pointer_coords(self.pointer_pos),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    }));
                } else if self.pointer_position_in_content_viewport(self.pointer_pos) {
                    self.dispatch_content_ui_event(UiEvent::Wheel(BlitzWheelEvent {
                        delta,
                        coords: self.content_pointer_coords(self.pointer_pos),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    }));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::Paint(snapshot) => {
                let Some(provider) = self.provider.as_mut() else {
                    return;
                };
                match provider.on_paint_frame(snapshot) {
                    Ok(()) => {
                        self.with_automation_controller(|automation, app| {
                            automation.note_rendering_update(app)
                        });
                        self.request_window_redraw();
                    }
                    Err(error) => {
                        eprintln!("paint error: {error}");
                    }
                }
            }
            FormalWebUserEvent::RequestRedraw(webview_id) => {
                if self.current_webview_id == Some(webview_id) {
                    self.request_window_redraw();
                }
            }
            FormalWebUserEvent::NavigationRequested { webview_id, destination_url } => {
                self.handle_navigation_requested(webview_id, destination_url);
            }
            FormalWebUserEvent::NavigationCompleted(completed) => {
                self.handle_navigation_completed(completed);
            }
            FormalWebUserEvent::NewTopLevelTraversable(webview_id, target_name) => {
                if let Some(child_navigable_host) =
                    parse_child_navigable_host_target(&target_name)
                {
                    if let Some(provider) = self.provider.as_mut() {
                        provider.register_child_navigable_host(
                            webview_id,
                            child_navigable_host.parent_traversable_id,
                            child_navigable_host.content_frame_id,
                        );
                    }
                } else {
                    self.has_top_level_traversable = true;
                    self.current_webview_id = Some(webview_id);
                    if let Some(provider) = self.provider.as_mut() {
                        provider.on_new_top_level_traversable(webview_id);
                    }
                    self.request_visible_redraw("request_redraw");
                }
            }
            FormalWebUserEvent::Automation(command) => {
                self.with_automation_controller(|automation, app| {
                    automation.handle_command(app, command)
                });
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::Exit => {
                event_loop.exit();
            }
        }
    }
}
