use std::cell::Cell;
use std::rc::Rc;

use ipc_messages::content::{CanvasId, WebviewId};
use ipc_messages::graphics::GraphicsCommand;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use log::error;

use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
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
    /// The JS object implementing this interface, set by the Web IDL layer.
    pub(crate) reflector: Option<JsObject>,

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
    context_2d: GcCell<Option<OffscreenCanvasRenderingContext2D>>,
}

impl OffscreenCanvas {
    pub fn new(
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            reflector: None,
            canvas_id,
            width,
            height,
            context_mode: Rc::new(Cell::new(OffscreenCanvasContextMode::None)),
            context_2d: gc_cell_new(None, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-offscreencanvas>
    pub(crate) fn constructor(
        width: u64,
        height: u64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: "Initialize the bitmap of this to a rectangular array of transparent black pixels of the dimensions specified by width and height."
        // Step 2: "Initialize the width of this to width."
        // Step 3: "Initialize the height of this to height."
        // Note: the bitmap is realized as the graphics-process canvas slot, and
        // the width and height are stored on the platform object.  A
        // constructor-created canvas has no placeholder canvas element, so it
        // has no embed site.  The dimensions are stored as 32-bit values, so a
        // width or height above `u32::MAX` saturates.
        let width = u32::try_from(width).unwrap_or(u32::MAX);
        let height = u32::try_from(height).unwrap_or(u32::MAX);
        let canvas = Self::new(CanvasId::new(), width, height, ec);
        canvas.register_canvas_with_graphics(ec)?;

        // Step 4: "Set this's inherited language to explicitly unknown."
        // Step 5: "Set this's inherited direction to \"ltr\"."
        // Step 6: "Let global be the relevant global object of this."
        // Step 7: "If global is a Window object:"
        // Step 7.1: "Let element be the document element of global's associated Document."
        // Step 7.2: "If element is not null:"
        // Step 7.2.1: "Set the inherited language of this to element's language."
        // Step 7.2.2: "Set the inherited direction of this to element's directionality."
        // Note: inherited language and direction are not modeled.
        Ok(canvas)
    }

    /// Register the canvas with the graphics process so its committed scenes
    /// are accepted.  A constructor-created canvas has no placeholder embed
    /// site, so it is not added to the document's canvas registry.
    pub(crate) fn register_canvas_with_graphics(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        with_global_scope(ec, |global_scope, ec| {
            if let Some(graphics_sender) = global_scope.graphics_sender() {
                let command =
                    GraphicsCommand::RegisterCanvas {
                        webview_id: WebviewId(global_scope.source_navigable_id().ok_or_else(
                            || ec.new_type_error("OffscreenCanvas has no navigable id"),
                        )?),
                        canvas_id: self.canvas_id,
                    };
                if let Err(error) = graphics_sender.send(command) {
                    error!("failed to register canvas with graphics: {error}");
                }
            }
            Ok(())
        })
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
    ) -> Completion<Option<OffscreenCanvasRenderingContext2D>, Types> {
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
    ) -> Completion<OffscreenCanvasRenderingContext2D, Types> {
        // Step 1: "If the algorithm was passed some arguments, let arg be the first such argument. Otherwise, let arg be undefined."
        // Step 2: "Let settings be the result of converting arg to the dictionary type CanvasRenderingContext2DSettings. (This can throw an exception.)"
        // Note: options are not passed or modeled.
        // Step 3: "Let context be a new OffscreenCanvasRenderingContext2D object."
        let context_object = create_interface_instance::<Types, OffscreenCanvasRenderingContext2D>(
            OffscreenCanvasRenderingContext2D::new(self.canvas_id, self.width, self.height),
            ec,
        )?;
        // The platform data is cloned back out of the created object; its
        // reflector was set by `create_interface_instance`, so the context the
        // OffscreenCanvas caches resolves to this same object.
        let context = ec
            .with_object_any(&context_object)
            .and_then(|data| {
                data.downcast_ref::<OffscreenCanvasRenderingContext2D>()
                    .cloned()
            })
            .ok_or_else(|| {
                ec.new_type_error("OffscreenCanvasRenderingContext2D object has no platform data")
            })?;

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
