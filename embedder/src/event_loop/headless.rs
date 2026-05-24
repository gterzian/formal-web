use super::{
    BrowserState, FormalWebUserEvent, NavigationCompleted, NavigationCompletion,
    PendingNavigation, automation_screenshot_png, automation_visible_frame_viewports,
    read_clipboard_text, startup_destination_url, update_window_viewport_snapshot,
    write_clipboard_text,
};
use super::winit_integration::event_loop_options;
use automation::{AutomationController, AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::ColorScheme;
use ipc_messages::content::WebviewId;
use keyboard_types::Modifiers as KeyboardModifiers;
use serde_json::Value;
use std::time::Duration;
use webview::WebviewProvider;
use ::winit::application::ApplicationHandler;
use ::winit::event::WindowEvent;
use ::winit::event_loop::ActiveEventLoop;
use ::winit::window::WindowId;

const HEADLESS_VIEWPORT_WIDTH: u32 = 800;
const HEADLESS_VIEWPORT_HEIGHT: u32 = 600;

fn input_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

fn ui_event_summary(event: &UiEvent) -> String {
    match event {
        UiEvent::PointerMove(event)
        | UiEvent::PointerDown(event)
        | UiEvent::PointerUp(event) => format!(
            "pointer client=({:.1},{:.1})",
            event.coords.client_x, event.coords.client_y
        ),
        UiEvent::Wheel(event) => format!(
            "wheel client=({:.1},{:.1}) delta={:?}",
            event.coords.client_x, event.coords.client_y, event.delta
        ),
        UiEvent::KeyDown(_) => String::from("key-down"),
        UiEvent::KeyUp(_) => String::from("key-up"),
        UiEvent::Ime(_) => String::from("ime"),
        UiEvent::AppleStandardKeybinding(_) => String::from("apple-keybinding"),
    }
}

fn headless_viewport_snapshot() -> (u32, u32, f32, ColorScheme) {
    (
        HEADLESS_VIEWPORT_WIDTH,
        HEADLESS_VIEWPORT_HEIGHT,
        1.0,
        ColorScheme::Light,
    )
}

pub(super) struct HeadlessEmbedderApp {
    pub(super) started: bool,
    pub(super) browser: BrowserState,
    pub(super) automation: AutomationController,
    pub(super) provider: Option<WebviewProvider>,
    pub(super) current_webview_id: Option<WebviewId>,
    pub(super) has_top_level_traversable: bool,
    pub(super) buttons: MouseEventButtons,
}

impl Default for HeadlessEmbedderApp {
    fn default() -> Self {
        Self {
            started: false,
            browser: BrowserState::default(),
            automation: AutomationController::default(),
            provider: None,
            current_webview_id: None,
            has_top_level_traversable: false,
            buttons: MouseEventButtons::None,
        }
    }
}

impl HeadlessEmbedderApp {
    fn apply_viewport_snapshot(&mut self) {
        let viewport_snapshot = headless_viewport_snapshot();
        update_window_viewport_snapshot(Some(viewport_snapshot));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(viewport_snapshot));
            if let Some(webview_id) = self.current_webview_id {
                let _ = provider.set_traversable_viewport(webview_id, viewport_snapshot, 0.0, 0.0);
            }
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
        Ok(())
    }

    fn handle_navigation_requested(&mut self, webview_id: WebviewId, destination_url: String) {
        if self.current_webview_id == Some(webview_id) {
            self.browser.begin_navigation(PendingNavigation {
                url: destination_url,
            });
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
                }
            }
        }
    }

    fn pointer_coords(&self, x: f32, y: f32) -> PointerCoords {
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            page_x: x,
            page_y: y,
        }
    }

    fn send_content_ui_event(&mut self, event: UiEvent) -> Result<(), String> {
        if !self.has_top_level_traversable {
            return Err(String::from("no top-level traversable is active"));
        }

        let Some(provider) = self.provider.as_mut() else {
            return Err(String::from("webview provider is not initialized"));
        };
        let Some(webview_id) = self.current_webview_id else {
            return Err(String::from("no current webview is active"));
        };

        if input_debug_enabled() {
            eprintln!(
                "[input-debug][embedder-headless] send_ui_event webview={} {}",
                webview_id.0,
                ui_event_summary(&event)
            );
        }

        provider.send_ui_event(webview_id, event)
    }

    fn dispatch_automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        if input_debug_enabled() {
            eprintln!(
                "[input-debug][embedder-headless] automation_click at=({x:.1},{y:.1})"
            );
        }
        let modifiers = KeyboardModifiers::default();
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.pointer_coords(x, y),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event))?;

        self.buttons |= MouseEventButton::Main.into();
        let down_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.pointer_coords(x, y),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerDown(down_event))?;

        self.buttons.remove(MouseEventButton::Main.into());
        let up_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.pointer_coords(x, y),
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerUp(up_event))
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
        if input_debug_enabled() {
            eprintln!(
                "[input-debug][embedder-headless] automation_scroll at=({x:.1},{y:.1}) delta=({delta_x:.1},{delta_y:.1})"
            );
        }
        let modifiers = KeyboardModifiers::default();
        let move_event = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.pointer_coords(x, y),
            button: Default::default(),
            buttons: self.buttons,
            mods: modifiers,
            details: PointerDetails::default(),
        };
        self.send_content_ui_event(UiEvent::PointerMove(move_event))?;

        self.send_content_ui_event(UiEvent::Wheel(BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(f64::from(delta_x), f64::from(delta_y)),
            coords: self.pointer_coords(x, y),
            buttons: self.buttons,
            mods: modifiers,
        }))
    }
}

