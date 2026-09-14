use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use anyrender::{PaintScene, Scene};
use ipc_messages::content::{CanvasId, RecordedScene, serialize_scene_to_vec};
use ipc_messages::graphics::GraphicsCommand;
use js_engine::{Completion, ExecutionContext};
use kurbo::{Affine, Rect, Shape};
use log::error;
use peniko::{Color, Fill};

use crate::js::Types;
use crate::js::platform_objects::with_global_scope;

/// The output bitmap and drawing state of a 2D rendering context, shared by
/// the `CanvasRenderingContext2D` and `OffscreenCanvasRenderingContext2D`
/// interfaces (the mixins both interfaces include operate on it).  Each
/// interface struct owns one; the mixin traits below implement the spec's
/// member algorithms over the [`CanvasContext2D`] accessor.
///
/// The bindings take a clone of the platform object out of the engine before
/// running a member, so the mutable state is `Rc`-shared and every clone
/// draws into the same output bitmap.
#[derive(Clone)]
pub(crate) struct RenderingContext2D {
    /// The canvas id of the associated canvas (its embed site).
    canvas_id: CanvasId,

    /// The output bitmap dimensions in CSS pixels.
    width: u32,
    height: u32,

    /// The accumulated anyrender drawing commands of this context's output
    /// bitmap.
    scene: Rc<RefCell<Scene>>,

    /// <https://html.spec.whatwg.org/#dom-context-2d-fillstyle>
    fill_style: Rc<RefCell<Color>>,

    /// The last-set string form of the fill style (returned by the getter).
    fill_style_string: Rc<RefCell<String>>,

    /// The stack of saved drawing states (a copy per `save()`).
    drawing_state_stack: Rc<RefCell<Vec<DrawingState>>>,

    /// The context lost boolean (always false: context loss is not modeled).
    context_lost: Rc<Cell<bool>>,
}

/// The tracked subset of a drawing state (only the fill style so far).
#[derive(Clone)]
struct DrawingState {
    fill_style: Color,
    fill_style_string: String,
}

impl RenderingContext2D {
    pub(crate) fn new(canvas_id: CanvasId, width: u32, height: u32) -> Self {
        Self {
            canvas_id,
            width,
            height,
            scene: Rc::new(RefCell::new(Scene::new())),
            fill_style: Rc::new(RefCell::new(Color::BLACK)),
            fill_style_string: Rc::new(RefCell::new(String::from("#000000"))),
            drawing_state_stack: Rc::new(RefCell::new(Vec::new())),
            context_lost: Rc::new(Cell::new(false)),
        }
    }

    /// <https://html.spec.whatwg.org/#reset-the-rendering-context-to-its-default-state>
    fn reset_the_rendering_context_to_its_default_state(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: "Clear canvas's bitmap to transparent black."
        *self.scene.borrow_mut() = Scene::new();
        // Step 2: "Empty the list of subpaths in the context's current default path."
        // Note: The current default path is not tracked.
        // Step 3: "Clear the context's drawing state stack."
        self.drawing_state_stack.borrow_mut().clear();
        // Step 4: "Reset everything that drawing state consists of to their initial values."
        *self.fill_style.borrow_mut() = Color::BLACK;
        *self.fill_style_string.borrow_mut() = String::from("#000000");
        self.commit(ec)
    }

