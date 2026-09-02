//! <https://html.spec.whatwg.org/#safe-passing-of-structured-data>
//!
//! Implements the structured clone algorithm, including serialization and
//! deserialization of JavaScript values across realm boundaries, and support
//! for transferable and serializable platform objects.
//!
//! The `SerializedRecord`, `PrimitiveValue`, and `TransferDataHolder` types
//! are pure data (serde-serializable) so they can cross IPC boundaries; they
//! live in `ipc_messages::safe_passing_of_structured_data`.

// The traits, variants, and fields below that trigger dead_code warnings
// are intentionally defined as the spec-required extension points for
// future [Serializable] platform objects and resizable ArrayBuffer
// support. All of them will be used once those features are wired up.
// (`Transferable` is implemented by `MessagePort` and used by the
// structured serialize/deserialize flows.)
#![allow(dead_code)]

use std::collections::HashMap;

use ipc_messages::safe_passing_of_structured_data::{
    PrimitiveValue, SerializedRecord, TransferDataHolder,
};

use super::messageport;

use js_engine::{
    Completion, EcmascriptHost, ExecutionContext, JsTypes, enums::TypedArrayElementType,
};

/// <https://html.spec.whatwg.org/#safe-passing-of-structured-data>
type Types = crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
type JsString = <Types as JsTypes>::JsString;
type JsBigInt = <Types as JsTypes>::JsBigInt;
type PropertyKey = <Types as JsTypes>::PropertyKey;
type ArrayBuffer = <Types as JsTypes>::ArrayBuffer;
type SharedArrayBuffer = <Types as JsTypes>::SharedArrayBuffer;
type TypedArray = <Types as JsTypes>::TypedArray;
type DataView = <Types as JsTypes>::DataView;
type MapType = <Types as JsTypes>::Map;
type SetType = <Types as JsTypes>::Set;
type Constructor = <Types as JsTypes>::Constructor;

// Traits for platform objects (bridge layer)

/// <https://html.spec.whatwg.org/#serializable-objects>
///
/// A platform object whose primary interface is decorated with the `[Serializable]`
/// Web IDL extended attribute must implement this trait.
pub trait Serializable: std::fmt::Debug {
    /// <https://html.spec.whatwg.org/#serialization-steps>
    fn serialization_steps(
        &self,
        serialized: &mut HashMap<String, JsValue>,
        for_storage: bool,
        memory: &mut MemoryMap,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types>;
}

/// <https://html.spec.whatwg.org/#transferable-objects>
///
/// A platform object whose primary interface is decorated with the `[Transferable]`
/// Web IDL extended attribute must implement this trait.
pub trait Transferable {
    /// The data holder carried with the transfer (the spec's dataHolder),
    /// as serde/IPC-safe pure data so a transfer can cross processes.
    type TransferDataHolder: std::fmt::Debug;

