use std::cell::Cell;
use std::rc::Rc;

use ipc_messages::content::CanvasId;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::invalid_state_error_value;

use super::OffscreenCanvasRenderingContext2D;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#offscreencanvas-context-mode>
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OffscreenCanvasContextMode {
    None,
    Context2D,
    Detached,
}

/// <https://html.spec.whatwg.org/#offscreencanvas>
#[gc_struct]
pub struct OffscreenCanvas {
    /// The canvas id linking this OffscreenCanvas to its placeholder canvas
    /// element's embed site (see `HTMLCanvasElement.transferControlToOffscreen`).
    #[ignore_trace]
    canvas_id: CanvasId,

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas-width>
    #[ignore_trace]
    width: u32,

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas-height>
    #[ignore_trace]
    height: u32,

    /// The OffscreenCanvas object's context mode; `Rc`-shared because the
    /// bindings and the transfer steps run on a clone of the platform object.
    #[ignore_trace]
    context_mode: Rc<Cell<OffscreenCanvasContextMode>>,

    /// The cached OffscreenCanvasRenderingContext2D object returned by
    /// `getContext("2d")`, so a second call returns the same object.
    context_2d: GcCell<Option<JsObject>>,
}

impl OffscreenCanvas {
    pub fn new(
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            canvas_id,
            width,
            height,
            context_mode: Rc::new(Cell::new(OffscreenCanvasContextMode::None)),
            context_2d: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas-width>
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas-height>
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// The canvas id linking this OffscreenCanvas to its placeholder canvas
    /// element's embed site (carried by its transfer data holder).
    pub(crate) fn canvas_id(&self) -> CanvasId {
        self.canvas_id
    }

    /// The OffscreenCanvas object's context mode (read by its transfer steps).
    pub(crate) fn context_mode(&self) -> OffscreenCanvasContextMode {
        self.context_mode.get()
    }

    /// Set the OffscreenCanvas object's context mode (its transfer steps set it
    /// to detached).
    pub(crate) fn set_context_mode(&self, mode: OffscreenCanvasContextMode) {
        self.context_mode.set(mode);
    }

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas-getcontext>
    pub(crate) fn get_context(
        &self,
        context_id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<JsObject>, Types> {
        // Step 1: "If options is not an object, then set options to null."
        // Step 2: "Set options to the result of converting options to a JavaScript value."
        // Note: options are not passed or modeled.
        // Step 3: "Run the steps in the cell of the following table whose column header matches this
        // OffscreenCanvas object's context mode and whose row header matches contextId:"
        match (self.context_mode.get(), context_id) {
            // detached / any: throw an "InvalidStateError" DOMException.
            (OffscreenCanvasContextMode::Detached, _) => Err(invalid_state_error_value(ec)),
            // 2d / "2d": return the same object as was returned the last time.
            (OffscreenCanvasContextMode::Context2D, "2d") => Ok(self.context_2d.borrow(ec).clone()),
            // none / "2d": follow the offscreen 2D context creation algorithm, and return its result.
            (OffscreenCanvasContextMode::None, "2d") => {
                let context = self.create_offscreen_2d_context(ec)?;
                // The cell leaves the OffscreenCanvas with a bound 2d context: cache the object and
                // switch the mode so a later call takes the "2d" row.
                *self.context_2d.borrow_mut(ec) = Some(context.clone());
                self.context_mode.set(OffscreenCanvasContextMode::Context2D);
                Ok(Some(context))
            }
            // Every other cell: return null.
            _ => Ok(None),
        }
    }

    /// <https://html.spec.whatwg.org/#offscreen-2d-context-creation-algorithm>
    fn create_offscreen_2d_context(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: "If the algorithm was passed some arguments, let arg be the first such argument. Otherwise, let arg be undefined."
        // Step 2: "Let settings be the result of converting arg to the dictionary type CanvasRenderingContext2DSettings. (This can throw an exception.)"
        // Note: options are not passed or modeled.
        // Step 3: "Let context be a new OffscreenCanvasRenderingContext2D object."
        let context = create_interface_instance::<Types, OffscreenCanvasRenderingContext2D>(
            OffscreenCanvasRenderingContext2D::new(self.canvas_id, self.width, self.height),
            ec,
        )?;
        // Step 4: "Set context's associated OffscreenCanvas object to target."
        // Note: the context does not hold the OffscreenCanvas object; its canvas member is not exposed.
        // Step 5: "Run the canvas settings output bitmap initialization algorithm, given context and settings."
        // Note: not implemented.
        // Step 6: "Set context's output bitmap to a newly created bitmap with the dimensions specified by the width and height attributes of target, and set target's bitmap to the same bitmap (so that they are shared)."
        // Step 7: "If context's alpha flag is set to true, initialize all the pixels of context's output bitmap to transparent black. Otherwise, initialize the pixels to opaque black."
        // Note: the output bitmap is realized as the graphics-process canvas slot, shared with the context through the canvas id and starting transparent black.
        // Step 8: "Return context."
        Ok(context)
    }
}
