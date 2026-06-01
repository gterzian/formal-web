use anyrender::Scene as RenderScene;
use blitz_dom::{BaseDocument, Document, DocumentConfig, qual_name};
use blitz_html::HtmlDocument;
use blitz_paint::paint_scene;
use blitz_traits::events::{BlitzKeyEvent, UiEvent};
use blitz_traits::shell::{ClipboardError, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use keyboard_types::Key;
use std::sync::Arc;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::{Cursor, Window};

const DEFAULT_CHROME_HEIGHT_CSS: f32 = 80.0;

const CHROME_HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html>
  <head>
    <style>
      :root { color-scheme: light; }
      html, body { margin: 0; padding: 0; }
      body {
        background: rgba(249, 248, 244, 0.98);
        color: #2f3134;
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
        font-size: 11px;
        line-height: 1.25;
        user-select: none;
        -webkit-user-select: none;
      }
      #chrome-shell {
        box-sizing: border-box;
        display: block;
        min-height: 40px;
        padding: 4px 8px;
        background: linear-gradient(180deg, rgba(252, 251, 247, 0.99), rgba(247, 245, 240, 0.97));
        border-bottom: 1px solid rgba(47, 49, 52, 0.14);
      }
      #tab-strip {
        box-sizing: border-box;
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 2px;
        padding: 2px 0 4px 0;
        min-height: 32px;
      }
      .tab-button {
        box-sizing: border-box;
        display: inline-flex;
        align-items: center;
        padding: 4px 12px;
        border: 1px solid rgba(102, 110, 120, 0.18);
        border-radius: 6px;
        background: rgba(255, 255, 255, 0.5);
        color: #2f3134;
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
        font-size: 11px;
        cursor: default;
        max-width: 160px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex-shrink: 1;
        min-width: 60px;
      }
      .tab-button.active {
        background: rgba(255, 255, 255, 0.94);
        border-color: rgba(78, 109, 142, 0.35);
        font-weight: 500;
      }
      .tab-button:hover { background: rgba(255, 255, 255, 0.75); }
      #new-tab-btn {
        box-sizing: border-box;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 4px 8px;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: #2f3134;
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
        font-size: 16px;
        font-weight: 400;
        cursor: default;
        line-height: 1;
        flex-shrink: 0;
        width: 28px;
        height: 28px;
      }
      #new-tab-btn:hover { background: rgba(47, 49, 52, 0.08); }
      .tab-spacer { flex: 1; }
      #address {
        box-sizing: border-box;
        display: block;
        width: 100%;
        height: 42px;
        padding: 0 11px;
        line-height: normal;
        border: 1px solid rgba(102, 110, 120, 0.24);
        border-radius: 16px;
        background: rgba(255, 255, 255, 0.94);
        color: #2f3134;
        font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
        font-size: 12px;
        appearance: none;
        -webkit-appearance: none;
        outline: none;
        user-select: text;
        -webkit-user-select: text;
      }
      #address:focus {
        outline: none;
        border-color: rgba(78, 109, 142, 0.42);
      }
    </style>
  </head>
  <body>
    <div id="chrome-shell">
      <div id="tab-strip">
        __TABS__
        <div class="tab-spacer"></div>
        <div id="new-tab-btn">+</div>
      </div>
      <input id="address" type="text" value="__URL__" spellcheck="false" autocomplete="off" />
    </div>
  </body>
