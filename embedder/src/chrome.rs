use anyrender::Scene as RenderScene;
use blitz_dom::{qual_name, BaseDocument, Document, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_paint::paint_scene;
use blitz_traits::events::{BlitzKeyEvent, UiEvent};
use blitz_traits::shell::Viewport;
use keyboard_types::Key;

const DEFAULT_CHROME_HEIGHT_CSS: f32 = 40.0;
const CHROME_HTML: &str = r#"
<!DOCTYPE html>
<html>
  <head>
    <style>
      :root {
                color-scheme: light;
      }

      html,
      body {
        margin: 0;
        padding: 0;
      }

      body {
                background: rgba(249, 248, 244, 0.98);
                color: #2f3134;
            font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
            font-size: 11px;
            line-height: 1.25;
      }

      #chrome-shell {
        box-sizing: border-box;
                display: block;
                min-height: 40px;
            padding: 4px 8px;
                background: linear-gradient(180deg, rgba(252, 251, 247, 0.99), rgba(247, 245, 240, 0.97));
                border-bottom: 1px solid rgba(47, 49, 52, 0.14);
      }

     #address {
    box-sizing: border-box;
    display: block;
    width: 100%;
    
    /* 1. Set an explicit height to create the container size */
    height: 42px; /* Adjust this slightly if you want the bar taller/shorter */
    
    /* 2. Remove top/bottom padding, keep horizontal padding */
    padding: 0 11px; 
    
    /* 3. Reset line-height so the browser handles vertical centering */
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
}

            #address:focus {
                outline: none;
                border-color: rgba(78, 109, 142, 0.42);
            }

            @media (max-width: 560px) {
                #chrome-shell {
                    padding: 4px 6px;
                }

                #address {
        padding: 9px 10px;
        border-radius: 16px;
    }
            }
    </style>
  </head>
  <body>
    <div id="chrome-shell">
      <input id="address" type="text" value="" spellcheck="false" autocomplete="off" />
    </div>
  </body>
</html>
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeAction {
    Navigate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChromeViewState {
    pub address: String,
}

#[derive(Clone, Copy)]
struct ChromeNodeIds {
    shell: usize,
    address: usize,
}

pub struct ChromeUi {
    document: BaseDocument,
    node_ids: ChromeNodeIds,
    full_viewport: Viewport,
    height_css: f32,
}

impl ChromeUi {
    pub fn new(full_viewport: Viewport) -> Result<Self, String> {
        let mut document = HtmlDocument::from_html(
            CHROME_HTML,
            DocumentConfig {
                viewport: Some(Self::chrome_viewport(&full_viewport, DEFAULT_CHROME_HEIGHT_CSS)),
                ..Default::default()
            },
        )
        .into_inner();
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
        self.set_address_value(&state.address);
        self.refresh_layout();
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
                let was_address_focused = self.takes_text_input_focus();
                let page_x = event.page_x();
                let page_y = event.page_y();
                self.document.handle_ui_event(UiEvent::PointerDown(event));
                if !was_address_focused && self.is_address_hit(page_x, page_y) {
                    self.select_all_address_text();
                }
                None
            }
            UiEvent::PointerUp(event) => {
                self.document.handle_ui_event(UiEvent::PointerUp(event));
                None
            }
            UiEvent::KeyDown(event) => self.handle_key_down(event),
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

    fn handle_key_down(&mut self, event: BlitzKeyEvent) -> Option<ChromeAction> {
        if self.takes_text_input_focus() && is_submit_key(&event) {
            self.clear_focus();
            return Some(ChromeAction::Navigate);
        }
        self.document.handle_ui_event(UiEvent::KeyDown(event));
        None
    }

    fn set_address_value(&mut self, value: &str) {
        if self.address_value() == value {
            return;
        }

        let mut mutator = self.document.mutate();
        mutator.set_attribute(self.node_ids.address, qual_name!("value"), value);
    }

    fn refresh_layout(&mut self) {
        self.document.resolve(0.0);
        let measured_height = self
            .document
            .get_client_bounding_rect(self.node_ids.shell)
            .map(|rect| rect.height as f32)
            .unwrap_or(DEFAULT_CHROME_HEIGHT_CSS)
            .max(DEFAULT_CHROME_HEIGHT_CSS);
        if (measured_height - self.height_css).abs() > 0.5 {
            self.height_css = measured_height;
            self.document
                .set_viewport(Self::chrome_viewport(&self.full_viewport, self.height_css));
            self.document.resolve(0.0);
        }
    }

    fn is_address_hit(&self, x: f32, y: f32) -> bool {
        let Some(hit) = self.document.hit(x, y) else {
            return false;
        };
        self.document.node_chain(hit.node_id).contains(&self.node_ids.address)
    }

    fn select_all_address_text(&mut self) {
        self.document
            .with_text_input(self.node_ids.address, |mut driver| driver.select_all());
    }

    fn chrome_viewport(full_viewport: &Viewport, height_css: f32) -> Viewport {
        let mut viewport = full_viewport.clone();
        viewport.window_size.1 = ((height_css * full_viewport.scale()).ceil() as u32).max(1);
        viewport
    }
}

fn is_submit_key(event: &BlitzKeyEvent) -> bool {
    match &event.key {
        Key::Enter => true,
        Key::Character(value) => value == "\n",
        _ => false,
    }
}