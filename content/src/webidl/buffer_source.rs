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
    if let Some(data_view) = <Types as JsTypes>::object_as_data_view(&object) {
        // Step 5.1: "Set jsArrayBuffer to jsBufferSource.[[ViewedArrayBuffer]]."
        let array_buffer = ec.data_view_buffer(&data_view)?;

        // Step 5.2: "Set offset to jsBufferSource.[[ByteOffset]]."
        let offset = ec.data_view_byte_offset(&data_view)? as usize;

        // Step 5.3: "Set length to jsBufferSource.[[ByteLength]]."
        let length = ec.data_view_byte_length(&data_view)? as usize;

        // Steps 7 to 10: the bytes of the viewed buffer, or the empty byte
        // sequence for a detached buffer.
        if let Some(all_bytes) = ec.array_buffer_data(&array_buffer) {
            return Ok(all_bytes[offset..offset + length].to_vec());
        }
        return Ok(Vec::new());
    }
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
        || <Types as JsTypes>::object_as_data_view(&object).is_some()
}

/// <https://webidl.spec.whatwg.org/#arraybuffer-create>
pub(crate) fn create_array_buffer(
    bytes: &[u8],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::JsObject, Types> {
    // Step 1: "Let jsArrayBuffer be ? AllocateArrayBuffer(realm.[[Intrinsics]].[[%ArrayBuffer%]], byteSequence's length)."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let buffer =
        ec.allocate_array_buffer(intrinsics.array_buffer.clone(), bytes.len() as u64, None)?;
    // Step 2: "Let arrayBuffer be the result of converting jsArrayBuffer to an IDL value of type ArrayBuffer."
    // Step 3: "Write byteSequence into arrayBuffer."
    for (index, byte) in bytes.iter().enumerate() {
        let value = ec.value_from_number(f64::from(*byte));
        ec.set_value_in_buffer(
            &buffer,
            index as u64,
            js_engine::enums::TypedArrayElementType::Uint8,
            value,
            false,
            js_engine::enums::SharedMemoryOrder::Unordered,
        )?;
    }
    // Step 4: "Return arrayBuffer."
    Ok(<Types as JsTypes>::object_from_array_buffer(buffer))
}

/// <https://webidl.spec.whatwg.org/#dfn-create-a-Uint8Array>
pub(crate) fn create_uint8_array(
    bytes: &[u8],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::JsObject, Types> {
    // Step 1: "Let jsArrayBuffer be the result of creating an ArrayBuffer from byteSequence in realm."
    let buffer_object = create_array_buffer(bytes, ec)?;
    let buffer = <Types as JsTypes>::object_as_array_buffer(&buffer_object)
        .ok_or_else(|| ec.new_type_error("the created ArrayBuffer is not an ArrayBuffer"))?;

    // Step 2: "Let jsUint8Array be ! Construct(%Uint8Array%, « jsArrayBuffer »)."
    let array = ec.construct_typed_array_view(
        js_engine::enums::TypedArrayElementType::Uint8,
        buffer,
        0,
        bytes.len() as u64,
    )?;

    // Step 3: "Return the result of converting jsUint8Array to an IDL value of type Uint8Array."
    Ok(<Types as JsTypes>::object_from_typed_array(array))
}

/// <https://webidl.spec.whatwg.org/#dfn-write>
pub(crate) fn write_into_array_buffer_view(
    bytes: &[u8],
    view: &JsValue,
    starting_offset: u64,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Let jsView be the result of converting view to a JavaScript value."
    let object = <Types as JsTypes>::value_as_object(view)
        .ok_or_else(|| ec.new_type_error("the view is not an ArrayBufferView"))?;
    let typed_array = <Types as JsTypes>::object_as_typed_array(&object)
        .ok_or_else(|| ec.new_type_error("the view is not an ArrayBufferView"))?;

    // Step 2: "Assert: bytes's length ≤ jsView.[[ByteLength]] − startingOffset."
    let byte_length = ec.typed_array_byte_length(&typed_array)?;
    debug_assert!(bytes.len() as u64 <= byte_length.saturating_sub(starting_offset));

    // Step 3: "Assert: if view is not a DataView, then bytes's length modulo the element size of view's type is 0."
    // Step 4: "Let arrayBuffer be the result of converting jsView.[[ViewedArrayBuffer]] to an IDL value of type ArrayBuffer."
    let array_buffer = ec.typed_array_buffer(&typed_array)?;

    // Step 5: "Write bytes into arrayBuffer with startingOffset set to jsView.[[ByteOffset]] + startingOffset."
    let byte_offset = ec.typed_array_byte_offset(&typed_array)?;
    for (index, byte) in bytes.iter().enumerate() {
        let value = ec.value_from_number(f64::from(*byte));
        ec.set_value_in_buffer(
            &array_buffer,
            byte_offset + starting_offset + index as u64,
            js_engine::enums::TypedArrayElementType::Uint8,
            value,
            false,
            js_engine::enums::SharedMemoryOrder::Unordered,
        )?;
    }
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#dfn-byte-length>
pub(crate) fn array_buffer_view_byte_length(
    view: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u64, Types> {
    let object = <Types as JsTypes>::value_as_object(view)
        .ok_or_else(|| ec.new_type_error("the view is not an ArrayBufferView"))?;
    let typed_array = <Types as JsTypes>::object_as_typed_array(&object)
        .ok_or_else(|| ec.new_type_error("the view is not an ArrayBufferView"))?;
    ec.typed_array_byte_length(&typed_array)
}

/// <https://webidl.spec.whatwg.org/#js-buffer-source-types>
pub(crate) fn convert_js_to_uint8_array(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::TypedArray, Types> {
    // Step 1: "If V is not an Object, or V does not have a [[TypedArrayName]] internal slot with a value equal to T's name, then throw a TypeError."
    let typed_array = <Types as JsTypes>::value_as_object(value)
        .and_then(|object| <Types as JsTypes>::object_as_typed_array(&object))
        .ok_or_else(|| ec.new_type_error("the value is not a Uint8Array"))?;
    if ec.typed_array_element_type(&typed_array)
        != Some(js_engine::enums::TypedArrayElementType::Uint8)
    {
        return Err(ec.new_type_error("the value is not a Uint8Array"));
    }

    // Step 2: "If IsTypedArrayOutOfBounds(V.[[ViewedArrayBuffer]]) is true, then throw a TypeError."
    // Step 3: "Return the IDL value of type T that is a reference to the same object as V."
    Ok(typed_array)
}