</html>
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChromeAction {
    Navigate,
    NewTab,
    NewWindow,
    SwitchTab(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeTabInfo {
    pub label: String,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeViewState {
    pub address: String,
    pub tabs: Vec<ChromeTabInfo>,
}

struct ChromeNodeIds {
    shell: usize,
    address: usize,
}

pub struct ChromeUi {
    document: BaseDocument,
    node_ids: ChromeNodeIds,
    full_viewport: Viewport,
    height_css: f32,
    shell_provider: Arc<dyn ShellProvider>,
    last_tab_count: usize,
    last_labels: Vec<String>,
}

fn build_html(url: &str, tabs: &[ChromeTabInfo]) -> String {
    let tab_html: String = tabs
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let active = if info.active { " active" } else { "" };
            format!(
                r#"<button id="tab-{}" class="tab-button{}">{}</button>"#,
                i, active, info.label
            )
        })
        .collect::<Vec<_>>()
        .join("\n        ");
    let escaped_url = url
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    CHROME_HTML_TEMPLATE
        .replace("__TABS__", &tab_html)
        .replace("__URL__", &escaped_url)
}

impl ChromeUi {
    pub fn new(
        full_viewport: Viewport,
        shell_provider: Arc<dyn ShellProvider>,
    ) -> Result<Self, String> {
        Self::from_html(full_viewport, shell_provider, "", &[])
    }

    fn from_html(
        full_viewport: Viewport,
        shell_provider: Arc<dyn ShellProvider>,
        url: &str,
        tabs: &[ChromeTabInfo],
    ) -> Result<Self, String> {
        let html = build_html(url, tabs);
        let mut document = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                viewport: Some(Self::chrome_viewport(
                    &full_viewport,
                    DEFAULT_CHROME_HEIGHT_CSS,
                )),
                ..Default::default()
            },
        )
        .into_inner();
        document.set_shell_provider(shell_provider.clone());
        document.resolve(0.0);
        let node_ids = ChromeNodeIds {
            shell: document
                .get_element_by_id("chrome-shell")
                .ok_or_else(|| String::from("chrome shell node missing"))?,
            address: document
                .get_element_by_id("address")
                .ok_or_else(|| String::from("chrome address input missing"))?,
        };
        let mut chrome = Self {
            document,
            node_ids,
            full_viewport,
            height_css: DEFAULT_CHROME_HEIGHT_CSS,
            shell_provider,
            last_tab_count: tabs.len(),
            last_labels: tabs.iter().map(|t| t.label.clone()).collect(),
        };
        chrome.refresh_layout();
        Ok(chrome)
    }

    pub fn set_viewport(&mut self, full_viewport: Viewport) {
        self.full_viewport = full_viewport;
        self.document
            .set_viewport(Self::chrome_viewport(&self.full_viewport, self.height_css));
        self.refresh_layout();
    }

    pub fn sync_state(&mut self, state: &ChromeViewState) {
        // Rebuild when tab count changes or any label changed
        let labels_changed = state.tabs.len() == self.last_tab_count
            && state
                .tabs
                .iter()
                .zip(self.last_labels.iter())
                .any(|(t, old)| t.label != *old);
        if state.tabs.len() != self.last_tab_count || labels_changed {
            self.last_labels = state.tabs.iter().map(|t| t.label.clone()).collect();
            let html = build_html(&state.address, &state.tabs);
            let mut doc = HtmlDocument::from_html(
                &html,
                DocumentConfig {
                    viewport: Some(Self::chrome_viewport(&self.full_viewport, self.height_css)),
                    ..Default::default()
                },
            )
            .into_inner();
            doc.set_shell_provider(self.shell_provider.clone());
            doc.resolve(0.0);
            if let Some(s) = doc.get_element_by_id("chrome-shell") {
                self.node_ids.shell = s;
            }
            if let Some(a) = doc.get_element_by_id("address") {
                self.node_ids.address = a;
            }
            self.document = doc;
            self.last_tab_count = state.tabs.len();
            self.last_labels = state.tabs.iter().map(|t| t.label.clone()).collect();
            self.refresh_layout();
        } else if self.address_value() != state.address {
            let mut mutator = self.document.mutate();
            mutator.set_attribute(self.node_ids.address, qual_name!("value"), &state.address);
        }
    }

    pub fn height_css(&self) -> f32 {
        self.height_css
    }

    pub fn height_physical(&self) -> u32 {
        ((self.height_css * self.full_viewport.scale()).ceil() as u32).max(1)
    }

    pub fn paint_scene(&mut self) -> RenderScene {
        let viewport = self.document.viewport().clone();
        let (width, height) = viewport.window_size;
        let mut scene = RenderScene::new();
        paint_scene(
            &mut scene,
            &self.document,
            viewport.scale_f64(),
            width,
            height,
            0,
            0,
        );
        scene
    }

    pub fn takes_text_input_focus(&self) -> bool {
        self.document.get_focussed_node_id() == Some(self.node_ids.address)
    }

    pub fn clear_focus(&mut self) {
        self.document.clear_focus();
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) -> Option<ChromeAction> {
        match event {
            UiEvent::PointerDown(event) => {
                let page_x = event.page_x();
                let page_y = event.page_y();
                let Some(hit) = self.document.hit(page_x, page_y) else {
                    self.document.handle_ui_event(UiEvent::PointerDown(event));
                    return None;
                };
                let chain = self.document.node_chain(hit.node_id);
                for (i, node_id) in chain.iter().enumerate() {
                    if let Some(node) = self.document.get_node_mut(*node_id) {
                        if let Some(element) = node.element_data_mut() {
                            let id_q = qual_name!("id");
                            if let Some(attr) = element.attrs.get(&id_q) {
                                if attr.value == "new-tab-btn" {
                                    let is_shift =
                                        event.mods.contains(keyboard_types::Modifiers::SHIFT);
                                    return if is_shift {
                                        Some(ChromeAction::NewWindow)
                                    } else {
                                        Some(ChromeAction::NewTab)
                                    };
                                }
                                if let Some(index_str) = attr.value.strip_prefix("tab-") {
                                    if let Ok(index) = index_str.parse::<usize>() {
                                        return Some(ChromeAction::SwitchTab(index));
                                    }
                                }
                            }
                        }
                    }
                }

                let was_focused = self.takes_text_input_focus();
                self.document.handle_ui_event(UiEvent::PointerDown(event));
                if !was_focused && chain.contains(&self.node_ids.address) {
                    self.select_all_address_text();
                }
                None
            }
            UiEvent::PointerUp(event) => {
                self.document.handle_ui_event(UiEvent::PointerUp(event));
                None
            }
            UiEvent::KeyDown(event) => {
                let focused = self.takes_text_input_focus();
                let submit = is_submit_key(&event);
                if focused && submit {
                    self.clear_focus();
                    return Some(ChromeAction::Navigate);
                }
                self.document.handle_ui_event(UiEvent::KeyDown(event));
                None
            }
            UiEvent::PointerMove(event) => {
                self.document.handle_ui_event(UiEvent::PointerMove(event));
                None
            }
            UiEvent::Wheel(event) => {
                self.document.handle_ui_event(UiEvent::Wheel(event));
                None
            }
            UiEvent::KeyUp(event) => {
                self.document.handle_ui_event(UiEvent::KeyUp(event));
                None
            }
            UiEvent::Ime(event) => {
                self.document.handle_ui_event(UiEvent::Ime(event));
                None
            }
            UiEvent::AppleStandardKeybinding(event) => {
                self.document
                    .handle_ui_event(UiEvent::AppleStandardKeybinding(event));
                None
            }
        }
    }

    pub fn address_value(&self) -> String {
        self.document
            .get_node(self.node_ids.address)
            .and_then(|node| node.element_data())
            .and_then(|element| element.text_input_data())
            .map(|input| input.editor.raw_text().to_string())
            .unwrap_or_default()
    }

    fn refresh_layout(&mut self) {
        self.document.resolve(0.0);
        let measured = self
            .document
            .get_client_bounding_rect(self.node_ids.shell)
            .map(|r| r.height as f32)
            .unwrap_or(DEFAULT_CHROME_HEIGHT_CSS)
            .max(DEFAULT_CHROME_HEIGHT_CSS);
        if (measured - self.height_css).abs() > 0.5 {
            self.height_css = measured;
            self.document
                .set_viewport(Self::chrome_viewport(&self.full_viewport, self.height_css));
            self.document.resolve(0.0);
        }
    }

    fn select_all_address_text(&mut self) {
        self.document
            .with_text_input(self.node_ids.address, |mut driver| driver.select_all());
    }

    fn chrome_viewport(full_viewport: &Viewport, height_css: f32) -> Viewport {
        let mut vp = full_viewport.clone();
        vp.window_size.1 = ((height_css * full_viewport.scale()).ceil() as u32).max(1);
        vp
    }
}

fn is_submit_key(event: &BlitzKeyEvent) -> bool {
    match &event.key {
        Key::Enter => true,
        Key::Character(v) if v == "\n" => true,
        _ => false,
    }
}

// ── Shell provider for the chrome document ────────────────────────────────

pub struct WinitShellProvider {
    window: Arc<Window>,
}

impl WinitShellProvider {
    pub fn new(window: Arc<Window>) -> Self {
        Self { window }
    }
}

fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("clipboard read error: {e}"))
}

fn clipboard_write(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| format!("clipboard write error: {e}"))
}

impl ShellProvider for WinitShellProvider {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
    fn set_cursor(&self, icon: CursorIcon) {
        self.window.set_cursor(Cursor::Icon(icon));
    }
    fn set_window_title(&self, title: String) {
        self.window.set_title(&title);
    }
    fn set_ime_enabled(&self, enabled: bool) {
        self.window.set_ime_allowed(enabled);
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, w: f32, h: f32) {
        self.window
            .set_ime_cursor_area(LogicalPosition::new(x, y), LogicalSize::new(w, h));
    }
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        clipboard_read().map_err(|_| ClipboardError)
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        clipboard_write(text).map_err(|_| ClipboardError)
    }
}