impl AutomationHost for HeadlessEmbedderApp {
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

impl ApplicationHandler<FormalWebUserEvent> for HeadlessEmbedderApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        self.apply_viewport_snapshot();

        let startup_url = event_loop_options().startup_url;
        match startup_destination_url(startup_url.as_deref()) {
            Ok(destination_url) => {
                self.browser.begin_navigation(PendingNavigation {
                    url: destination_url,
                });
                if let Some(provider) = self.provider.as_ref()
                    && provider.start(startup_url.as_deref()).is_err()
                {
                    update_window_viewport_snapshot(None);
                    event_loop.exit();
                }
            }
            Err(_error) => {
                update_window_viewport_snapshot(None);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
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
                    }
                    Err(error) => {
                        eprintln!("paint error: {error}");
                    }
                }
            }
            FormalWebUserEvent::RequestRedraw(_webview_id) => {}
            FormalWebUserEvent::NavigationRequested { webview_id, destination_url } => {
                self.handle_navigation_requested(webview_id, destination_url);
            }
            FormalWebUserEvent::NavigationCompleted(completed) => {
                self.handle_navigation_completed(completed);
            }
            FormalWebUserEvent::RegisterChildNavigableHost {
                child_webview_id,
                parent_traversable_id,
                content_frame_id,
            } => {
                if let Some(provider) = self.provider.as_mut() {
                    provider.register_child_navigable_host(
                        child_webview_id,
                        parent_traversable_id,
                        content_frame_id,
                    );
                }
            }
            FormalWebUserEvent::NewTopLevelTraversable(webview_id, target_name) => {
                let _ = target_name;
                self.has_top_level_traversable = true;
                self.current_webview_id = Some(webview_id);
                if let Some(provider) = self.provider.as_mut() {
                    provider.on_new_top_level_traversable(webview_id);
                }
                self.apply_viewport_snapshot();
                if let Some(provider) = self.provider.as_ref() {
                    provider.note_rendering_opportunity(webview_id, "request_redraw");
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
                self.with_automation_controller(|automation, _app| {
                    automation.abort_pending_navigation(String::from(
                        "headless embedder exited before navigation completed",
                    ))
                });
                update_window_viewport_snapshot(None);
                event_loop.exit();
            }
        }
    }
}