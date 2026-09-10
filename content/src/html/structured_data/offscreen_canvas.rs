//! The OffscreenCanvas-specific parts of the safe passing of structured data:
//! recognizing a transferable OffscreenCanvas (the [[Detached]]-slot check of
//! StructuredSerializeWithTransfer step 2.1), running its transfer steps and
//! building its data holder (step 5.2), and rebuilding the canvas on the
//! receiving side (StructuredDeserializeWithTransfer step 3.2).  The generic
//! algorithms live in [`super::safe_passing_of_structured_data`]; the wire
//! data holder (`OffscreenCanvasTransferData`) lives in
//! `ipc_messages::safe_passing_of_structured_data`.

use ipc_messages::safe_passing_of_structured_data::{
    OffscreenCanvasTransferData, TransferDataHolder,
};

use crate::html::OffscreenCanvas;
use crate::webidl::bindings::create_interface_instance;

use js_engine::{Completion, ExecutionContext, JsTypes};

type Types = crate::js::Types;
type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvas>
/// (StructuredSerializeWithTransfer step 5.2's OffscreenCanvas branch).
pub(crate) fn transfer_steps(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransferDataHolder, Types> {
    // The OffscreenCanvas transfer steps: the canvas id (the placeholder
    // canvas element reference, realized as the shared CanvasId) and the
    // bitmap dimensions travel in the data holder; the source object's
    // context mode becomes detached, which the source realm no longer tracks.
    let canvas = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned())
        .ok_or_else(|| crate::webidl::data_clone_error_value(ec))?;
    Ok(TransferDataHolder::OffscreenCanvas(
        OffscreenCanvasTransferData {
            canvas_id: canvas.canvas_id(),
            width: canvas.width(),
            height: canvas.height(),
        },
    ))
}

/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvas>
/// (StructuredDeserializeWithTransfer step 3.2's OffscreenCanvas branch).
pub(crate) fn transfer_receiving_steps(
    data_holder: &OffscreenCanvasTransferData,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // A new OffscreenCanvas is created in the target realm with the carried
    // dimensions, linked to the same placeholder canvas element through the
    // shared canvas id (the transfer-receiving steps set the placeholder
    // reference; here the id is the reference).
    let canvas = OffscreenCanvas::new(
        data_holder.canvas_id,
        data_holder.width,
        data_holder.height,
        ec,
    );
    let object = create_interface_instance::<Types, OffscreenCanvas>(canvas, ec)?;
    Ok(Types::value_from_object(object))
}

/// Whether a transferable is an OffscreenCanvas platform object, which has a
/// [[Detached]] internal slot and therefore satisfies the check of
/// StructuredSerializeWithTransfer step 2.1.
pub(crate) fn is_transferable_platform_object(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> bool {
    ec.with_object_any(object)
        .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned())
        .is_some()
}
