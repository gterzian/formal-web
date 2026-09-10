use std::cell::RefCell;
use std::rc::Rc;

use anyrender::{PaintScene, Scene};
use ipc_messages::content::{CanvasId, RecordedScene, serialize_scene_to_vec};
use ipc_messages::graphics::GraphicsCommand;
use js_engine::{Completion, ExecutionContext, gc_struct};
use kurbo::{Affine, Rect, Shape};
use log::error;
use peniko::{Color, Fill};

use crate::js::Types;
use crate::js::platform_objects::with_global_scope;

/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvasrenderingcontext2d>
#[gc_struct]
pub struct OffscreenCanvasRenderingContext2D {
    /// The canvas id of the associated OffscreenCanvas (its placeholder
    /// canvas element's embed site).
    #[ignore_trace]
    canvas_id: CanvasId,

    /// The output bitmap width in CSS pixels, copied from the associated
    /// OffscreenCanvas at creation.
    #[ignore_trace]
    width: u32,

    /// The output bitmap height in CSS pixels, copied from the associated
    /// OffscreenCanvas at creation.
    #[ignore_trace]
    height: u32,

    /// The accumulated anyrender drawing commands of this context's output
    /// bitmap. Each draw appends to it and commits the full scene to the
    /// graphics process. `Rc<RefCell<..>>` because a binding call clones the
    /// platform object out of the object registry; a plain `RefCell` would
    /// be deep-copied and every draw discarded.
    #[ignore_trace]
    scene: Rc<RefCell<Scene>>,

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-fillstyle>
    #[ignore_trace]
    fill_style: Rc<RefCell<Color>>,

    /// The last-set string form of fillStyle (returned by the getter).
    #[ignore_trace]
    fill_style_string: Rc<RefCell<String>>,
}

impl OffscreenCanvasRenderingContext2D {
    pub fn new(
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            canvas_id,
            width,
            height,
            scene: Rc::new(RefCell::new(Scene::new())),
            fill_style: Rc::new(RefCell::new(Color::BLACK)),
            fill_style_string: Rc::new(RefCell::new(String::from("#000000"))),
        }
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-fillstyle>
    pub(crate) fn set_fill_style(&self, value: &str) {
        // Step 1: "If the given value is a string:"
        // Step 1.1: "Let context be this's canvas attribute's value, if that is an element;
        // otherwise null."
        // Step 1.2: "Let parsedValue be the result of parsing the given value with context if non-null."
        // Step 1.3: "If parsedValue is failure, then return."
        // Note: A minimal CSS color parser (hex and a few named colors); full
        // CSS color parsing is not implemented.
        let Some(color) = parse_css_color(value) else {
            return;
        };
        // Step 1.4: "Set this's fill style to parsedValue."
        *self.fill_style.borrow_mut() = color;
        *self.fill_style_string.borrow_mut() = value.to_owned();

        // Step 1.5: "Return."
        // Step 2: "If the given value is a CanvasPattern object that is marked as not origin-clean, then set
        // this's origin-clean flag to false."
        // Step 3: "Set this's fill style to the given value."
        // TODO: Not yet implemented (the binding converts a DOMString; only the
        // string form is modeled, so CanvasPattern and CanvasGradient values
        // do not reach this method).
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-fillstyle>
    pub(crate) fn fill_style_value(&self) -> String {
        // Step 1: "If this's fill style is a CSS color, then return the serialization of that color with HTML-compatible serialization requested."
        // Note: The stored string form is returned without re-serialization.
        // Step 2: "Return this's fill style."
        self.fill_style_string.borrow().clone()
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-fillrect>
    pub(crate) fn fill_rect(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: "If any of the arguments are infinite or NaN, then return."
        if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
            return Ok(());
        }
        // Step 2: "If either w or h are zero, then return."
        if width == 0.0 || height == 0.0 {
            return Ok(());
        }
        // Step 3: "Paint the specified rectangular area using this's fill style."
        let rect = Rect::new(x, y, x + width, y + height);
        let fill_style = *self.fill_style.borrow();
        self.scene.borrow_mut().fill(
            Fill::NonZero,
            Affine::IDENTITY,
            fill_style,
            None,
            &rect.to_path(0.1),
        );
        self.commit(ec)
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-clearrect>
    pub(crate) fn clear_rect(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: "If any of the arguments are infinite or NaN, then return."
        if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
            return Ok(());
        }
        // Step 2: "Let pixels be the set of pixels in the specified rectangle
        // that also intersect the current clipping region."
        // Step 3: "Clear the pixels in pixels to a transparent black, erasing
        // any previous image."
        // Note: Clearing is modeled as a transparent fill; the current
        // clipping region is not tracked.
        if width == 0.0 || height == 0.0 {
            return Ok(());
        }
        let rect = Rect::new(x, y, x + width, y + height);
        self.scene.borrow_mut().fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::TRANSPARENT,
            None,
            &rect.to_path(0.1),
        );
        self.commit(ec)
    }