    /// Send this context's accumulated scene to the graphics process so the
    /// canvas's embed site re-composes with the latest drawing.  The graphics
    /// channel is the current realm's: a worker's for an OffscreenCanvas
    /// rendered on a worker, the window's for a canvas rendered on the window.
    fn commit(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let scene = self.scene.borrow().clone();
        let recorded = RecordedScene::from_scene_without_fonts(scene);
        let scene_bytes = serialize_scene_to_vec(&recorded).map_err(|error| {
            ec.new_type_error(&format!("failed to serialize canvas scene: {error}"))
        })?;
        let region = ipc::IpcSharedRegion::from_bytes(&scene_bytes);
        let mut shmem_map = HashMap::new();
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

/// Access to the shared [`RenderingContext2D`] behind a 2D rendering context
/// platform object.  Each interface struct implements this; the mixin traits
/// below are blanket implemented for every accessor, so a member's algorithm
/// is written once for both interfaces.
pub(crate) trait CanvasContext2D {
    fn rendering_context_2d(&self) -> &RenderingContext2D;
}

/// <https://html.spec.whatwg.org/#canvasfillstrokestyles>
pub(crate) trait CanvasFillStrokeStyles: CanvasContext2D {
    /// <https://html.spec.whatwg.org/#dom-context-2d-fillstyle>
    fn set_fill_style(&self, value: &str) {
        let context = self.rendering_context_2d();
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
        *context.fill_style.borrow_mut() = color;
        *context.fill_style_string.borrow_mut() = value.to_owned();

        // Step 1.5: "Return."
        // Step 2: "If the given value is a CanvasPattern object that is marked as not origin-clean, then set
        // this's origin-clean flag to false."
        // Step 3: "Set this's fill style to the given value."
        // Note: Not yet implemented (the binding converts a DOMString; only the
        // string form is modeled, so CanvasPattern and CanvasGradient values
        // do not reach this method).
    }

    /// <https://html.spec.whatwg.org/#dom-context-2d-fillstyle>
    fn fill_style_value(&self) -> String {
        let context = self.rendering_context_2d();
        // Step 1: "If this's fill style is a CSS color, then return the serialization of that color with HTML-compatible serialization requested."
        // Note: The stored string form is returned without re-serialization.
        // Step 2: "Return this's fill style."
        context.fill_style_string.borrow().clone()
    }
}

impl<T: CanvasContext2D> CanvasFillStrokeStyles for T {}

/// <https://html.spec.whatwg.org/#canvasstate>
pub(crate) trait CanvasState: CanvasContext2D {
    /// <https://html.spec.whatwg.org/#dom-context-2d-save>
    fn save(&self) {
        let context = self.rendering_context_2d();
        // "The save() method steps are to push a copy of the current drawing
        // state onto the drawing state stack."
        let state = DrawingState {
            fill_style: *context.fill_style.borrow(),
            fill_style_string: context.fill_style_string.borrow().clone(),
        };
        context.drawing_state_stack.borrow_mut().push(state);
    }

    /// <https://html.spec.whatwg.org/#dom-context-2d-restore>
    fn restore(&self) {
        let context = self.rendering_context_2d();
        // "The restore() method steps are to pop the top entry in the drawing
        // state stack, and reset the drawing state it describes. If there is no
        // saved state, then the method must do nothing."
        if let Some(state) = context.drawing_state_stack.borrow_mut().pop() {
            *context.fill_style.borrow_mut() = state.fill_style;
            *context.fill_style_string.borrow_mut() = state.fill_style_string;
        }
    }

    /// <https://html.spec.whatwg.org/#dom-context-2d-reset>
    fn reset(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        // "The reset() method steps are to reset the rendering context to its
        // default state."
        self.rendering_context_2d()
            .reset_the_rendering_context_to_its_default_state(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-context-2d-iscontextlost>
    fn is_context_lost(&self) -> bool {
        // "The isContextLost() method steps are to return this's context lost."
        // Note: The context lost boolean is only ever initialized to false;
        // the context lost steps that would set it are not implemented.
        self.rendering_context_2d().context_lost.get()
    }
}

impl<T: CanvasContext2D> CanvasState for T {}

/// <https://html.spec.whatwg.org/#canvasrect>
pub(crate) trait CanvasRect: CanvasContext2D {
    /// <https://html.spec.whatwg.org/#dom-context-2d-fillrect>
    fn fill_rect(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let context = self.rendering_context_2d();
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
        let fill_style = *context.fill_style.borrow();
        context.scene.borrow_mut().fill(
            Fill::NonZero,
            Affine::IDENTITY,
            fill_style,
            None,
            &rect.to_path(0.1),
        );
        context.commit(ec)
    }

    /// <https://html.spec.whatwg.org/#dom-context-2d-clearrect>
    fn clear_rect(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let context = self.rendering_context_2d();
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
        let rect = Rect::new(x, y, x + width, y + height);
        context.scene.borrow_mut().fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::TRANSPARENT,
            None,
            &rect.to_path(0.1),
        );
        context.commit(ec)
    }
}

impl<T: CanvasContext2D> CanvasRect for T {}

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
