//! `AutomationHost` for the AppKit (`MacApp`) embedder.
//!
//! This is a child module of `app` so the implementation can reach the
//! private `MacApp` internals (windows, provider, per-tab state) that the
//! automation commands operate on without widening their visibility.

use super::{MacApp, capture_web_view_png};
use automation::{AutomationHost, AutomationSnapshot, AutomationVisibleFrameViewport};
use keyboard_types::Modifiers as KeyboardModifiers;
use serde_json::Value;
use std::time::Duration;
use webview::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, Point, PointerDetails, UiEvent,
};

impl AutomationHost for MacApp {
    fn automation_snapshot(&mut self) -> AutomationSnapshot {
        let webview_id = self.active_tab_webview_id();
        let tab = webview_id.and_then(|webview_id| {
            self.active_window_id
                .and_then(|window_id| self.windows.get(&window_id))
                .and_then(|window_state| window_state.tabs.get(&webview_id))
        });
        let current_url = tab.and_then(|tab| tab.committed_url.clone());
        let displayed_url = tab.map(|tab| tab.display_url()).unwrap_or_default();
        AutomationSnapshot {
            webview_id,
            current_url,
            displayed_url,
            navigable_id: None,
            has_top_level_traversable: webview_id.is_some(),
        }
    }

    fn automation_visible_frame_viewports(
        &mut self,
    ) -> Result<Vec<AutomationVisibleFrameViewport>, String> {
        Ok(Vec::new())
    }

    fn automation_screenshot(&mut self) -> Result<Vec<u8>, String> {
        let Some(window_id) = self.active_window_id else {
            return Err(String::from("no active window"));
        };
        let Some(window_state) = self.windows.get(&window_id) else {
            return Err(String::from("active window state missing"));
        };
        capture_web_view_png(&window_state.web_view)
    }

    fn begin_automation_navigation(&mut self, url: String) -> Result<(), String> {
        let Some(window_id) = self.active_window_id else {
            return Err(String::from("no active window"));
        };
        let webview_id = self
            .windows
            .get(&window_id)
            .and_then(|window_state| window_state.active_tab);
        let Some(provider) = self.provider.as_ref() else {
            return Err(String::from("no provider"));
        };
        provider.navigate(webview_id, &url)?;
        if let Some(window_state) = self.windows.get_mut(&window_id)
            && let Some(webview_id) = webview_id
            && let Some(tab) = window_state.tabs.get_mut(&webview_id)
        {
            tab.pending_url = Some(url);
        }
        Ok(())
    }

    fn automation_click(&mut self, x: f32, y: f32) -> Result<(), String> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?;
        let webview_id = self
            .active_tab_webview_id()
            .ok_or_else(|| String::from("no active webview"))?;
        let mods = KeyboardModifiers::default();
        let coords = self.automation_coords(x, y);
        let mut buttons: MouseEventButtons = MouseEventButtons::None;
        provider.send_ui_event(
            webview_id,
            UiEvent::PointerMove(BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords,
                button: Default::default(),
                buttons,
                mods,
                details: PointerDetails::default(),
                element: Point { x: 0.0, y: 0.0 },
                active_pointers: Default::default(),
            }),
        )?;
        buttons |= MouseEventButton::Main.into();
        provider.send_ui_event(
            webview_id,
            UiEvent::PointerDown(BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords,
                button: MouseEventButton::Main,
                buttons,
                mods,
                details: PointerDetails::default(),
                element: Point { x: 0.0, y: 0.0 },
                active_pointers: Default::default(),
            }),
        )?;
        buttons.remove(MouseEventButton::Main.into());
        provider.send_ui_event(
            webview_id,
            UiEvent::PointerUp(BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords,
                button: MouseEventButton::Main,
                buttons,
                mods,
                details: PointerDetails::default(),
                element: Point { x: 0.0, y: 0.0 },
                active_pointers: Default::default(),
            }),
        )
    }

    fn automation_click_element(&mut self, selector: String) -> Result<(), String> {
        match self.provider.as_ref().zip(self.active_tab_webview_id()) {
            Some((provider, webview_id)) => provider.click_element(webview_id, selector)?,
            None => return Err(String::from("no webview")),
        }
        Ok(())
    }

    fn automation_scroll(
        &mut self,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| String::from("no provider"))?;
        let webview_id = self
            .active_tab_webview_id()
            .ok_or_else(|| String::from("no active webview"))?;
        let mods = KeyboardModifiers::default();
        let coords = self.automation_coords(x, y);
        provider.send_ui_event(
            webview_id,
            UiEvent::PointerMove(BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords,
                button: Default::default(),
                buttons: MouseEventButtons::None,
                mods,
                details: PointerDetails::default(),
                element: Point { x: 0.0, y: 0.0 },
                active_pointers: Default::default(),
            }),
        )?;
        provider.send_ui_event(
            webview_id,
            UiEvent::Wheel(BlitzWheelEvent {
                delta: BlitzWheelDelta::Pixels(f64::from(delta_x), f64::from(delta_y)),
                coords,
                buttons: MouseEventButtons::None,
                mods,
                element: Point { x: 0.0, y: 0.0 },
            }),
        )
    }

    fn automation_evaluate_script(
        &mut self,
        source: String,
        timeout: Duration,
    ) -> Result<Value, String> {
        match self.provider.as_ref().zip(self.active_tab_webview_id()) {
            Some((provider, webview_id)) => provider.evaluate_script(webview_id, source, timeout),
            None => Err(String::from("no webview")),
        }
    }
}
