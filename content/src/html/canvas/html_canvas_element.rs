use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blitz_dom::BaseDocument;
use ipc_messages::content::{CanvasId, WebviewId};
use ipc_messages::graphics::GraphicsCommand;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use log::error;

use crate::html::HTMLElement;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::invalid_state_error_value;

use super::{CanvasRenderingContext2D, OffscreenCanvas};

type JsObject = <Types as JsTypes>::JsObject;

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
    context_2d: GcCell<Option<JsObject>>,
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

    fn width_attribute(&self) -> u32 {
        // The width attribute defaults to 300.
        self.html_element
            .element
            .get_attribute("width")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(300)
    }

    fn height_attribute(&self) -> u32 {
        // The height attribute defaults to 150.
        self.html_element
            .element
            .get_attribute("height")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(150)
    }

    /// <https://html.spec.whatwg.org/#dom-canvas-getcontext>
    pub(crate) fn get_context(
        &self,
        context_id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<JsObject>, Types> {
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
    ) -> Completion<JsObject, Types> {
        // Step 1: "Let settings be the result of converting options to the dictionary type
        // CanvasRenderingContext2DSettings. (This can throw an exception.)"
        // Note: options are not modeled.
        // Step 2: "Let context be a new CanvasRenderingContext2D object."
        let width = self.width_attribute();
        let height = self.height_attribute();
        let canvas_id = CanvasId::new();
        let context = create_interface_instance::<Types, CanvasRenderingContext2D>(
            CanvasRenderingContext2D::new(canvas_id, width, height, self.html_element.clone()),
            ec,
        )?;
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
    ) -> Completion<JsObject, Types> {
        // Step 1: "If this canvas element's context mode is not set to none, throw an
        // "InvalidStateError" DOMException."
        if self.context_mode.get() != CanvasContextMode::None {
            return Err(invalid_state_error_value(ec));
        }

        // Step 2: "Let offscreenCanvas be a new OffscreenCanvas object with its
        // width and height equal to the values of the width and height content
        // attributes of this canvas element."
        let width = self.width_attribute();
        let height = self.height_attribute();
        let canvas_id = CanvasId::new();
        let offscreen_object = create_interface_instance::<Types, OffscreenCanvas>(
            OffscreenCanvas::new(canvas_id, width, height, ec),
            ec,
        )?;

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
        Ok(offscreen_object)
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