    /// Send this context's accumulated scene to the graphics process so the
    /// placeholder canvas element's layer re-composes with the latest drawing.
    fn commit(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let scene = self.scene.borrow().clone();
        let recorded = RecordedScene::from_scene_without_fonts(scene);
        let scene_bytes = serialize_scene_to_vec(&recorded).map_err(|error| {
            ec.new_type_error(&format!("failed to serialize canvas scene: {error}"))
        })?;
        let region = ipc::IpcSharedRegion::from_bytes(&scene_bytes);
        let mut shmem_map = std::collections::HashMap::new();
        shmem_map.insert(0usize, region);
        let command = GraphicsCommand::CanvasPaint {
            canvas_id: self.canvas_id,
            width: self.width,
            height: self.height,
            scene_shmem_key: 0,
        };
        with_global_scope(ec, |global_scope, _ec| {
            if let Some(graphics_sender) = global_scope.graphics_sender()
                && let Err(error) = graphics_sender.send_with_shmem_map(command, shmem_map.clone())
            {
                error!("failed to send canvas paint to graphics: {error}");
            }
            Ok(())
        })
    }
}

/// Parse a minimal subset of CSS colors into a peniko color: `#rgb`,
/// `#rrggbb`, `#rrggbbaa`, and the named colors `transparent`, `black`,
/// `white`, `red`, `green`, `blue`.
fn parse_css_color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "transparent" => Some(Color::TRANSPARENT),
        "black" => Some(Color::BLACK),
        "white" => Some(Color::WHITE),
        "red" => Some(Color::from_rgb8(255, 0, 0)),
        "green" => Some(Color::from_rgb8(0, 128, 0)),
        "blue" => Some(Color::from_rgb8(0, 0, 255)),
        _ => {
            let hex = value.strip_prefix('#')?;
            let expand = |channel: u32| ((channel << 4) | channel) as u8;
            match hex.len() {
                3 => {
                    let channels = u32::from_str_radix(hex, 16).ok()?;
                    Some(Color::from_rgb8(
                        expand((channels >> 8) & 0xF),
                        expand((channels >> 4) & 0xF),
                        expand(channels & 0xF),
                    ))
                }
                6 => {
                    let channels = u32::from_str_radix(hex, 16).ok()?;
                    Some(Color::from_rgb8(
                        ((channels >> 16) & 0xFF) as u8,
                        ((channels >> 8) & 0xFF) as u8,
                        (channels & 0xFF) as u8,
                    ))
                }
                8 => {
                    let channels = u32::from_str_radix(hex, 16).ok()?;
                    Some(Color::from_rgba8(
                        ((channels >> 24) & 0xFF) as u8,
                        ((channels >> 16) & 0xFF) as u8,
                        ((channels >> 8) & 0xFF) as u8,
                        (channels & 0xFF) as u8,
                    ))
                }
                _ => None,
            }
        }
    }
}
