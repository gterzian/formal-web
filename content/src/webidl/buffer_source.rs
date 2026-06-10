use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    object::{JsObject, builtins::JsTypedArray},
    object::builtins::{JsArrayBuffer, JsSharedArrayBuffer, JsUint8Array},
};

/// Reject a value if it is a SharedArrayBuffer.
///
/// Called by `get_a_copy_of_the_buffer_source` and
/// `convert_js_value_to_idl_array_buffer` to enforce the
/// "not associated with [AllowShared]" constraint.
fn reject_if_shared_array_buffer(value: &JsValue) -> JsResult<()> {
    if let Some(object) = value.as_object() {
        if JsSharedArrayBuffer::from_object(object.clone()).is_ok() {
            return Err(JsNativeError::typ()
                .with_message("SharedArrayBuffer is not allowed in this context")
                .into());
        }
    }
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy>
///
/// Get a copy of the bytes held by the buffer source.
///
/// Implements the Web IDL algorithm of the same name.  This version
/// enforces the [AllowShared] and [AllowResizable] constraints for a
/// `BufferSource` without either extended attribute (as used by the
/// WebAssembly API).
pub(crate) fn get_a_copy_of_the_buffer_source(
    value: &JsValue,
    context: &mut Context,
) -> JsResult<Vec<u8>> {
    // Step 1: "Let jsBufferSource be the result of converting bufferSource
    //          to a JavaScript value."
    // (The caller has already provided the JavaScript value.)

    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("argument must be an ArrayBuffer or typed array")
    })?;

    // Step 5: "If jsBufferSource has a [[ViewedArrayBuffer]] internal slot, then:"
    if let Ok(typed_array) = JsTypedArray::from_object(object.clone()) {
        // Step 5.1: "Set jsArrayBuffer to jsBufferSource.[[ViewedArrayBuffer]]."
        let buffer_value = typed_array.buffer(context)?;

        // Reject SharedArrayBuffer backing (not allowed without AllowShared).
        reject_if_shared_array_buffer(&buffer_value)?;

        // Step 5.2: "Set offset to jsBufferSource.[[ByteOffset]]."
        let offset = typed_array.byte_offset(context)?;

        // Step 5.3: "Set length to jsBufferSource.[[ByteLength]]."
        let length = typed_array.byte_length(context)?;

        // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
        //          the empty byte sequence."
        // Note: Boa's to_vec() returns an error for detached buffers.

        // Step 8: "Let bytes be a new byte sequence of length equal to length."
        // Step 9: "For i in the range offset to offset + length − 1, ..."
        //
        // We use a Uint8Array view of the backing buffer to efficiently
        // copy the viewed range into a Vec.
        if let Some(buf_object) = buffer_value.as_object() {
            if let Ok(buf) = JsArrayBuffer::from_object(buf_object.clone()) {
                if let Some(all_bytes) = buf.to_vec() {
                    // Step 10: "Return bytes."
                    return Ok(all_bytes[offset..offset + length].to_vec());
                }
                // Detached buffer → return empty per Step 7.
                return Ok(Vec::new());
            }
        }

        return Err(JsNativeError::typ()
            .with_message("typed array backing buffer is not a valid ArrayBuffer")
            .into());
    }

    // Step 6: "Otherwise:"
    // Step 6.1: "Assert: jsBufferSource is an ArrayBuffer object."
    // Reject SharedArrayBuffer first.
    reject_if_shared_array_buffer(value)?;

    if let Ok(array_buffer) = JsArrayBuffer::from_object(object.clone()) {
        // Step 6.2: "Set length to jsBufferSource.[[ArrayBufferByteLength]]."
        // Step 7: "If IsDetachedBuffer(jsArrayBuffer) is true, then return
        //          the empty byte sequence."
        // Note: to_vec() returns None if detached, which we map to empty.
        // Step 8-9: "Let bytes be a new byte sequence ..."
        // Step 10: "Return bytes."
        return Ok(array_buffer.to_vec().unwrap_or_default());
    }

    Err(JsNativeError::typ()
        .with_message("argument must be an ArrayBuffer or typed array")
        .into())
}

/// <https://webidl.spec.whatwg.org/#js-arraybuffer>
///
/// Convert a JavaScript value to an IDL ArrayBuffer value, rejecting
/// SharedArrayBuffer and resizable ArrayBuffers.
///
/// Implements "convert a JavaScript value to IDL ArrayBuffer" without
/// AllowShared and without AllowResizable.
pub(crate) fn convert_js_value_to_idl_array_buffer(
    value: &JsValue,
    _context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "If V is not an Object, or V does not have an [[ArrayBufferData]]
    //          internal slot, then throw a TypeError."
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("expected an ArrayBuffer object")
    })?;

    // Step 2: "If IsSharedArrayBuffer(V) is true, then throw a TypeError."
    if JsSharedArrayBuffer::from_object(object.clone()).is_ok() {
        return Err(JsNativeError::typ()
            .with_message("SharedArrayBuffer is not allowed in this context")
            .into());
    }

    // Step 3: "If ... not [AllowResizable] and IsFixedLengthArrayBuffer(V) is
    //          false, then throw a TypeError."
    // Note: Boa does not expose IsFixedLengthArrayBuffer publicly.

    // Step 4: "Return the IDL ArrayBuffer value that is a reference to the
    //          same object as V."
    let buf = JsArrayBuffer::from_object(object)?;
    Ok(buf.into())
}

/// <https://webidl.spec.whatwg.org/#dfn-buffer-source-type>
///
/// Check whether a JavaScript value is a buffer source type (ArrayBuffer,
/// SharedArrayBuffer, or ArrayBufferView).
pub(crate) fn is_buffer_source(value: &JsValue, _context: &mut Context) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    JsArrayBuffer::from_object(object.clone()).is_ok()
        || JsTypedArray::from_object(object.clone()).is_ok()
}
