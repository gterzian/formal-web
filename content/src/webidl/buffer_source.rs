//! <https://webidl.spec.whatwg.org/#js-buffer-source-types>

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

#[allow(dead_code)]
type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy>
#[allow(dead_code)]
pub(crate) fn get_a_copy_of_the_buffer_source(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<u8>, Types> {
    // Step 1: "Let jsBufferSource be the result of converting bufferSource
    //          to a JavaScript value."
    let object = <Types as JsTypes>::value_as_object(value)
        .ok_or_else(|| ec.new_type_error("argument must be an ArrayBuffer or typed array"))?;

    // Step 5: "If jsBufferSource has a [[ViewedArrayBuffer]] internal slot, then:"
    if let Some(typed_array) = <Types as JsTypes>::object_as_typed_array(&object) {
        // Step 5.1: "Set jsArrayBuffer to jsBufferSource.[[ViewedArrayBuffer]]."
        let array_buffer = ec.typed_array_buffer(&typed_array)?;

        // Step 5.2: "Set offset to jsBufferSource.[[ByteOffset]]."
        let offset = ec.typed_array_byte_offset(&typed_array)? as usize;

        // Step 5.3: "Set length to jsBufferSource.[[ByteLength]]."
        let length = ec.typed_array_byte_length(&typed_array)? as usize;

        // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
        //          the empty byte sequence."
        // Step 8: "Let bytes be a new byte sequence of length equal to length."
        // Step 9: "For i in the range offset to offset + length − 1, ..."
        if let Some(all_bytes) = ec.array_buffer_data(&array_buffer) {
            // Step 10: "Return bytes."
            return Ok(all_bytes[offset..offset + length].to_vec());
        }
        return Ok(Vec::new());
    }

    // Step 6: "Otherwise:"
    // Step 6.1: "Assert: jsBufferSource is an ArrayBuffer or SharedArrayBuffer object."
    if let Some(array_buffer) = <Types as JsTypes>::object_as_array_buffer(&object) {
        // Step 6.2: "Set length to jsBufferSource.[[ArrayBufferByteLength]]."
        // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
        //          the empty byte sequence."
        // Step 8-9: "Let bytes be a new byte sequence ..."
        // Step 10: "Return bytes."
        return Ok(ec.array_buffer_data(&array_buffer).unwrap_or_default());
    }

    Err(ec.new_type_error("argument must be an ArrayBuffer or typed array"))
}

/// <https://webidl.spec.whatwg.org/#dfn-buffer-source-type>
#[allow(dead_code)]
pub(crate) fn is_buffer_source(value: &JsValue, _ec: &mut dyn ExecutionContext<Types>) -> bool {
    let Some(object) = <Types as JsTypes>::value_as_object(value) else {
        return false;
    };
    <Types as JsTypes>::object_as_array_buffer(&object).is_some()
        || <Types as JsTypes>::object_as_typed_array(&object).is_some()
}