    /// <https://html.spec.whatwg.org/#transferable-objects:transfer-steps>
    fn transfer_steps(
        &self,
        data_holder: &mut Self::TransferDataHolder,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types>;

    /// <https://html.spec.whatwg.org/#transferable-objects:transfer-receiving-steps>
    fn transfer_receiving_steps(
        &self,
        data_holder: &Self::TransferDataHolder,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types>;
}

// Pure-data types (IPC-safe)
//
// `PrimitiveValue`, `SerializedRecord`, and `TransferDataHolder` live in
// `ipc_messages::safe_passing_of_structured_data`; they are the wire format for the
// structured clone algorithm across the content/user-agent IPC boundary.

// Memory map (for cycle/duplicate detection)

/// The memory map used for cycle and duplicate detection during (de)serialization.
///
/// <https://html.spec.whatwg.org/#structuredserializeinternal> step 1.
#[derive(Default)]
pub struct MemoryMap {
    /// Object → (memory index, serialized record), keyed by object identity
    /// (the spec's `memory` map).  Every object serialized into the graph
    /// gets a stable index; a later occurrence of the same object (a cycle
    /// or a shared DAG node) serializes as a `BackReference(index)`.  The
    /// records are kept for the whole serialization (never removed), so a
    /// completed record is still reachable by index.
    serialized: Vec<(JsObject, usize, SerializedRecord)>,
    next_serialized_index: usize,
    /// Memory index → deserialized object, built in the same pre-order the
    /// serialization assigned the indices.  `BackReference(index)` resolves
    /// here to the object already built for that record.
    deserialized: HashMap<usize, JsObject>,
    next_deserialized_index: usize,
    /// Values rebuilt from the transfer data holders by
    /// StructuredDeserializeWithTransfer step 3, indexed by transfer-list
    /// position; `SerializedRecord::TransferredValue(index)` resolves here.
    /// <https://html.spec.whatwg.org/#structureddeserializewithtransfer>
    transferred_values: Vec<JsValue>,
}

impl MemoryMap {
    fn get_serialized(&self, object: &JsObject) -> Option<&SerializedRecord> {
        self.serialized
            .iter()
            .find(|(key, _, _)| key == object)
            .map(|(_, _, record)| record)
    }
    fn get_serialized_index(&self, object: &JsObject) -> Option<usize> {
        self.serialized
            .iter()
            .find(|(key, _, _)| key == object)
            .map(|(_, index, _)| *index)
    }
    fn insert_serialized(&mut self, object: &JsObject, record: SerializedRecord) {
        if let Some((_, _, existing)) = self.serialized.iter_mut().find(|(key, _, _)| key == object)
        {
            *existing = record;
        } else {
            let index = self.next_serialized_index;
            self.next_serialized_index += 1;
            self.serialized.push((object.clone(), index, record));
        }
    }
    fn get_serialized_mut(&mut self, object: &JsObject) -> Option<&mut SerializedRecord> {
        self.serialized
            .iter_mut()
            .find(|(key, _, _)| key == object)
            .map(|(_, _, record)| record)
    }
    /// The completed record of an object, leaving the memory-map entry in
    /// place so a later occurrence of the same object (a shared DAG node)
    /// still resolves by index.
    fn take_record(&self, object: &JsObject) -> Option<SerializedRecord> {
        self.get_serialized(object).cloned()
    }
    fn get_deserialized(&self, index: &usize) -> Option<JsObject> {
        self.deserialized.get(index).cloned()
    }
    /// Assign the next memory index to a deserialized object (the record
    /// types the serialization indexes, in the same pre-order).
    fn reserve_deserialized_index(&mut self, object: JsObject) {
        let index = self.next_deserialized_index;
        self.next_deserialized_index += 1;
        self.deserialized.insert(index, object);
    }
    fn transferred_value(&self, index: usize) -> Option<JsValue> {
        self.transferred_values.get(index).cloned()
    }
}

// Bridge: JsValue → PrimitiveValue

/// Convert a JsValue to its portable PrimitiveValue representation.
fn js_value_to_primitive(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<PrimitiveValue> {
    if Types::value_is_undefined(value) {
        Some(PrimitiveValue::Undefined)
    } else if Types::value_is_null(value) {
        Some(PrimitiveValue::Null)
    } else if let Some(b) = Types::value_as_bool(value) {
        Some(PrimitiveValue::Boolean(b))
    } else if let Some(n) = Types::value_as_number(value) {
        Some(PrimitiveValue::Number(n))
    } else if let Some(s) = Types::value_as_string(value) {
        let rust_str = ec.js_string_to_rust_string(&s);
        Some(PrimitiveValue::String(rust_str.encode_utf16().collect()))
    } else if let Some(_bi) = Types::value_as_bigint(value) {
        // BigInt value → decimal string via ToString
        ec.to_rust_string(value.clone())
            .ok()
            .map(PrimitiveValue::BigInt)
    } else {
        // Symbol or Object — not a primitive value
        None
    }
}

/// Disambiguated wrapper for `ExecutionContext::get` with PropertyKey.
fn get_property_key(
    ec: &mut dyn ExecutionContext<Types>,
    object: <Types as JsTypes>::JsObject,
    key: <Types as JsTypes>::PropertyKey,
) -> Completion<<Types as JsTypes>::JsValue, Types> {
    ExecutionContext::get(ec, object, key)
}

/// Convert a PropertyKey to UTF-16 code units (for use in serialized property lists).
fn property_key_to_string(
    key: &PropertyKey,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<u16>, Types> {
    let key_str = ec.property_key_to_rust_string(key);
    // If the key string starts with "Symbol(", it's a Symbol key → DataCloneError.
    if key_str.starts_with("Symbol(") {
        return Err(crate::webidl::data_clone_error_value(ec));
    }
    Ok(key_str.encode_utf16().collect())
}

// StructuredSerializeInternal

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
fn structured_serialize_internal(
    value: &JsValue,
    for_storage: bool,
    memory: &mut MemoryMap,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 1: If memory was not supplied, let memory be an empty map.
    //         (memory is always supplied here.)
    // Step 2: If memory[value] exists, then return memory[value].
    if let Some(index) =
        Types::value_as_object(value).and_then(|object| memory.get_serialized_index(&object))
    {
        // The object was already serialized (a cycle or a shared DAG node);
        // reference it by its memory index.
        return Ok(SerializedRecord::BackReference(index));
    }

    // Step 3: Let deep be false.
    // (This implementation handles deep serialization inline in the helper functions
    //  rather than via a unified deep flag — see serialize_map_contents, serialize_set_contents,
    //  serialize_array, and the ordinary object path below.)
    // Step 4: If value is undefined, null, a Boolean, a Number, a BigInt, or a String,
    //         then return { [[Type]]: "primitive", [[Value]]: value }.
    if let Some(prim) = js_value_to_primitive(value, ec) {
        return Ok(SerializedRecord::Primitive(prim));
    }

    // Step 5: If value is a Symbol, then throw a "DataCloneError" DOMException.
    if Types::value_as_symbol(value).is_some() {
        return Err(crate::webidl::data_clone_error_value(ec));
    }

    // Step 6: Let serialized be an uninitialized value.
    // (Implemented via individual return values in each branch below.)
    let object = Types::value_as_object(value)
        .ok_or_else(|| ec.new_type_error("unexpected non-object value in serialize"))?;

    // Step 7: If value has a [[BooleanData]] internal slot, then set serialized to
    //           { [[Type]]: "Boolean", [[BooleanData]]: value.[[BooleanData]] }.
    if let Some(b) = Types::boolean_wrapper_data(&object) {
        return Ok(SerializedRecord::Boolean(b));
    }

    // Step 8: Otherwise, if value has a [[NumberData]] internal slot, then set serialized to
    //           { [[Type]]: "Number", [[NumberData]]: value.[[NumberData]] }.
    if let Some(n) = Types::number_wrapper_data(&object) {
        return Ok(SerializedRecord::Number(n));
    }

    // Step 9: Otherwise, if value has a [[BigIntData]] internal slot, then set serialized to
    //           { [[Type]]: "BigInt", [[BigIntData]]: value.[[BigIntData]] }.
    if let Some(bi) = Types::bigint_wrapper_data(&object) {
        let bi_val = Types::value_from_bigint(bi);
        let bi_str = ec.to_rust_string(bi_val)?;
        return Ok(SerializedRecord::BigInt(bi_str));
    }

    // Step 10: Otherwise, if value has a [[StringData]] internal slot, then set serialized to
    //            { [[Type]]: "String", [[StringData]]: value.[[StringData]] }.
    if let Some(s) = Types::string_wrapper_data(&object) {
        let rust_str = ec.js_string_to_rust_string(&s);
        return Ok(SerializedRecord::String(rust_str.encode_utf16().collect()));
    }

    // Step 11: Otherwise, if value has a [[DateValue]] internal slot, then set serialized to
    //            { [[Type]]: "Date", [[DateValue]]: value.[[DateValue]] }.
    if Types::object_is_date(&object) {
        let ms = ec.get_date_value(&object)?;
        return Ok(SerializedRecord::Date(ms));
    }

    // Step 12: Otherwise, if value has a [[RegExpMatcher]] internal slot, then set serialized to
    //            { [[Type]]: "RegExp", [[RegExpMatcher]]: value.[[RegExpMatcher]],
    //              [[OriginalSource]]: value.[[OriginalSource]],
    //              [[OriginalFlags]]: value.[[OriginalFlags]] }.
    if Types::object_is_regexp(&object) {
        // Per spec, we must store [[OriginalSource]] and [[OriginalFlags]], not
        // the escaped source getter (EscapeRegExpPattern). Since the only escaping
        // EscapeRegExpPattern does is prefixing "/" with "\", we unescape by
        // removing the leading "\" on "/".
        let escaped_source = ec.get_regexp_source(&object)?;
        let source = unescape_regexp_source(&escaped_source);
        let flags = ec.get_regexp_flags(&object)?;
        return Ok(SerializedRecord::RegExp { source, flags });
    }

    // Step 13: Otherwise, if value has an [[ArrayBufferData]] internal slot.
    // Step 13.1: If IsSharedArrayBuffer(value) is true:
    if let Some(sab) = Types::object_as_shared_array_buffer(&object) {
        return serialize_shared_array_buffer(&sab, for_storage, ec);
    }

    // Step 13.2: Otherwise (non-shared ArrayBuffer):
    if let Some(buffer) = Types::object_as_array_buffer(&object) {
        return serialize_array_buffer(&buffer, ec);
    }

    // Step 14: Otherwise, if value has a [[ViewedArrayBuffer]] internal slot.
    if let Some(dv) = Types::object_as_data_view(&object) {
        return serialize_dataview(&dv, for_storage, memory, ec);
    }
    if let Some(ta) = Types::object_as_typed_array(&object) {
        return serialize_typed_array(&ta, for_storage, memory, ec);
    }

    // Step 15: Otherwise, if value has a [[MapData]] internal slot.
    if let Some(map) = Types::object_as_map(&object) {
        return serialize_map_contents(&map, for_storage, memory, &object, ec);
    }

    // Step 16: Otherwise, if value has a [[SetData]] internal slot.
    if let Some(set_type) = Types::object_as_set(&object) {
        return serialize_set_contents(&set_type, for_storage, memory, &object, ec);
    }

    // Step 17: Otherwise, if value has an [[ErrorData]] internal slot and value is not a platform object.
    if Types::object_is_error(&object) {
        return serialize_error(&object, ec);
    }

    // Step 18: Otherwise, if value is an Array exotic object.
    if ec.is_array(value)? {
        return serialize_array(value, for_storage, memory, ec);
    }

    // Step 19: Otherwise, if value is a platform object that is a serializable object.
    // TODO: Check registered [Serializable] platform objects.
    // Step 20: Otherwise, if value is a platform object, then throw a "DataCloneError" DOMException.
    // TODO: Add platform object detection.

    // Step 21: Otherwise, if IsCallable(value) is true, then throw a "DataCloneError" DOMException.
    if ec.is_callable(value) {
        return Err(crate::webidl::data_clone_error_value(ec));
    }

    // Step 22: Otherwise, if value has any internal slot other than [[Prototype]], [[Extensible]],
    //            or [[PrivateElements]], then throw a "DataCloneError" DOMException.
    // TODO: Add these checks.
    // Step 23: Otherwise, if value is an exotic object and value is not the %Object.prototype%
    //            intrinsic object associated with any realm, then throw a "DataCloneError" DOMException.
    // TODO: Add these checks.

    // Step 24 (spec's final fallback): Otherwise, set serialized to
    //   { [[Type]]: "Object", [[Properties]]: a new empty List }.
    // Step 25: Set memory[value] to serialized.
    let serialized = SerializedRecord::Object(Vec::new());
    memory.insert_serialized(&object, serialized);

    // Step 26 ("If deep is true" block for the Object case):
    //   For each key in ! EnumerableOwnProperties(value, key):
    //     If ! HasOwnProperty(value, key) is true:
    //       Let inputValue be ? value.[[Get]](key, value).
    //       Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory).
    //       Append (key, outputValue) to serialized.[[Properties]].
    let keys = ec.own_property_keys(object.clone())?;
    let mut properties = Vec::new();
    for key in keys {
        if !ec.has_own_property(object.clone(), key.clone())? {
            continue;
        }
        // Check enumerability per EnumerableOwnProperties.
        let desc = ec.get_own_property(object.clone(), key.clone())?;
        let enumerable = desc.as_ref().and_then(|d| d.enumerable).unwrap_or(false);
        if !enumerable {
            continue;
        }

        // Step 26: "Let inputValue be ? value.[[Get]](key, value)."
        let input_value = get_property_key(ec, object.clone(), key.clone())?;

        // Step 26: "Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory)."
        let output_value = structured_serialize_internal(&input_value, for_storage, memory, ec)?;
        let key_str = property_key_to_string(&key, ec)?;
        properties.push((key_str, output_value));
    }

    // Step 28 (spec): Return serialized.
    if let Some(SerializedRecord::Object(props)) = memory.get_serialized_mut(&object) {
        *props = properties;
    }
    Ok(memory
        .take_record(&object)
        .expect("entry must exist in memory"))
}

// Serialization helpers

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 13.2: non-shared ArrayBuffer.
fn serialize_array_buffer(
    buffer: &ArrayBuffer,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 13.2.1: If IsDetachedBuffer(value) is true, then throw a "DataCloneError" DOMException.
    let data = ec
        .array_buffer_data(buffer)
        .ok_or_else(|| crate::webidl::data_clone_error_value(ec))?;

    // Step 13.2.2: Let size be value.[[ArrayBufferByteLength]].
    let size = data.len() as u64;

    // Step 13.2.3: Let dataCopy be ? CreateByteDataBlock(size).
    //            Perform CopyDataBlockBytes(dataCopy, 0, value.[[ArrayBufferData]], 0, size).
    let data_copy = data.to_vec();

    // Step 13.2.4: If value has an [[ArrayBufferMaxByteLength]] internal slot, then
    //                set serialized to { [[Type]]: "ResizableArrayBuffer", [[ArrayBufferData]]: dataCopy,
    //                                     [[ArrayBufferByteLength]]: size, [[ArrayBufferMaxByteLength]]: value.[[ArrayBufferMaxByteLength]] }.
    // TODO: Support resizable ArrayBuffers.
    // Step 13.2.5: Otherwise, set serialized to { [[Type]]: "ArrayBuffer", [[ArrayBufferData]]: dataCopy,
    //                                             [[ArrayBufferByteLength]]: size }.
    Ok(SerializedRecord::ArrayBuffer {
        data: data_copy,
        byte_length: size,
        max_byte_length: None,
    })
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 13.1: SharedArrayBuffer.
fn serialize_shared_array_buffer(
    sab: &SharedArrayBuffer,
    for_storage: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 13.1.1: If IsSharedArrayBuffer(value) is true:
    //   Step 13.1.1.1: If the current settings object's cross-origin isolated capability is false,
    //                    then throw a "DataCloneError" DOMException.
    // TODO: Check cross-origin isolated capability.
    // Step 13.1.2: If forStorage is true, then throw a "DataCloneError" DOMException.
    if for_storage {
        return Err(crate::webidl::data_clone_error_value(ec));
    }

    // Step 13.1.3: If value has an [[ArrayBufferMaxByteLength]] internal slot, then
    //                set serialized to { [[Type]]: "GrowableSharedArrayBuffer", ... }.
    // TODO: Support GrowableSharedArrayBuffer.
    // Step 13.1.4: Otherwise, set serialized to { [[Type]]: "SharedArrayBuffer",
    //               [[ArrayBufferData]]: value.[[ArrayBufferData]],
    //               [[ArrayBufferByteLength]]: value.[[ArrayBufferByteLength]],
    //               [[AgentCluster]]: the surrounding agent's agent cluster }.
    // Copy raw bytes for IPC portability.
    // Extract byte length from the SAB object.
    let sab_obj = Types::object_from_shared_array_buffer(sab.clone());
    let buffer_val = EcmascriptHost::get(ec, &sab_obj, "byteLength")?;
    let _byte_length = ec.to_number(buffer_val)? as u64;
    // TODO: extract raw data from SharedArrayBuffer.
    let data = Vec::new();
    let agent_cluster = String::from("default"); // TODO: use actual agent cluster.
    Ok(SerializedRecord::SharedArrayBuffer {
        data,
        agent_cluster,
    })
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 14: Otherwise, if value has a [[ViewedArrayBuffer]] internal slot.
/// Step 14.1: If IsArrayBufferViewOutOfBounds(value) is true, then throw a "DataCloneError" DOMException.
fn serialize_dataview(
    dataview: &DataView,
    for_storage: bool,
    memory: &mut MemoryMap,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // TODO: Check IsArrayBufferViewOutOfBounds.

    // Step 14.2: Let buffer be the value of value's [[ViewedArrayBuffer]] internal slot.
    let buffer = ec.data_view_buffer(dataview)?;
    let buffer_obj = Types::object_from_array_buffer(buffer.clone());

    // Step 14.3: Let bufferSerialized be ? StructuredSerializeInternal(buffer, forStorage, memory).
    let buffer_val = Types::value_from_object(buffer_obj);
    let buffer_serialized = structured_serialize_internal(&buffer_val, for_storage, memory, ec)?;

    // Step 14.4: If value has a [[DataView]] internal slot, then set serialized to
    //              { [[Type]]: "ArrayBufferView", [[Constructor]]: "DataView",
    //                [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: value.[[ByteLength]],
    //                [[ByteOffset]]: value.[[ByteOffset]] }.
    let byte_length = ec.data_view_byte_length(dataview)?;
    let byte_offset = ec.data_view_byte_offset(dataview)?;

    Ok(SerializedRecord::ArrayBufferView {
        constructor: String::from("DataView"),
        buffer_serialized: Box::new(buffer_serialized),
        byte_length,
        byte_offset,
        array_length: None,
    })
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 14 (continued): TypedArray branch.
fn serialize_typed_array(
    typed_array: &TypedArray,
    for_storage: bool,
    memory: &mut MemoryMap,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // TODO: Check IsArrayBufferViewOutOfBounds.

    // Step 14.2: Let buffer be the value of value's [[ViewedArrayBuffer]] internal slot.
    let buffer = ec.typed_array_buffer(typed_array)?;

    // Step 14.3: Let bufferSerialized be ? StructuredSerializeInternal(buffer, forStorage, memory).
    let buffer_obj = Types::object_from_array_buffer(buffer);
    let buffer_val = Types::value_from_object(buffer_obj);
    let buffer_serialized = structured_serialize_internal(&buffer_val, for_storage, memory, ec)?;

    // Step 14.5 (spec numbering): Otherwise (value does not have a [[DataView]] internal slot):
    //   Step 14.5.1: Assert: value has a [[TypedArrayName]] internal slot.
    let element_type = ec
        .typed_array_element_type(typed_array)
        .ok_or_else(|| ec.new_type_error("TypedArray has no kind"))?;
    let constructor = typed_array_kind_name(element_type);

    //   Step 14.5.2: Set serialized to { [[Type]]: "ArrayBufferView", [[Constructor]]: value.[[TypedArrayName]],
    //                   [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: value.[[ByteLength]],
    //                   [[ByteOffset]]: value.[[ByteOffset]], [[ArrayLength]]: value.[[ArrayLength]] }.
    let byte_length = ec.typed_array_byte_length(typed_array)?;
    let byte_offset = ec.typed_array_byte_offset(typed_array)?;
    // TypedArray length = byte_length / element byte size.
    let element_size = typed_array_element_byte_size(element_type) as u64;
    let array_length = if byte_length > 0 {
        byte_length / element_size
    } else {
        0
    };

    Ok(SerializedRecord::ArrayBufferView {
        constructor,
        buffer_serialized: Box::new(buffer_serialized),
        byte_length,
        byte_offset,
        array_length: Some(array_length),
    })
}

/// Return the byte size of a single element of the given TypedArray element type.
fn typed_array_element_byte_size(element_type: TypedArrayElementType) -> u8 {
    use TypedArrayElementType::*;
    match element_type {
        Int8 | Uint8 | Uint8Clamped => 1,
        Int16 | Uint16 => 2,
        Int32 | Uint32 | Float32 => 4,
        Float16 => 2,
        BigInt64 | BigUint64 | Float64 => 8,
    }
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 15: Otherwise, if value has a [[MapData]] internal slot.
///   Step 15.1: Set serialized to { [[Type]]: "Map", [[MapData]]: a new empty List }.
///   Step 15.2: Set memory[value] to serialized. (for cycle detection)
///   Step 15.3: Let copiedList be a new empty List.
///   Step 15.4: For each Record { [[Key]], [[Value]] } entry of value.[[MapData]]:
///     If entry.[[Key]] is not the special value empty, append a copy to copiedList.
///   Step 15.5: For each Record { [[Key]], [[Value]] } entry of copiedList:
///     Step 15.5.1: Let serializedKey be ? StructuredSerializeInternal(entry.[[Key]], forStorage, memory).
///     Step 15.5.2: Let serializedValue be ? StructuredSerializeInternal(entry.[[Value]], forStorage, memory).
///     Step 15.5.3: Append { [[Key]]: serializedKey, [[Value]]: serializedValue } to serialized.[[MapData]].
fn serialize_map_contents(
    map: &MapType,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 15.1: Set serialized to { [[Type]]: "Map", [[MapData]]: a new empty List }.
    let serialized = SerializedRecord::Map(Vec::new());

    // Step 15.2: Set memory[value] to serialized.
    memory.insert_serialized(object, serialized);

    // Step 15.3: Let copiedList be a new empty List.
    // Step 15.4: For each Record { [[Key]], [[Value]] } entry of value.[[MapData]]:
    let raw_entries = ec.map_get_entries(map)?;

    // Step 15.5: For each Record { [[Key]], [[Value]] } entry of copiedList:
    let mut entries = Vec::new();
    for (key, val) in raw_entries {
        // Step 15.5.1: Let serializedKey be ? StructuredSerializeInternal(entry.[[Key]], forStorage, memory).
        let sk = structured_serialize_internal(&key, for_storage, memory, ec)?;

        // Step 15.5.2: Let serializedValue be ? StructuredSerializeInternal(entry.[[Value]], forStorage, memory).
        let sv = structured_serialize_internal(&val, for_storage, memory, ec)?;

        // Step 15.5.3: Append { [[Key]]: serializedKey, [[Value]]: serializedValue } to serialized.[[MapData]].
        entries.push((sk, sv));
    }

    // Update the entry in memory with the serialized entries.
    if let Some(SerializedRecord::Map(entry_list)) = memory.get_serialized_mut(object) {
        *entry_list = entries;
    }
    Ok(memory
        .take_record(object)
        .expect("Map entry must exist in memory"))
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 16: Otherwise, if value has a [[SetData]] internal slot.
///   Step 16.1: Set serialized to { [[Type]]: "Set", [[SetData]]: a new empty List }.
///   Step 16.2: Set memory[value] to serialized. (for cycle detection)
///   Step 16.3: Let copiedList be a new empty List.
///   Step 16.4: For each entry of value.[[SetData]]:
///     If entry is not the special value empty, append entry to copiedList.
///   Step 16.5: For each entry of copiedList:
///     Step 16.5.1: Let serializedEntry be ? StructuredSerializeInternal(entry, forStorage, memory).
///     Step 16.5.2: Append serializedEntry to serialized.[[SetData]].
fn serialize_set_contents(
    set_type: &SetType,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 16.1: Set serialized to { [[Type]]: "Set", [[SetData]]: a new empty List }.
    let serialized = SerializedRecord::Set(Vec::new());

    // Step 16.2: Set memory[value] to serialized.
    memory.insert_serialized(object, serialized);

    // Step 16.3: Let copiedList be a new empty List.
    // Step 16.4: For each entry of value.[[SetData]]:
    let raw_entries = ec.set_get_values(set_type)?;

    // Step 16.5: For each entry of copiedList:
    let mut entries = Vec::new();
    for entry in raw_entries {
        // Step 16.5.1: Let serializedEntry be ? StructuredSerializeInternal(entry, forStorage, memory).
        let sv = structured_serialize_internal(&entry, for_storage, memory, ec)?;

        // Step 16.5.2: Append serializedEntry to serialized.[[SetData]].
        entries.push(sv);
    }

    if let Some(SerializedRecord::Set(entry_list)) = memory.get_serialized_mut(object) {
        *entry_list = entries;
    }
    Ok(memory
        .take_record(object)
        .expect("Set entry must exist in memory"))
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 17: Otherwise, if value has an [[ErrorData]] internal slot and value is not a platform object.
fn serialize_error(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    // Step 17.1: Let name be ? Get(value, "name").
    let name_val = EcmascriptHost::get(ec, object, "name")?;

    // Step 17.2: If name is not a String value, then set name to "Error".
    let name = if Types::value_as_string(&name_val).is_some() {
        ec.to_rust_string(name_val)?
    } else {
        String::from("Error")
    };

    // Step 17.3: If name is not one of "Error", "EvalError", "RangeError", "ReferenceError",
    //              "SyntaxError", "TypeError", or "URIError", then set name to "Error".
    let name = normalize_error_name(&name);

    // Step 17.4: Let valueMessageDesc be ? value.[[GetOwnProperty]]("message").
    // Step 17.5: Let message be undefined if IsDataDescriptor(valueMessageDesc) is false,
    //              and ? ToString(valueMessageDesc.[[Value]]) otherwise.
    let msg_key = ec.property_key_from_str("message");
    let msg_desc = ec.get_own_property(object.clone(), msg_key)?;
    let message: Option<String> = match msg_desc {
        Some(ref desc) if desc.value.is_some() => desc
            .value
            .clone()
            .map(|v| ec.to_rust_string(v))
            .transpose()?,
        _ => None,
    };

    // Step 17.6: Let stack be an implementation-defined string that represents value.[[Stack]].
    let stack = EcmascriptHost::get(ec, object, "stack")
        .ok()
        .and_then(|v| ec.to_rust_string(v).ok())
        .unwrap_or_default();

    // Step 17.7: Set serialized to { [[Type]]: "Error", [[Name]]: name, [[Message]]: message, [[Stack]]: stack }.
    // Additionally, serialize [[ErrorData]].[[Cause]] per ES2022.
    let cause_key = ec.property_key_from_str("cause");
    let cause_desc = ec.get_own_property(object.clone(), cause_key)?;
    let cause = cause_desc
        .as_ref()
        .and_then(|d| {
            d.value.clone().and_then(|val| {
                // cause is serialized as a sub-serialization. Use a fresh memory map for
                // the self-contained cause value.
                let mut cause_memory = MemoryMap::default();
                structured_serialize_internal(&val, false, &mut cause_memory, ec).ok()
            })
        })
        .map(Box::new);

    Ok(SerializedRecord::Error {
        name,
        message,
        stack,
        cause,
    })
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 18: Otherwise, if value is an Array exotic object.
///   Step 18.1: Let valueLenDescriptor be ? OrdinaryGetOwnProperty(value, "length").
///   Step 18.2: Let valueLen be valueLenDescriptor.[[Value]].
///   Step 18.3: Set serialized to { [[Type]]: "Array", [[Length]]: valueLen, [[Properties]]: a new empty List }.
///   Step 18.4: Set memory[value] to serialized. (for cycle detection)
///   Step 18.5: For each key of ! EnumerableOwnProperties(value, key):
///     If ! HasOwnProperty(value, key) is true:
///       Let inputValue be ? value.[[Get]](key, value).
///       Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory).
///       Append (key, outputValue) to serialized.[[Properties]].
fn serialize_array(
    value: &JsValue,
    for_storage: bool,
    memory: &mut MemoryMap,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    let object = Types::value_as_object(value)
        .ok_or_else(|| ec.new_type_error("expected object for array serialize"))?;

    // Step 18.1-2: Get length via own property descriptor.
    let length_key = ec.property_key_from_str("length");
    let length_desc = ec
        .get_own_property(object.clone(), length_key)?
        .ok_or_else(|| ec.new_type_error("Array has no length"))?;
    let length = length_desc
        .value
        .as_ref()
        .and_then(|v| Types::value_as_number(v).map(|n| n as u64))
        .ok_or_else(|| ec.new_type_error("Array length is not a number"))?;

    // Step 18.3: Set serialized to { [[Type]]: "Array", [[Length]]: valueLen, [[Properties]]: a new empty List }.
    let serialized = SerializedRecord::Array {
        length,
        properties: Vec::new(),
    };

    // Step 18.4: Set memory[value] to serialized.
    memory.insert_serialized(&object, serialized);

    // Step 18.5: For each key of ! EnumerableOwnProperties(value, key):
    let keys = ec.own_property_keys(object.clone())?;
    let mut properties = Vec::new();
    for key in keys {
        // If ! HasOwnProperty(value, key) is true:
        if !ec.has_own_property(object.clone(), key.clone())? {
            continue;
        }
        // Check enumerability per EnumerableOwnProperties.
        let desc = ec.get_own_property(object.clone(), key.clone())?;
        let enumerable = desc.as_ref().and_then(|d| d.enumerable).unwrap_or(false);
        if !enumerable {
            continue;
        }

        // Let inputValue be ? value.[[Get]](key, value).
        let input_value = get_property_key(ec, object.clone(), key.clone())?;
        // Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory).
        let output_value = structured_serialize_internal(&input_value, for_storage, memory, ec)?;
        let key_str = property_key_to_string(&key, ec)?;
        properties.push((key_str, output_value));
    }

    if let Some(SerializedRecord::Array {
        properties: props, ..
    }) = memory.get_serialized_mut(&object)
    {
        *props = properties;
    }
    Ok(memory
        .take_record(&object)
        .expect("Array entry must exist in memory"))
}

// Public API: simple serialize entry points

/// <https://html.spec.whatwg.org/#structuredserialize>
pub fn structured_serialize(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    let mut memory = MemoryMap::default();
    structured_serialize_internal(value, false, &mut memory, ec)
}

/// <https://html.spec.whatwg.org/#structuredserializeforstorage>
pub fn structured_serialize_for_storage(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializedRecord, Types> {
    let mut memory = MemoryMap::default();
    structured_serialize_internal(value, true, &mut memory, ec)
}

// StructuredSerializeWithTransfer

/// Represents the result of [`structured_serialize_with_transfer`].
pub struct SerializeWithTransferResult {
    pub serialized: SerializedRecord,
    pub transfer_data_holders: Vec<TransferDataHolder>,
}

/// <https://html.spec.whatwg.org/#structuredserializewithtransfer>
pub fn structured_serialize_with_transfer(
    value: &JsValue,
    transfer_list: Vec<JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SerializeWithTransferResult, Types> {
    // Step 1: Let memory be an empty map.
    let mut memory = MemoryMap::default();

    // Step 2: For each transferable of transferList:
    for (index, transferable) in transfer_list.iter().enumerate() {
        let Some(object) = Types::value_as_object(transferable) else {
            // If transferable has neither an [[ArrayBufferData]] internal slot nor a
            // [[Detached]] internal slot, then throw a "DataCloneError" DOMException.
            return Err(crate::webidl::data_clone_error_value(ec));
        };
        let has_ab = Types::object_as_array_buffer(&object).is_some();
        let has_sab = Types::object_as_shared_array_buffer(&object).is_some();

        // Step 2.1: If transferable has neither an [[ArrayBufferData]] internal slot nor a
        //             [[Detached]] internal slot, then throw.
        if !has_ab && !has_sab && !messageport::is_transferable_platform_object(&object, ec) {
            return Err(crate::webidl::data_clone_error_value(ec));
        }

        // Step 2.2: If transferable has an [[ArrayBufferData]] internal slot and
        //             IsSharedArrayBuffer(transferable) is true, then throw.
        if has_sab {
            return Err(crate::webidl::data_clone_error_value(ec));
        }

        // Step 2.3: If memory[transferable] exists, then throw.
        if memory.get_serialized(&object).is_some() {
            return Err(crate::webidl::data_clone_error_value(ec));
        }

        // Step 2.4: Set memory[transferable] to { [[Type]]: an uninitialized value }.
        // Note: The spec's "uninitialized" dataHolder record is represented by a
        // transfer-list index so the graph stays IPC-serializable (see the
        // `TransferredValue` variant).  Step 5 fills the actual holder list.
        memory.insert_serialized(&object, SerializedRecord::TransferredValue(index));
    }

    // Step 3: Let serialized be ? StructuredSerializeInternal(value, false, memory).
    let serialized = structured_serialize_internal(value, false, &mut memory, ec)?;

    // Step 4: Let transferDataHolders be a new empty List.
    let mut transfer_data_holders = Vec::new();

    // Step 5: For each transferable of transferList:
    for transferable in &transfer_list {
        let object = Types::value_as_object(transferable)
            .ok_or_else(|| crate::webidl::data_clone_error_value(ec))?;
        if let Some(buffer) = Types::object_as_array_buffer(&object) {
            // Step 5.1: If transferable has an [[ArrayBufferData]] internal slot:
            //   Step 5.1.1: If IsDetachedBuffer(transferable) is true, then throw.
            let data = ec
                .array_buffer_data(&buffer)
                .ok_or_else(|| crate::webidl::data_clone_error_value(ec))?;
            let byte_length = data.len() as u64;

            // TODO: Check for [[ArrayBufferMaxByteLength]] (ResizableArrayBuffer case).
            // Step 5.1.x: Set dataHolder.[[ArrayBufferData]] to transferable.[[ArrayBufferData]]
            //             and [[ArrayBufferByteLength]] to transferable.[[ArrayBufferByteLength]].
            let data_copy = data.to_vec();

            // Step 5.1.4: Perform ? DetachArrayBuffer(transferable).
            ec.detach_array_buffer(buffer, None)
                .map_err(|_| crate::webidl::data_clone_error_value(ec))?;

            transfer_data_holders.push(TransferDataHolder::ArrayBuffer {
                data: data_copy,
                byte_length,
                max_byte_length: None,
            });
        } else {
            // Step 5.2: Otherwise (platform object with a [[Detached]]
            //           internal slot): perform the transfer steps for the
            //           interface identified by interfaceName.
            // Note: Only MessagePort is transferable; the steps run in
            // messageport::transfer_steps, which reads the port out of its
            // platform object, builds its data holder, and runs the transfer
            // steps on it (the port's record leaves this realm there, so the
            // source object's [[Detached]] state is implicit).
            transfer_data_holders.push(messageport::transfer_steps(&object, ec)?);
        }
    }

    // Step 6: Return { [[Serialized]]: serialized, [[TransferDataHolders]]: transferDataHolders }.
    Ok(SerializeWithTransferResult {
        serialized,
        transfer_data_holders,
    })
}

// StructuredDeserialize

/// <https://html.spec.whatwg.org/#structureddeserialize>
fn structured_deserialize(
    serialized: &SerializedRecord,
    _target_realm: &JsValue,
    memory: &mut MemoryMap,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: If memory was not supplied, let memory be an empty map.
    //         (memory is always supplied here.)
    // Step 2: If memory[serialized] exists, then return memory[serialized].
    if let SerializedRecord::BackReference(index) = serialized {
        let object = memory
            .get_deserialized(index)
            .ok_or_else(|| ec.new_type_error("structured clone: unresolved back reference"))?;
        return Ok(Types::value_from_object(object));
    }

    // Step 3: Let deep be false.
    let mut deep = false;

    // Step 4: Let value be an uninitialized value.
    let value: JsValue;

    // Get realm intrinsics for constructing objects.
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);

    match serialized {
        // Step 5: If serialized.[[Type]] is "primitive", then set value to serialized.[[Value]].
        SerializedRecord::Primitive(p) => {
            value = deserialize_primitive_value(p, ec)?;
        }

        // The transfer-list reference resolves to the value rebuilt from the
        // corresponding transfer data holder (StructuredDeserializeWithTransfer
        // step 3).
        SerializedRecord::TransferredValue(index) => {
            value = memory
                .transferred_value(*index)
                .ok_or_else(|| ec.new_type_error("missing transferred value"))?;
        }

        // Step 6: Otherwise, if serialized.[[Type]] is "Boolean", then set value to a new Boolean
        //           object in targetRealm whose [[BooleanData]] internal slot value is
        //           serialized.[[BooleanData]].
        SerializedRecord::Boolean(b) => {
            let bool_val = ec.value_from_bool(*b);
            let obj = ec.construct(intrinsics.boolean.clone(), &[bool_val], None)?;
            value = Types::value_from_object(obj);
        }

        // Step 7: Otherwise, if serialized.[[Type]] is "Number", then set value to a new Number
        //           object in targetRealm whose [[NumberData]] internal slot value is
        //           serialized.[[NumberData]].
        SerializedRecord::Number(n) => {
            let num_val = ec.value_from_number(*n);
            let obj = ec.construct(intrinsics.number.clone(), &[num_val], None)?;
            value = Types::value_from_object(obj);
        }

        // Step 8: Otherwise, if serialized.[[Type]] is "BigInt", then set value to a new BigInt
        //           object in targetRealm whose [[BigIntData]] internal slot value is
        //           serialized.[[BigIntData]].
        SerializedRecord::BigInt(s) => {
            // Create a BigInt wrapper via Object(BigInt(string)).
            let bi_val = ec
                .string_to_bigint(ec.js_string_from_str(s))
                .map(Types::value_from_bigint)
                .unwrap_or_else(|| ec.value_from_number(0.0));

            // Use Object() constructor to wrap the BigInt primitive.
            let obj = ec.construct(intrinsics.object.clone(), &[bi_val], None)?;
            value = Types::value_from_object(obj);
        }

        // Step 9: Otherwise, if serialized.[[Type]] is "String", then set value to a new String
        //           object in targetRealm whose [[StringData]] internal slot value is
        //           serialized.[[StringData]].
        SerializedRecord::String(s) => {
            let str_val = ec.value_from_string(ec.js_string_from_str(&String::from_utf16_lossy(s)));
            let obj = ec.construct(intrinsics.string.clone(), &[str_val], None)?;
            value = Types::value_from_object(obj);
        }

        // Step 10: Otherwise, if serialized.[[Type]] is "Date", then set value to a new Date
        //            object in targetRealm whose [[DateValue]] internal slot value is
        //            serialized.[[DateValue]].
        SerializedRecord::Date(ms) => {
            let date = ec.construct(intrinsics.date.clone(), &[], None)?;
            // Call date.setTime(ms) to set the time.
            let set_time_val = EcmascriptHost::get(ec, &date, "setTime")?;
            let set_time_fn = Types::value_as_object(&set_time_val)
                .ok_or_else(|| ec.new_type_error("Date.setTime not callable"))?;
            let ms_val = ec.value_from_number(*ms);
            let date_val = Types::value_from_object(date.clone());
            EcmascriptHost::call(ec, &set_time_fn, &date_val, &[ms_val])?;
            value = date_val;
        }

        // Step 11: Otherwise, if serialized.[[Type]] is "RegExp", then set value to a new RegExp
        //            object in targetRealm whose [[RegExpMatcher]] internal slot value is
        //            serialized.[[RegExpMatcher]], whose [[OriginalSource]] internal slot value is
        //            serialized.[[OriginalSource]], and whose [[OriginalFlags]] internal slot value
        //            is serialized.[[OriginalFlags]].
        SerializedRecord::RegExp { source, flags } => {
            let src_val = ec.value_from_string(ec.js_string_from_str(source));
            let flags_val = ec.value_from_string(ec.js_string_from_str(flags));
            let regexp = ec.construct(intrinsics.regexp.clone(), &[src_val, flags_val], None)?;
            value = Types::value_from_object(regexp);
        }

        // Step 13: Otherwise, if serialized.[[Type]] is "SharedArrayBuffer":
        SerializedRecord::SharedArrayBuffer {
            data,
            agent_cluster: _,
        } => {
            // TODO: check agent cluster.
            // Create SharedArrayBuffer via the constructor.
            let sab_len_val = ec.value_from_number(data.len() as f64);
            let sab_obj =
                ec.construct(intrinsics.shared_array_buffer.clone(), &[sab_len_val], None)?;
            value = Types::value_from_object(sab_obj);
        }

        // Step 14: Otherwise, if serialized.[[Type]] is "ArrayBuffer", then set value to a new
        //            ArrayBuffer object in targetRealm whose [[ArrayBufferData]] internal slot
        //            value is serialized.[[ArrayBufferData]], and whose [[ArrayBufferByteLength]]
        //            internal slot value is serialized.[[ArrayBufferByteLength]].
        //   If this throws an exception, catch it, and then throw a "DataCloneError" DOMException.
        SerializedRecord::ArrayBuffer {
            data: data_copy,
            byte_length: _,
            ..
        } => {
            // Allocate an ArrayBuffer of the right size and fill it with data.
            let buf = ec
                .allocate_array_buffer(
                    intrinsics.array_buffer.clone(),
                    data_copy.len() as u64,
                    None,
                )
                .map_err(|_| crate::webidl::data_clone_error_value(ec))?;
            // Write data byte by byte using set_value_in_buffer with Uint8.
            for (i, byte) in data_copy.iter().enumerate() {
                let byte_val = ec.value_from_number(*byte as f64);
                ec.set_value_in_buffer(
                    &buf,
                    i as u64,
                    TypedArrayElementType::Uint8,
                    byte_val,
                    false,
                    js_engine::enums::SharedMemoryOrder::Unordered,
                )?;
            }
            let buf_obj = Types::object_from_array_buffer(buf);
            value = Types::value_from_object(buf_obj);
        }

        // Step 16: Otherwise, if serialized.[[Type]] is "ArrayBufferView":
        SerializedRecord::ArrayBufferView {
            constructor,
            buffer_serialized,
            byte_length,
            byte_offset,
            array_length: _,
        } => {
            //   Let deserializedArrayBuffer be ?
            //     StructuredDeserialize(serialized.[[ArrayBufferSerialized]], targetRealm, memory).
            let deserialized_buffer =
                structured_deserialize(buffer_serialized, _target_realm, memory, ec)?;
            let buffer_obj = Types::value_as_object(&deserialized_buffer)
                .ok_or_else(|| ec.new_type_error("deserialized buffer is not an object"))?;

            if constructor == "DataView" {
                let buffer = Types::object_as_array_buffer(&buffer_obj)
                    .ok_or_else(|| ec.new_type_error("expected ArrayBuffer for DataView"))?;
                let dv = ec.construct_data_view_from_buffer(buffer, *byte_offset, *byte_length)?;
                let dv_obj = Types::object_from_data_view(dv);
                value = Types::value_from_object(dv_obj);
            } else {
                let kind = parse_typed_array_kind(constructor).ok_or_else(|| {
                    ec.new_type_error(&format!("Unknown typed array kind: {constructor}"))
                })?;
                let buffer = Types::object_as_array_buffer(&buffer_obj)
                    .ok_or_else(|| ec.new_type_error("expected ArrayBuffer for TypedArray"))?;
                let ta = ec.construct_typed_array_view(kind, buffer, *byte_offset, *byte_length)?;
                let ta_obj = Types::object_from_typed_array(ta);
                value = Types::value_from_object(ta_obj);
            }
        }

        // Step 17: Otherwise, if serialized.[[Type]] is "Map":
        SerializedRecord::Map(_) => {
            let map_obj = ec.construct(intrinsics.map.clone(), &[], None)?;
            value = Types::value_from_object(map_obj);
            deep = true;
        }

        // Step 18: Otherwise, if serialized.[[Type]] is "Set":
        SerializedRecord::Set(_) => {
            let set_obj = ec.construct(intrinsics.set.clone(), &[], None)?;
            value = Types::value_from_object(set_obj);
            deep = true;
        }

        // Step 19: Otherwise, if serialized.[[Type]] is "Array":
        SerializedRecord::Array { length, .. } => {
            let array = ec.create_empty_array();
            let len_key = ec.property_key_from_str("length");
            let len_val = ec.value_from_number(*length as f64);
            ec.set(array.clone(), len_key, len_val, true)?;
            value = Types::value_from_object(array);
            deep = true;
        }

        // Step 20: Otherwise, if serialized.[[Type]] is "Object":
        SerializedRecord::Object(_) => {
            let obj = ec.create_plain_object(None);
            value = Types::value_from_object(obj);
            deep = true;
        }

        // Step 21: Otherwise, if serialized.[[Type]] is "Error":
        SerializedRecord::Error {
            name,
            message,
            stack,
            cause,
        } => {
            value = deserialize_error(name, message, stack, cause, ec)?;
        }

        // Step 22: Otherwise (platform object):
        SerializedRecord::PlatformObject { .. } => {
            return Err(crate::webidl::data_clone_error_value(ec));
        }
        // Back references resolve at step 2 (the memory map); a reference to
        // an index not yet built is a malformed record.
        SerializedRecord::BackReference(index) => {
            return Err(ec.new_type_error(&format!(
                "structured clone: dangling back reference {index}"
            )));
        }
    }

    // Step 23: Set memory[serialized] to value.  The serialization assigns
    // memory indices to exactly the record kinds that can be back-referenced
    // (object containers and the transfer placeholders); the same kinds get
    // their deserialized object reserved here, in the same pre-order.
    let needs_index = matches!(
        serialized,
        SerializedRecord::Object(_)
            | SerializedRecord::Map(_)
            | SerializedRecord::Set(_)
            | SerializedRecord::Array { .. }
            | SerializedRecord::TransferredValue(_)
    );
    if needs_index && let Some(obj) = Types::value_as_object(&value) {
        memory.reserve_deserialized_index(obj);
    }

    // Step 24: If deep is true:
    if deep {
        match serialized {
            // Step 24.a: If serialized.[[Type]] is "Map":
            SerializedRecord::Map(entries) => {
                let map_obj = Types::value_as_object(&value)
                    .and_then(|o| Types::object_as_map(&o))
                    .ok_or_else(|| ec.new_type_error("expected Map"))?;
                for (key_rec, val_rec) in entries {
                    let dk = structured_deserialize(key_rec, _target_realm, memory, ec)?;
                    let dv = structured_deserialize(val_rec, _target_realm, memory, ec)?;
                    ec.map_set_entry(&map_obj, dk, dv)?;
                }
            }

            // Step 24.b: If serialized.[[Type]] is "Set":
            SerializedRecord::Set(entries) => {
                let set_obj = Types::value_as_object(&value)
                    .and_then(|o| Types::object_as_set(&o))
                    .ok_or_else(|| ec.new_type_error("expected Set"))?;
                for entry in entries {
                    let de = structured_deserialize(entry, _target_realm, memory, ec)?;
                    ec.set_add_entry(&set_obj, de)?;
                }
            }

            // Step 24.c: If serialized.[[Type]] is "Array" or "Object":
            SerializedRecord::Array { properties, .. } | SerializedRecord::Object(properties) => {
                let obj = Types::value_as_object(&value)
                    .ok_or_else(|| ec.new_type_error("expected object"))?;
                for (key, val_rec) in properties {
                    let dv = structured_deserialize(val_rec, _target_realm, memory, ec)?;
                    let key_str = String::from_utf16_lossy(key);
                    let pk = ec.property_key_from_str(&key_str);
                    ec.set(obj.clone(), pk, dv, true)?;
                }
            }

            // Step 24.d: Otherwise (platform object):
            SerializedRecord::PlatformObject { .. } => {}
            _ => {}
        }
    }

    // Step 25: Return value.
    Ok(value)
}

/// Deserialize a PrimitiveValue back to JsValue.
fn deserialize_primitive_value(
    p: &PrimitiveValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    match p {
        PrimitiveValue::Undefined => Ok(ec.value_undefined()),
        PrimitiveValue::Null => Ok(ec.value_null()),
        PrimitiveValue::Boolean(b) => Ok(ec.value_from_bool(*b)),
        PrimitiveValue::Number(n) => Ok(ec.value_from_number(*n)),
        PrimitiveValue::String(s) => {
            let js_str = ec.js_string_from_str(&String::from_utf16_lossy(s));
            Ok(ec.value_from_string(js_str))
        }
        PrimitiveValue::BigInt(s) => {
            if let Some(bi) = ec.string_to_bigint(ec.js_string_from_str(s)) {
                Ok(Types::value_from_bigint(bi))
            } else {
                // Fall back to 0n if parsing failed
                let bi = ec
                    .string_to_bigint(ec.js_string_from_str("0"))
                    .unwrap_or_else(|| unreachable!("0 is a valid BigInt"));
                Ok(Types::value_from_bigint(bi))
            }
        }
    }
}

/// <https://html.spec.whatwg.org/#structureddeserialize>
/// Step 21: Otherwise, if serialized.[[Type]] is "Error":
///   Step 21.1: Let prototype be %Error.prototype%.
///   Step 21.2: If serialized.[[Name]] is "EvalError", then set prototype to %EvalError.prototype%.
///   Step 21.3: If serialized.[[Name]] is "RangeError", then set prototype to %RangeError.prototype%.
///   Step 21.4: If serialized.[[Name]] is "ReferenceError", then set prototype to %ReferenceError.prototype%.
///   Step 21.5: If serialized.[[Name]] is "SyntaxError", then set prototype to %SyntaxError.prototype%.
///   Step 21.6: If serialized.[[Name]] is "TypeError", then set prototype to %TypeError.prototype%.
///   Step 21.7: If serialized.[[Name]] is "URIError", then set prototype to %URIError.prototype%.
///   Step 21.8: Let message be serialized.[[Message]].
///   Step 21.9: Set value to OrdinaryObjectCreate(prototype, « [[ErrorData]], [[Stack]] »).
///   Step 21.10: Let messageDesc be PropertyDescriptor { [[Value]]: message, [[Writable]]: true,
///                 [[Enumerable]]: false, [[Configurable]]: true }.
///   Step 21.11: If message is not undefined, then perform !
///                 OrdinaryDefineOwnProperty(value, "message", messageDesc).
///   Step 21.12: Set value.[[Stack]] to serialized.[[Stack]].
fn deserialize_error(
    name: &str,
    message: &Option<String>,
    stack: &str,
    cause: &Option<Box<SerializedRecord>>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);

    // Steps 21.1-21.7: Select constructor based on error name.
    let error_ctor = match name {
        "EvalError" => intrinsics.eval_error.clone(),
        "RangeError" => intrinsics.range_error.clone(),
        "ReferenceError" => intrinsics.reference_error.clone(),
        "SyntaxError" => intrinsics.syntax_error.clone(),
        "TypeError" => intrinsics.type_error.clone(),
        "URIError" => intrinsics.uri_error.clone(),
        _ => intrinsics.error.clone(),
    };

    // Create the error via constructor. If message is present, pass it.
    let error_obj = if let Some(msg) = message {
        let msg_val = ec.value_from_string(ec.js_string_from_str(msg));
        ec.construct(error_ctor, &[msg_val], None)
    } else {
        ec.construct(error_ctor, &[], None)
    }?;

    // Step 21.12: Set value.[[Stack]] to serialized.[[Stack]].
    let stack_key = ec.property_key_from_str("stack");
    let stack_str = ec.js_string_from_str(stack);
    let stack_val = ec.value_from_string(stack_str);
    ec.set(error_obj.clone(), stack_key, stack_val, true)?;
    let name_key = ec.property_key_from_str("name");
    let name_str = ec.js_string_from_str(name);
    let name_val = ec.value_from_string(name_str);
    ec.set(error_obj.clone(), name_key, name_val, true)?;

    // Additionally deserialize [[ErrorData]].[[Cause]] if present.
    if let Some(cause_serialized) = cause {
        let mut cause_memory = MemoryMap::default();
        if let Ok(cause_val) = structured_deserialize(
            cause_serialized,
            &ec.value_undefined(),
            &mut cause_memory,
            ec,
        ) {
            let cause_key = ec.property_key_from_str("cause");
            ec.set(error_obj.clone(), cause_key, cause_val, true)?;
        }
    }

    Ok(Types::value_from_object(error_obj))
}

// StructuredDeserializeWithTransfer

/// The result of [`structured_deserialize_with_transfer`].
/// Corresponds to the Record { [[Deserialized]]: deserialized, [[TransferredValues]]: transferredValues }
/// from the spec.
pub struct DeserializeWithTransferResult {
    pub deserialized: JsValue,
    pub transferred_values: Vec<JsValue>,
}

/// <https://html.spec.whatwg.org/#structureddeserializewithtransfer>
pub fn structured_deserialize_with_transfer(
    serialize_result: &SerializeWithTransferResult,
    target_realm: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<DeserializeWithTransferResult, Types> {
    // Step 1: Let memory be an empty map.
    let mut memory = MemoryMap::default();

    // Step 2: Let transferredValues be a new empty List.
    let mut transferred_values = Vec::new();

    // Step 3: For each transferDataHolder of
    //          serializeWithTransferResult.[[TransferDataHolders]]:
    for holder in &serialize_result.transfer_data_holders {
        let value: JsValue = match holder {
            // Step 3.1: If transferDataHolder.[[Type]] is "ArrayBuffer", then set value to a new
            //            ArrayBuffer object in targetRealm whose [[ArrayBufferData]] internal slot
            //            value is transferDataHolder.[[ArrayBufferData]], and whose
            //            [[ArrayBufferByteLength]] internal slot value is
            //            transferDataHolder.[[ArrayBufferByteLength]].
            TransferDataHolder::ArrayBuffer {
                data,
                byte_length: _,
                ..
            } => {
                let realm = ec.current_realm();
                let intrinsics = ec.realm_intrinsics(&realm);
                let buf = ec
                    .allocate_array_buffer(intrinsics.array_buffer.clone(), data.len() as u64, None)
                    .map_err(|_| crate::webidl::data_clone_error_value(ec))?;
                // Write data into the buffer byte by byte.
                for (i, byte) in data.iter().enumerate() {
                    let byte_val = ec.value_from_number(*byte as f64);
                    ec.set_value_in_buffer(
                        &buf,
                        i as u64,
                        TypedArrayElementType::Uint8,
                        byte_val,
                        false,
                        js_engine::enums::SharedMemoryOrder::Unordered,
                    )?;
                }
                let buf_obj = Types::object_from_array_buffer(buf);
                Types::value_from_object(buf_obj)
            }
            // Step 3.2: If transferDataHolder.[[Type]] is "MessagePort", then
            //           run the transfer-receiving steps for MessagePort given
            //           dataHolder and a new MessagePort in targetRealm.
            // Note: The steps run in messageport::transfer_receiving_steps,
            // which creates the new port in the current (target) realm
            // first, then runs the transfer-receiving steps on it.
            TransferDataHolder::MessagePort(holder) => {
                messageport::transfer_receiving_steps(holder, ec)?
            }
        };

        // Step 3.4: Set memory[transferDataHolder] to value.
        // Note: The spec keys the memory map by the dataHolder record itself.
        // Record identity cannot cross an IPC boundary, so the transferred
        // values are kept in a list indexed by transfer-list position; the
        // `TransferredValue(index)` records in the serialized graph resolve
        // here.  The transfer placeholders are the first entries the
        // serialization indexed, so each transferred value reserves the same
        // memory index, and a `BackReference` to a placeholder resolves to
        // the value itself.
        if let Some(obj) = Types::value_as_object(&value) {
            memory.reserve_deserialized_index(obj);
        }
        transferred_values.push(value);
    }
    memory.transferred_values = transferred_values.clone();

    // Step 4: Let deserialized be ? StructuredDeserialize(
    //           serializeWithTransferResult.[[Serialized]], targetRealm, memory).
    let deserialized =
        structured_deserialize(&serialize_result.serialized, target_realm, &mut memory, ec)?;

    // Step 5: Return { [[Deserialized]]: deserialized, [[TransferredValues]]: transferredValues }.
    Ok(DeserializeWithTransferResult {
        deserialized,
        transferred_values,
    })
}

// structuredClone API (WindowOrWorkerGlobalScope)

/// <https://html.spec.whatwg.org/#dom-structuredclone>
pub fn structured_clone(
    value: JsValue,
    options: Option<StructuredCloneOptions>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Let serialized be ? StructuredSerializeWithTransfer(value, options["transfer"]).
    let transfer = options
        .as_ref()
        .and_then(|o| o.transfer.clone())
        .unwrap_or_default();
    let serialized = structured_serialize_with_transfer(&value, transfer, ec)?;

    // Step 2: Let deserializeRecord be ? StructuredDeserializeWithTransfer(...).
    let desc_result = structured_deserialize_with_transfer(&serialized, &ec.value_undefined(), ec)?;

    // Step 3: Return deserializeRecord.[[Deserialized]].
    Ok(desc_result.deserialized)
}

/// Options for [`structured_clone`].
#[derive(Debug, Clone)]
pub struct StructuredCloneOptions {
    pub transfer: Option<Vec<JsValue>>,
}

// Utility functions

/// Reverse of EscapeRegExpPattern (spec 22.2.3.2.5).
/// The source getter escapes `\/`, `\n`, `\r`, `\u2028`, `\u2029`.
/// We undo these to recover the original [[OriginalSource]].
fn unescape_regexp_source(escaped: &str) -> String {
    let mut result = String::with_capacity(escaped.len());
    let mut chars = escaped.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('/') => result.push('/'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('u') => {
                    // \u2028 or \u2029
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16)
                        && let Some(ch) = char::from_u32(code)
                    {
                        result.push(ch);
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn normalize_error_name(name: &str) -> String {
    match name {
        "Error" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError" | "TypeError"
        | "URIError" => name.to_string(),
        _ => String::from("Error"),
    }
}

fn typed_array_kind_name(kind: TypedArrayElementType) -> String {
    use TypedArrayElementType::*;
    match kind {
        Int8 => String::from("Int8Array"),
        Uint8 => String::from("Uint8Array"),
        Uint8Clamped => String::from("Uint8ClampedArray"),
        Int16 => String::from("Int16Array"),
        Uint16 => String::from("Uint16Array"),
        Int32 => String::from("Int32Array"),
        Uint32 => String::from("Uint32Array"),
        BigInt64 => String::from("BigInt64Array"),
        BigUint64 => String::from("BigUint64Array"),
        Float32 => String::from("Float32Array"),
        Float64 => String::from("Float64Array"),
        Float16 => String::from("Float16Array"),
    }
}

fn parse_typed_array_kind(name: &str) -> Option<TypedArrayElementType> {
    use TypedArrayElementType::*;
    match name {
        "Int8Array" => Some(Int8),
        "Uint8Array" => Some(Uint8),
        "Uint8ClampedArray" => Some(Uint8Clamped),
        "Int16Array" => Some(Int16),
        "Uint16Array" => Some(Uint16),
        "Int32Array" => Some(Int32),
        "Uint32Array" => Some(Uint32),
        "BigInt64Array" => Some(BigInt64),
        "BigUint64Array" => Some(BigUint64),
        "Float32Array" => Some(Float32),
        "Float64Array" => Some(Float64),
        "Float16Array" => Some(Float16),
        _ => None,
    }
}
