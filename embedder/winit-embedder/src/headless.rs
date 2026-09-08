use crate::events::FormalWebUserEvent;
use crate::shared::{
    automation_screenshot_png, read_clipboard_text, startup_destination_url,
    update_window_viewport_snapshot, write_clipboard_text,
};
use ::winit::application::ApplicationHandler;
use ::winit::event::WindowEvent;
use ::winit::event_loop::ActiveEventLoop;
use ::winit::window::WindowId;
use automation::{
    AutomationController, AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport,
};
use keyboard_types::Modifiers as KeyboardModifiers;
use log::{debug, error};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use webview::WebviewProvider;
use webview::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, ColorScheme,
    FontTransportReceiver, MouseEventButton, MouseEventButtons, NavigationCompleted,
    NavigationCompletion, PointerCoords, PointerDetails, RecordedScene, UiEvent, WebviewId,
};

const HEADLESS_VIEWPORT_WIDTH: u32 = 800;
const HEADLESS_VIEWPORT_HEIGHT: u32 = 600;

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
    pub(super) startup_url: Option<String>,
    pub(super) pending_url: Option<String>,
    pub(super) committed_url: Option<String>,
    pub(super) automation: AutomationController,
    pub(super) provider: Option<WebviewProvider>,
    pub(super) current_webview_id: Option<WebviewId>,
    pub(super) buttons: MouseEventButtons,
    pub(super) composed_scenes: HashMap<WebviewId, RecordedScene>,
    pub(super) scene_font_receiver: FontTransportReceiver,
}

impl Default for HeadlessEmbedderApp {
    fn default() -> Self {
        Self {
            started: false,
            startup_url: None,
            pending_url: None,
            committed_url: None,
            automation: AutomationController::default(),
            provider: None,
            current_webview_id: None,
            buttons: MouseEventButtons::None,
            composed_scenes: HashMap::new(),
            scene_font_receiver: FontTransportReceiver::default(),
        }
    }
}

impl HeadlessEmbedderApp {
    fn displayed_url(&self) -> String {
        self.pending_url
            .clone()
            .or_else(|| self.committed_url.clone())
            .unwrap_or_default()
    }

    fn ensure_started(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        self.apply_viewport_snapshot();
        let startup_url = self.startup_url.clone();
        if let Ok(destination_url) = startup_destination_url(startup_url.as_deref()) {
            self.pending_url = Some(destination_url);
            if let Some(provider) = self.provider.as_ref()
                && provider.start(startup_url.as_deref(), Vec::new()).is_err()
            {
                update_window_viewport_snapshot(None);
                event_loop.exit();
            }
        } else {
            update_window_viewport_snapshot(None);
            event_loop.exit();
        }
    }

    fn apply_viewport_snapshot(&mut self) {
        let vs = headless_viewport_snapshot();
        update_window_viewport_snapshot(Some(vs));
        if let Some(provider) = self.provider.as_mut() {
            let _ = provider.set_default_viewport(Some(vs));
            if let Some(wid) = self.current_webview_id {
                let _ = provider.set_traversable_viewport(wid, vs, 0.0, 0.0);
            }
        }
    }

    fn begin_navigation(&mut self, url: String) -> Result<(), String> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?;
        provider.navigate(self.current_webview_id, &url)?;
        self.pending_url = Some(url);
        Ok(())
    }

    fn handle_navigation_requested(&mut self, webview_id: WebviewId, destination_url: String) {
        if self.current_webview_id.is_none() {
            self.current_webview_id = Some(webview_id);
            self.apply_viewport_snapshot();
        }
        if self.current_webview_id == Some(webview_id) {
            self.pending_url = Some(destination_url);
        }
    }

    fn with_automation<R>(
        &mut self,
        f: impl FnOnce(&mut AutomationController, &mut Self) -> R,
    ) -> R {
        let mut a = std::mem::take(&mut self.automation);
        let r = f(&mut a, self);
        self.automation = a;
        r
    }

    fn handle_navigation_completed(&mut self, completed: NavigationCompleted) {
        if self.current_webview_id.is_none() {
            self.current_webview_id = Some(completed.webview_id);
            self.apply_viewport_snapshot();
        }
        let is_current = self.current_webview_id == Some(completed.webview_id);
        match &completed.status {
            NavigationCompletion::Committed { url } => {
                if is_current {
                    self.pending_url = None;
                    self.committed_url = Some(url.clone());
                    self.with_automation(|a, app| a.note_navigation_committed(app));
                }
                if let Some(p) = self.provider.as_mut() {
                    p.on_navigation_committed(completed.webview_id);
                }
            }
            NavigationCompletion::Aborted { message } => {
                if is_current {
                    self.with_automation(|a, _| a.abort_pending_navigation(message.clone()));
                    self.pending_url = None;
                }
            }
        }
    }

    fn coords(&self, x: f32, y: f32) -> PointerCoords {
        PointerCoords {
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            page_x: x,
            page_y: y,
        }
    }

    fn send_ui_event(&mut self, event: UiEvent) -> Result<(), String> {
        let Some(provider) = self.provider.as_mut() else {
            return Err(String::from("no provider"));
        };
        let Some(wid) = self.current_webview_id else {
            return Err(String::from("no webview"));
        };
        provider.send_ui_event(wid, event)
    }
}

