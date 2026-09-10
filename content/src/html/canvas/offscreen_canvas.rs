use ipc_messages::content::CanvasId;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{ExecutionContext, JsTypes, gc_struct};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

use super::OffscreenCanvasRenderingContext2D;

/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvas>
#[gc_struct]
pub struct OffscreenCanvas {
    /// The canvas id linking this OffscreenCanvas to its placeholder canvas
    /// element's embed site (see `HTMLCanvasElement.transferControlToOffscreen`).
    #[ignore_trace]
    canvas_id: CanvasId,

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-offscreencanvas-width>
    #[ignore_trace]
    width: u32,

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-offscreencanvas-height>
    #[ignore_trace]
    height: u32,

    /// The cached OffscreenCanvasRenderingContext2D object returned by
    /// `getContext("2d")`, so a second call returns the same object.
    context_2d: GcCell<Option<<Types as JsTypes>::JsObject>>,
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
            context_2d: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-offscreencanvas-width>
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-offscreencanvas-height>
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// The canvas id linking this OffscreenCanvas to its placeholder canvas
    /// element's embed site (carried by its transfer data holder).
    pub(crate) fn canvas_id(&self) -> CanvasId {
        self.canvas_id
    }

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-offscreencanvas-getcontext>
    pub(crate) fn get_context(
        &self,
        context_id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> js_engine::Completion<Option<<Types as JsTypes>::JsObject>, Types> {
        // Step 1: "If options is not an object, then set options to null."
        // Step 2: "Set options to the result of converting options to a JavaScript value."
        // TODO: Not yet implemented (options are not passed or used; the
        // "2d" context ignores them).
        // Step 3: "Run the steps in the cell of the following table whose column header matches this
        // OffscreenCanvas object's context mode and whose row header matches contextId:"
        // Note: The context mode is the presence of the cached context object:
        // none (absent) with contextId "2d" returns the cached object, any
        // other contextId returns null; a detached OffscreenCanvas throwing
        // InvalidStateError is not modeled.
        if context_id != "2d" {
            return Ok(None);
        }
        if let Some(existing) = self.context_2d.borrow(ec).clone() {
            return Ok(Some(existing));
        }
        let context_object = create_interface_instance::<Types, OffscreenCanvasRenderingContext2D>(
            OffscreenCanvasRenderingContext2D::new(self.canvas_id, self.width, self.height, ec),
            ec,
        )?;
        *self.context_2d.borrow_mut(ec) = Some(context_object.clone());
        Ok(Some(context_object))
    }
}
