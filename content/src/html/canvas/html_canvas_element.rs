use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blitz_dom::BaseDocument;
use ipc_messages::content::{CanvasId, WebviewId};
use ipc_messages::graphics::GraphicsCommand;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, gc_struct};
use log::error;

use crate::html::HTMLElement;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::invalid_state_error_value;

use super::{CanvasRenderingContext2D, OffscreenCanvas};

/// <https://html.spec.whatwg.org/#concept-canvas-context-mode>
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasContextMode {
    None,
    Context2D,
    Placeholder,
}

/// <https://html.spec.whatwg.org/#htmlcanvaselement>
#[gc_struct]
pub struct HTMLCanvasElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,

    /// The canvas's canvas context mode; `Rc`-shared because the bindings run a
    /// member on a clone of the platform object.
    #[ignore_trace]
    context_mode: Rc<Cell<CanvasContextMode>>,

    /// The `CanvasRenderingContext2D` returned by a previous
    /// `getContext("2d")` call, so subsequent calls return the same object.
    context_2d: GcCell<Option<CanvasRenderingContext2D>>,
}

impl HTMLCanvasElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
            context_mode: Rc::new(Cell::new(CanvasContextMode::None)),
            context_2d: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-width>
    pub(crate) fn width(&self) -> u32 {
        self.html_element
            .element
            .get_attribute("width")
            .as_deref()
            .and_then(parse_non_negative_integer)
            .unwrap_or(300)
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-height>
    pub(crate) fn height(&self) -> u32 {
        self.html_element
            .element
            .get_attribute("height")
            .as_deref()
            .and_then(parse_non_negative_integer)
            .unwrap_or(150)
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-width>
    pub(crate) fn set_width(
        &self,
        value: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        self.set_dimension("width", value, ec)
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-height>
    pub(crate) fn set_height(
        &self,
        value: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        self.set_dimension("height", value, ec)
    }

    fn set_dimension(
        &self,
        name: &str,
        value: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // "When setting the value of the width or height attribute, if the context mode of the
        // canvas element is set to placeholder, the user agent must throw an "InvalidStateError"
        // DOMException and leave the attribute's value unchanged."
        if self.context_mode.get() == CanvasContextMode::Placeholder {
            return Err(invalid_state_error_value(ec));
        }
        self.html_element
            .element
            .set_attribute(name, &value.to_string());
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-getcontext>
    pub(crate) fn get_context(
        &self,
        context_id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<CanvasRenderingContext2D>, Types> {
        // Step 1: "If options is not an object, then set options to null."
        // Step 2: "Set options to the result of converting options to a JavaScript value."
        // Note: options are not passed or modeled.
        // Step 3: "Run the steps in the cell of the following table whose column header matches this
        // canvas element's canvas context mode and whose row header matches contextId:"
        match (self.context_mode.get(), context_id) {
            // none / "2d": follow the 2D context creation algorithm, and return its result.
            (CanvasContextMode::None, "2d") => {
                let context = self.create_2d_context(ec)?;
                // The cell leaves the element with a bound 2d context: cache the object and switch
                // the mode so a later call takes the "2d" row.
                *self.context_2d.borrow_mut(ec) = Some(context.clone());
                self.context_mode.set(CanvasContextMode::Context2D);
                Ok(Some(context))
            }
            // 2d / "2d": return the same object as was returned the last time.
            (CanvasContextMode::Context2D, "2d") => Ok(self.context_2d.borrow(ec).clone()),
            // placeholder / any: throw an "InvalidStateError" DOMException.
            (CanvasContextMode::Placeholder, _) => Err(invalid_state_error_value(ec)),
            // An unsupported value in the "2d" row and every other cell: return null.
            _ => Ok(None),
        }
    }

    /// <https://html.spec.whatwg.org/#2d-context-creation-algorithm>
    fn create_2d_context(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<CanvasRenderingContext2D, Types> {
        // Step 1: "Let settings be the result of converting options to the dictionary type
        // CanvasRenderingContext2DSettings. (This can throw an exception.)"
        // Note: options are not modeled.
        // Step 2: "Let context be a new CanvasRenderingContext2D object."
        let width = self.width();
        let height = self.height();
        let canvas_id = CanvasId::new();
        let context_object = create_interface_instance::<Types, CanvasRenderingContext2D>(
            CanvasRenderingContext2D::new(canvas_id, width, height, self.html_element.clone()),
            ec,
        )?;
        // The platform data is cloned back out of the created object; its
        // reflector was set by `create_interface_instance`, so the context the
        // element caches resolves to this same object.
        let context = ec
            .with_object_any(&context_object)
            .and_then(|data| data.downcast_ref::<CanvasRenderingContext2D>().cloned())
            .ok_or_else(|| {
                ec.new_type_error("CanvasRenderingContext2D object has no platform data")
            })?;

        // Step 3: "Initialize context's canvas attribute to point to target."
        //   The context holds the target element passed to its constructor.
        // Step 4: "Set context's output bitmap to the same bitmap as target's bitmap (so that they are shared)."
        // Step 5: "Set bitmap dimensions to the numeric values of target's width and height content attributes."
        // Note: The output bitmap is realized as the graphics-process canvas
        // slot: the canvas id is registered with the document's canvas
        // registry (so the element gets an embed site) and with the graphics
        // process (so the context's committed scenes route to the owning
        // webview's compositor). The bitmap dimensions are the width/height
        // content attributes passed to the context constructor.
        self.register_canvas_with_graphics(canvas_id, ec)?;
        // Step 6: "Run the canvas settings output bitmap initialization algorithm, given context and settings."
        // Note: not implemented.
        // Step 7: "Return context."
        Ok(context)
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-transfercontroltooffscreen>
    pub(crate) fn transfer_control_to_offscreen(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<OffscreenCanvas, Types> {
        // Step 1: "If this canvas element's context mode is not set to none, throw an
        // "InvalidStateError" DOMException."
        if self.context_mode.get() != CanvasContextMode::None {
            return Err(invalid_state_error_value(ec));
        }

        // Step 2: "Let offscreenCanvas be a new OffscreenCanvas object with its
        // width and height equal to the values of the width and height content
        // attributes of this canvas element."
        let width = self.width();
        let height = self.height();
        let canvas_id = CanvasId::new();
        let offscreen_object = create_interface_instance::<Types, OffscreenCanvas>(
            OffscreenCanvas::new(canvas_id, width, height, ec),
            ec,
        )?;
        // The platform data is cloned back out of the created object; its
        // reflector was set by `create_interface_instance`.
        let offscreen = ec
            .with_object_any(&offscreen_object)
            .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned())
            .ok_or_else(|| ec.new_type_error("OffscreenCanvas object has no platform data"))?;

        // Step 3: "Set the offscreenCanvas's placeholder canvas element to a
        // weak reference to this canvas element."
        // Step 4: "Set this canvas element's context mode to placeholder."
        // Note: The placeholder linkage is the CanvasId registered for this
        // element's node (the graphics process fills the element's embed site
        // from the OffscreenCanvas's committed scenes).
        self.register_canvas_with_graphics(canvas_id, ec)?;
        self.context_mode.set(CanvasContextMode::Placeholder);

        // Step 5: "Set the offscreenCanvas's inherited language to the language
        // of this canvas element."
        // Step 6: "Set the offscreenCanvas's inherited direction to the
        // directionality of this canvas element."
        // Note: inherited language and direction are not modeled.
        // Step 7: "Return offscreenCanvas."
        Ok(offscreen)
    }

    fn register_canvas_with_graphics(
        &self,
        canvas_id: CanvasId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // The spec has no such step: the canvas id is this implementation's
        // realization of the canvas's shared output bitmap and its
        // placeholder/embed-site linkage.  Register it with the document's
        // canvas registry (so the element gets an embed site) and with the
        // graphics process (so the context's committed scenes route to the
        // owning webview's compositor).
        let node_id = self.html_element.element.node.node_id;
        with_global_scope(ec, |global_scope, ec| {
            let document_id = global_scope
                .document_id()
                .ok_or_else(|| ec.new_type_error("canvas element has no associated document"))?;
            let registry = global_scope
                .canvas_registry()
                .ok_or_else(|| ec.new_type_error("no canvas registry"))?;
            registry
                .borrow_mut()
                .insert((document_id, node_id), canvas_id);
            if let Some(graphics_sender) = global_scope.graphics_sender() {
                let command =
                    GraphicsCommand::RegisterCanvas {
                        webview_id: WebviewId(global_scope.source_navigable_id().ok_or_else(
                            || ec.new_type_error("canvas element has no navigable id"),
                        )?),
                        canvas_id,
                    };
                if let Err(error) = graphics_sender.send(command) {
                    error!("failed to register canvas with graphics: {error}");
                }
            }
            // A canvas embed site appeared without a DOM mutation; mark the
            // document dirty so the next update-the-rendering rebuilds the
            // frame composition and includes it.
            global_scope.mark_document_dirty();
            Ok(())
        })
    }
}

/// <https://html.spec.whatwg.org/#rules-for-parsing-non-negative-integers>
fn parse_non_negative_integer(value: &str) -> Option<u32> {
    let mut characters = value.chars().peekable();
    // "Skip ASCII whitespace within input given position."
    while matches!(characters.peek(), Some(' ' | '\t' | '\n' | '\u{0C}' | '\r')) {
        characters.next();
    }
    // "If the character ... is a U+002B PLUS SIGN ... Otherwise, if the character is a U+002D
    // HYPHEN-MINUS character (-), then set sign to \"negative\" ..."
    let negative = match characters.peek() {
        Some('+') => {
            characters.next();
            false
        }
        Some('-') => {
            characters.next();
            true
        }
        _ => false,
    };
    // "If the character pointed to by position is not an ASCII digit, then return an error."
    if !characters.peek().is_some_and(char::is_ascii_digit) {
        return None;
    }
    // "Collect a sequence of characters that are ASCII digits, and interpret the resulting sequence
    // as a base-ten integer. Let value be that number."
    let mut parsed = 0u64;
    while let Some(character) = characters.peek() {
        let Some(digit) = character.to_digit(10) else {
            break;
        };
        parsed = parsed.saturating_mul(10).saturating_add(u64::from(digit));
        characters.next();
    }
    // "If sign is \"negative\", negate value. ... If value is less than zero, return an error."
    if negative && parsed != 0 {
        return None;
    }
    // The IDL attributes are `unsigned long`; the parsed integer converts modulo 2^32.
    Some((parsed % (1 << 32)) as u32)
}