impl AutomationHost for HeadlessEmbedderApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        AutomationSnapshot {
            webview_id: self.current_webview_id,
            current_url: self.committed_url.clone(),
            displayed_url: self.displayed_url(),
            navigable_id: None,
            has_top_level_traversable: self.current_webview_id.is_some(),
        }
    }

    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        Ok(Vec::new())
    }

    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String> {
        automation_screenshot_png()
    }

    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        self.begin_navigation(url)
    }

    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let mods = KeyboardModifiers::default();
        let coords = self.coords(x, y);
        let move_ev = UiEvent::PointerMove(BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: Default::default(),
            buttons: self.buttons,
            mods,
            details: PointerDetails::default(),
        });
        self.send_ui_event(move_ev)?;
        let cur_bt = self.buttons | MouseEventButton::Main.into();
        let down_ev = UiEvent::PointerDown(BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: MouseEventButton::Main,
            buttons: cur_bt,
            mods,
            details: PointerDetails::default(),
        });
        self.buttons = cur_bt;
        self.send_ui_event(down_ev)?;
        self.buttons.remove(MouseEventButton::Main.into());
        let up_ev = UiEvent::PointerUp(BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: MouseEventButton::Main,
            buttons: self.buttons,
            mods,
            details: PointerDetails::default(),
        });
        self.send_ui_event(up_ev)
    }

    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((p, wid)) => {
                p.click_element(wid, selector)?;
                Ok(())
            }
            None => Err(String::from("no webview")),
        }
    }

    fn automation_scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), String> {
        let mods = KeyboardModifiers::default();
        let mk = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.coords(x, y),
            button: Default::default(),
            buttons: self.buttons,
            mods,
            details: PointerDetails::default(),
        };
        self.send_ui_event(UiEvent::PointerMove(mk))?;
        self.send_ui_event(UiEvent::Wheel(BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(f64::from(dx), f64::from(dy)),
            coords: self.coords(x, y),
            buttons: self.buttons,
            mods,
        }))
    }

    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        match self.provider.as_ref().zip(self.current_webview_id) {
            Some((p, wid)) => p.evaluate_script(wid, source, timeout),
            None => Err(String::from("no webview")),
        }
    }
}

impl ApplicationHandler<FormalWebUserEvent> for HeadlessEmbedderApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.ensure_started(el);
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _wid: WindowId, _ev: WindowEvent) {}

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.ensure_started(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: FormalWebUserEvent) {
        match event {
            FormalWebUserEvent::RequestRedraw(webview_id) => {
                // Headless has no window to paint; notify the UA that a
                // frame is needed for the requested webview.
                if let Some(provider) = self.provider.as_ref()
                    && let Err(error) = provider.frame_needed(webview_id)
                {
                    error!("[embedder] frame needed: {error}");
                }
            }
            FormalWebUserEvent::NavigationRequested {
                webview_id,
                destination_url,
            } => {
                self.handle_navigation_requested(webview_id, destination_url);
            }
            FormalWebUserEvent::NavigationCompleted(c) => self.handle_navigation_completed(c),
            FormalWebUserEvent::NewWebview(wid, _) => {
                // Keep the first (primary) webview as the automation target:
                // popups opened by window.open must not steal the WebDriver
                // session's current window, or test reports would be polled
                // against the popup instead of the test page.
                if self.current_webview_id.is_none() {
                    self.current_webview_id = Some(wid);
                }
                self.apply_viewport_snapshot();
            }
            // Headless has no chrome to label; the title is not needed.
            FormalWebUserEvent::TitleChanged { .. } => {}
            FormalWebUserEvent::CreateWindow => {}
            FormalWebUserEvent::Automation(cmd) => {
                self.with_automation(|a, app| a.handle_command(app, cmd));
            }
            FormalWebUserEvent::ClipboardRead { reply } => {
                let _ = reply.send(read_clipboard_text());
            }
            FormalWebUserEvent::ClipboardWrite { text, reply } => {
                let _ = reply.send(write_clipboard_text(text));
            }
            FormalWebUserEvent::NewWebContentScene {
                webview_id,
                scene_bytes,
                font_registrations,
                font_data,
            } => {
                // Register fonts from the graphics process.
                self.scene_font_receiver
                    .register_fonts(font_registrations, &font_data);
                // Deserialize and store the composed scene.
                match webview::deserialize_scene_from_slice(&scene_bytes) {
                    Ok(scene) => {
                        self.composed_scenes.insert(webview_id, scene);
                    }
                    Err(error) => {
                        error!("[embedder] failed to deserialize composed scene: {error}");
                    }
                }
            }
            FormalWebUserEvent::NewWebContentLayers {
                webview_id, layers, ..
            } => {
                debug!(
                    "[embedder] headless NewWebContentLayers {:?} ({} layers)",
                    webview_id,
                    layers.len()
                );
                // Headless has no display pacing; request the next frame
                // immediately.
                if let Some(provider) = self.provider.as_ref()
                    && let Err(error) = provider.frame_needed(webview_id)
                {
                    error!("[embedder] frame needed: {error}");
                }
            }
            FormalWebUserEvent::Exit => {
                self.with_automation(|a, _| {
                    a.abort_pending_navigation(String::from("headless exited"))
                });
                update_window_viewport_snapshot(None);
                el.exit();
            }
        }
    }
}
