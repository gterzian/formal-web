//! <https://webidl.spec.whatwg.org/#js-buffer-source-types>

use boa_engine::{
    JsNativeError, JsResult, JsValue, object::builtins::JsArrayBuffer,
    object::builtins::JsTypedArray,
};

use js_engine::{Completion, ExecutionContext};

/// <https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy>
pub(crate) fn get_a_copy_of_the_buffer_source(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<u8>, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let result: JsResult<Vec<u8>> = (|| {
        // Step 1: "Let jsBufferSource be the result of converting bufferSource
        //          to a JavaScript value."
        let object = value.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("argument must be an ArrayBuffer or typed array")
        })?;

        // Step 5: "If jsBufferSource has a [[ViewedArrayBuffer]] internal slot, then:"
        if let Ok(typed_array) = JsTypedArray::from_object(object.clone()) {
            // Step 5.1: "Set jsArrayBuffer to jsBufferSource.[[ViewedArrayBuffer]]."
            let buffer_value = typed_array.buffer(context)?;
            // Step 5.2: "Set offset to jsBufferSource.[[ByteOffset]]."
            let offset = typed_array.byte_offset(context)?;
            // Step 5.3: "Set length to jsBufferSource.[[ByteLength]]."
            let length = typed_array.byte_length(context)?;

            // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
            //          the empty byte sequence."
            // Step 8: "Let bytes be a new byte sequence of length equal to length."
            // Step 9: "For i in the range offset to offset + length − 1, ..."
            if let Some(buf_object) = buffer_value.as_object() {
                if let Ok(buf) = JsArrayBuffer::from_object(buf_object.clone()) {
                    if let Some(all_bytes) = buf.to_vec() {
                        // Step 10: "Return bytes."
                        return Ok(all_bytes[offset..offset + length].to_vec());
                    }
                    return Ok(Vec::new());
                }
            }

            return Err(JsNativeError::typ()
                .with_message("typed array backing buffer is not a valid ArrayBuffer")
                .into());
        }

        // Step 6: "Otherwise:"
        // Step 6.1: "Assert: jsBufferSource is an ArrayBuffer or SharedArrayBuffer object."
        if let Ok(array_buffer) = JsArrayBuffer::from_object(object.clone()) {
            // Step 6.2: "Set length to jsBufferSource.[[ArrayBufferByteLength]]."
            // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
            //          the empty byte sequence."
            // Step 8-9: "Let bytes be a new byte sequence ..."
            // Step 10: "Return bytes."
            return Ok(array_buffer.to_vec().unwrap_or_default());
        }

        Err(JsNativeError::typ()
            .with_message("argument must be an ArrayBuffer or typed array")
            .into())
    })();
    crate::js::js_result_to_completion(result, context)
}

/// <https://webidl.spec.whatwg.org/#dfn-buffer-source-type>
pub(crate) fn is_buffer_source(value: &JsValue, _ec: &mut dyn ExecutionContext<crate::js::Types>) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    JsArrayBuffer::from_object(object.clone()).is_ok()
        || JsTypedArray::from_object(object.clone()).is_ok()
}
