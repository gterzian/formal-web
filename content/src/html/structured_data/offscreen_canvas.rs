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

use crate::html::{OffscreenCanvas, OffscreenCanvasContextMode};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{data_clone_error_value, invalid_state_error_value};

use js_engine::{Completion, ExecutionContext, JsTypes};

type Types = crate::js::Types;
type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://html.spec.whatwg.org/#the-offscreencanvas-interface:transfer-steps>
pub(crate) fn transfer_steps(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransferDataHolder, Types> {
    let canvas = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned())
        .ok_or_else(|| data_clone_error_value(ec))?;
    // Step 1: "If value's context mode is not equal to none, then throw an "InvalidStateError" DOMException."
    if canvas.context_mode() != OffscreenCanvasContextMode::None {
        return Err(invalid_state_error_value(ec));
    }
    // Step 2: "Set value's context mode to detached."
    canvas.set_context_mode(OffscreenCanvasContextMode::Detached);
    // Step 3: "Let width and height be the dimensions of value's bitmap."
    let width = canvas.width();
    let height = canvas.height();
    // Step 4: "Let language and direction be the values of value's inherited language and inherited direction."
    // Note: inherited language and direction are not modeled.
    // Step 5: "Unset value's bitmap."
    // Note: the bitmap is owned by the graphics process; nothing is unset here.
    // Step 6: "Set dataHolder.[[Width]] to width and dataHolder.[[Height]] to height."
    // Step 7: "Set dataHolder.[[Language]] to language and dataHolder.[[Direction]] to direction."
    // Note: not modeled (see step 4).
    // Step 8: "Set dataHolder.[[PlaceholderCanvas]] to be a weak reference to value's placeholder canvas element, if value has one, or null if it does not."
    // Note: the placeholder canvas element linkage is the canvas id.
    Ok(TransferDataHolder::OffscreenCanvas(
        OffscreenCanvasTransferData {
            canvas_id: canvas.canvas_id(),
            width,
            height,
        },
    ))
}

/// <https://html.spec.whatwg.org/#the-offscreencanvas-interface:transfer-receiving-steps>
pub(crate) fn transfer_receiving_steps(
    data_holder: &OffscreenCanvasTransferData,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: "Initialize value's bitmap to a rectangular array of transparent black pixels with width given by dataHolder.[[Width]] and height given by dataHolder.[[Height]]."
    // Step 2: "Set value's inherited language to dataHolder.[[Language]] and its inherited direction to dataHolder.[[Direction]]."
    // Note: inherited language and direction are not modeled.
    // Step 3: "If dataHolder.[[PlaceholderCanvas]] is not null, set value's placeholder canvas element to dataHolder.[[PlaceholderCanvas]] (while maintaining the weak reference semantics)."
    // Note: the placeholder canvas element linkage is the canvas id.
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
