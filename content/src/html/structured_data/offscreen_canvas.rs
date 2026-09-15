//! The OffscreenCanvas-specific parts of the safe passing of structured data:
//! running its transfer steps and building its data holder (step 5.2 of
//! StructuredSerializeWithTransfer), and rebuilding the canvas on the
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

use js_engine::{Completion, ExecutionContext};

type Types = crate::js::Types;

/// <https://html.spec.whatwg.org/#the-offscreencanvas-interface:transfer-steps>
pub(crate) fn transfer_steps(
    value: &OffscreenCanvas,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<TransferDataHolder, Types> {
    // Step 1: "If value's context mode is not equal to none, then throw an "InvalidStateError" DOMException."
    if value.context_mode() != OffscreenCanvasContextMode::None {
        return Err(invalid_state_error_value(ec));
    }
    // Step 2: "Set value's context mode to detached."
    value.set_context_mode(OffscreenCanvasContextMode::Detached);
    // Step 3: "Let width and height be the dimensions of value's bitmap."
    let width = value.width();
    let height = value.height();
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
            canvas_id: value.canvas_id(),
            width,
            height,
        },
    ))
}

/// <https://html.spec.whatwg.org/#the-offscreencanvas-interface:transfer-receiving-steps>
pub(crate) fn transfer_receiving_steps(
    data_holder: &OffscreenCanvasTransferData,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<OffscreenCanvas, Types> {
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
    let canvas_object = create_interface_instance::<Types, OffscreenCanvas>(canvas, ec)?;
    // The platform data is cloned back out of the created object; its
    // reflector was set by `create_interface_instance`.
    ec.with_object_any(&canvas_object)
        .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned())
        .ok_or_else(|| data_clone_error_value(ec))
}
