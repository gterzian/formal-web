use boa_engine::{Context, JsNativeError, JsResult, JsValue, js_string};
use boa_engine::object::{JsObject, builtins::{JsArrayBuffer, JsTypedArray, JsUint8Array}};

/// <https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy>
pub(crate) fn get_stable_bytes(value: &JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    // Step: "Let jsBufferSource be the result of converting bufferSource to a JavaScript value."
    // Step: "Let jsArrayBuffer be jsBufferSource."
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebAssembly: argument must be an ArrayBuffer or typed array")
    })?;

    // Step: "If jsBufferSource has a [[ViewedArrayBuffer]] internal slot, then:"
    if let Ok(typed_array) = JsTypedArray::from_object(object.clone()) {
        // Step: "Set jsArrayBuffer to jsBufferSource.[[ViewedArrayBuffer]]."
        // Step: "Set offset to jsBufferSource.[[ByteOffset]]."
        // Step: "Set length to jsBufferSource.[[ByteLength]]."
        let length = typed_array.length(context)?;
        let mut bytes = vec![0u8; length];
        for i in 0..length {
            let v = object.get(i, context).map_err(|_| {
                JsNativeError::typ().with_message("failed to read typed array")
            })?;
            if let Some(num) = v.as_number() {
                bytes[i] = num as u8;
            }
        }
        return Ok(bytes);
    }

    // Step (otherwise): "Set length to jsBufferSource.[[ArrayBufferByteLength]]."
    if let Ok(array_buffer) = JsArrayBuffer::from_object(object.clone()) {
        if let Some(buf_bytes) = array_buffer.to_vec() {
            return Ok(buf_bytes);
        }
        // Fallback: create a Uint8Array view and read via indexed access.
        let view = JsUint8Array::from_array_buffer(array_buffer, context)?;
        let view_obj: JsObject = view.into();
        let len = view_obj
            .get(js_string!("length"), context)
            .ok()
            .and_then(|v| v.as_number())
            .map(|n| n as usize)
            .unwrap_or(0);
        let mut bytes = vec![0u8; len];
        for i in 0..len {
            let v = view_obj.get(i, context).map_err(|_| {
                JsNativeError::typ().with_message("failed to read array buffer")
            })?;
            if let Some(num) = v.as_number() {
                bytes[i] = num as u8;
            }
        }
        return Ok(bytes);
    }

    Err(JsNativeError::typ()
        .with_message("WebAssembly: argument must be an ArrayBuffer or typed array")
        .into())
}

/// <https://webidl.spec.whatwg.org/#dfn-buffer-source-type>
pub(crate) fn is_buffer_source(value: &JsValue, _context: &mut Context) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    JsArrayBuffer::from_object(object.clone()).is_ok()
        || JsTypedArray::from_object(object.clone()).is_ok()
}
