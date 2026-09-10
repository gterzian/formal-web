use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use ipc_messages::content::CanvasId;
use js_engine::{Completion, ExecutionContext, gc_struct};

use crate::html::HTMLElement;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

use super::OffscreenCanvas;

/// <https://html.spec.whatwg.org/multipage/canvas.html#htmlcanvaselement>
#[gc_struct]
pub struct HTMLCanvasElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLCanvasElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
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

    /// <https://html.spec.whatwg.org/multipage/canvas.html#dom-canvas-transfercontroltooffscreen>
    pub(crate) fn transfer_control_to_offscreen(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as js_engine::JsTypes>::JsObject, Types> {
        // Step 1: "If this canvas element's context mode is not set to none, throw an
        // "InvalidStateError" DOMException."
        // Note: A canvas element is a placeholder once a CanvasId is registered
        // for its node; re-invoking transferControlToOffscreen throws.
        let node_id = self.html_element.element.node.node_id;
        let already_transferred =
            crate::js::platform_objects::with_global_scope(ec, |global_scope, _ec| {
                let Some(document_id) = global_scope.document_id() else {
                    return Ok(false);
                };
                Ok(global_scope.canvas_registry().is_some_and(|registry| {
                    registry.borrow().contains_key(&(document_id, node_id))
                }))
            })?;
        if already_transferred {
            return Err(ec.new_type_error("InvalidStateError"));
        }

        // Step 2: "Let offscreenCanvas be a new OffscreenCanvas object with its
        // width and height equal to the values of the width and height content
        // attributes of this canvas element."
        let width = self.width_attribute();
        let height = self.height_attribute();
        let canvas_id = CanvasId::new();

        // Step 3: "Set the offscreenCanvas's placeholder canvas element to a
        // weak reference to this canvas element."
        // Step 4: "Set this canvas element's context mode to placeholder."
        // Note: The placeholder linkage is the CanvasId registered for this
        // element's node (the graphics process fills the element's embed site
        // from the OffscreenCanvas's committed scenes). The registration and
        // the graphics-process canvas registration both happen here.
        let offscreen_object = create_interface_instance::<Types, OffscreenCanvas>(
            OffscreenCanvas::new(canvas_id, width, height, ec),
            ec,
        )?;

        crate::js::platform_objects::with_global_scope(ec, |global_scope, ec| {
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
                let command = ipc_messages::graphics::GraphicsCommand::RegisterCanvas {
                    webview_id: ipc_messages::content::WebviewId(
                        global_scope.source_navigable_id().ok_or_else(|| {
                            ec.new_type_error("canvas element has no navigable id")
                        })?,
                    ),
                    canvas_id,
                };
                if let Err(error) = graphics_sender.send(command) {
                    log::error!("failed to register canvas with graphics: {error}");
                }
            }
            // A canvas embed site appeared without a DOM mutation; mark the
            // document dirty so the next update-the-rendering rebuilds the
            // frame composition and includes it.
            global_scope.mark_document_dirty();
            Ok(())
        })?;

        // Step 5: "Set the offscreenCanvas's inherited language to the language
        // of this canvas element."
        // Step 6: "Set the offscreenCanvas's inherited direction to the
        // directionality of this canvas element."
        // TODO: Not yet implemented (no inherited language/direction state).
        // Step 7: "Return offscreenCanvas."
        Ok(offscreen_object)
    }
}
