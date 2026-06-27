//! <https://html.spec.whatwg.org/#safe-passing-of-structured-data>
//!
//! Implements the structured clone algorithm, including serialization and
//! deserialization of JavaScript values across realm boundaries, and support
//! for transferable and serializable platform objects.
//!
//! The `SerializedRecord` and `PrimitiveValue` types are pure data (serde-serializable)
//! so they can cross IPC boundaries. The Boa engine integration is confined to the
//! `structured_serialize_internal` and `structured_deserialize` functions.

// The traits, variants, and fields below that trigger dead_code warnings
// are intentionally defined as the spec-required extension points for
// future [Serializable]/[Transferable] platform objects and resizable
// ArrayBuffer support. All of them will be used once those features are
// wired up.
#![allow(dead_code)]

use std::collections::HashMap;

use boa_engine::{
    Context, JsBigInt, JsError, JsNativeError, JsResult, JsString, JsValue, JsVariant,
    builtins::error::{Error, ErrorKind},
    js_string,
    object::{
        JsObject,
        builtins::{
            JsArray, JsArrayBuffer, JsDataView, JsDate, JsMap, JsRegExp, JsSet,
            JsSharedArrayBuffer, JsTypedArray, js_typed_array_from_kind,
        },
    },
    property::PropertyKey,
};

use crate::dom::DOMException;
use crate::webidl::bindings::create_interface_instance;
use js_engine::boa::BoaTypes;

// ──────────────────────────────────────────────────────────────────────────────
// Traits for platform objects (bridge layer)
// ──────────────────────────────────────────────────────────────────────────────

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
        context: &mut Context,
    ) -> JsResult<()>;
}

/// <https://html.spec.whatwg.org/#transferable-objects>
///
/// A platform object whose primary interface is decorated with the `[Transferable]`
/// Web IDL extended attribute must implement this trait.
pub trait Transferable: std::fmt::Debug {
    /// <https://html.spec.whatwg.org/#transfer-steps>
    fn transfer_steps(
        &self,
        data_holder: &mut HashMap<String, JsValue>,
        context: &mut Context,
    ) -> JsResult<()>;

    /// <https://html.spec.whatwg.org/#transfer-receiving-steps>
    fn transfer_receiving_steps(
        &self,
        data_holder: &HashMap<String, JsValue>,
        context: &mut Context,
    ) -> JsResult<()>;
}

// ──────────────────────────────────────────────────────────────────────────────
// Pure-data types (IPC-safe)
// ──────────────────────────────────────────────────────────────────────────────

/// A primitive JavaScript value in a portable, serializable form.
#[derive(Debug, Clone)]
pub enum PrimitiveValue {
    /// The `undefined` value.
    Undefined,
    /// The `null` value.
    Null,
    /// A boolean.
    Boolean(bool),
    /// A 64-bit floating point number.
    Number(f64),
    /// A string, stored as UTF-16 code units to preserve unpaired surrogates.
    String(Vec<u16>),
    /// A BigInt, represented as its decimal string.
    BigInt(String),
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
///
/// A serialized representation of a JavaScript value (corresponds to a Record
/// in the spec). All fields are plain Rust types so the enum can cross IPC
/// boundaries.
#[derive(Debug, Clone)]
pub enum SerializedRecord {
    /// { [[Type]]: "primitive", [[Value]]: value }
    Primitive(PrimitiveValue),
    /// { [[Type]]: "Boolean", [[BooleanData]]: bool }
    Boolean(bool),
    /// { [[Type]]: "Number", [[NumberData]]: f64 }
    Number(f64),
    /// { [[Type]]: "BigInt", [[BigIntData]]: string }
    BigInt(String),
    /// { [[Type]]: "String", [[StringData]]: string (as UTF-16 code units) }
    String(Vec<u16>),
    /// { [[Type]]: "Date", [[DateValue]]: f64 }
    Date(f64),
    /// { [[Type]]: "RegExp", [[OriginalSource]], [[OriginalFlags]] }
    RegExp { source: String, flags: String },
    /// { [[Type]]: "SharedArrayBuffer" }  — raw bytes + metadata
    SharedArrayBuffer {
        data: Vec<u8>,
        agent_cluster: String,
    },
    /// { [[Type]]: "ArrayBuffer", [[ArrayBufferData]]: dataCopy, [[ArrayBufferByteLength]]: size }
    ArrayBuffer {
        data: Vec<u8>,
        byte_length: u64,
        max_byte_length: Option<u64>,
    },
    /// { [[Type]]: "ArrayBufferView" }
    /// When [[Constructor]] is "DataView":
    ///   { [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: byteLength, [[ByteOffset]]: byteOffset }
    /// Otherwise (typed array):
    ///   { [[Constructor]]: constructor, [[ArrayBufferSerialized]]: bufferSerialized,
    ///     [[ByteLength]]: byteLength, [[ByteOffset]]: byteOffset, [[ArrayLength]]: arrayLength }
    ArrayBufferView {
        constructor: String,
        buffer_serialized: Box<SerializedRecord>,
        byte_length: u64,
        byte_offset: u64,
        array_length: Option<u64>,
    },
    /// { [[Type]]: "Map" }
    Map(Vec<(SerializedRecord, SerializedRecord)>),
    /// { [[Type]]: "Set" }
    Set(Vec<SerializedRecord>),
    /// { [[Type]]: "Error" }
    Error {
        name: String,
        message: Option<String>,
        stack: String,
        cause: Option<Box<SerializedRecord>>,
    },
    /// { [[Type]]: "Array" }
    Array {
        length: u64,
        properties: Vec<(Vec<u16>, SerializedRecord)>,
    },
    /// Platform object implementing [Serializable].
    PlatformObject {
        interface_name: String,
        fields: HashMap<String, SerializedRecord>,
    },
    /// { [[Type]]: "Object" }
    Object(Vec<(Vec<u16>, SerializedRecord)>),
}

// ──────────────────────────────────────────────────────────────────────────────
// Memory map (Boa-internal, for cycle/duplicate detection)
// ──────────────────────────────────────────────────────────────────────────────

/// The memory map used for cycle and duplicate detection during (de)serialization.
///
/// <https://html.spec.whatwg.org/#structuredserializeinternal> step 1.
#[derive(Default)]
pub struct MemoryMap {
    serialized: HashMap<usize, SerializedRecord>,
    deserialized: HashMap<usize, JsObject>,
}

impl MemoryMap {
    fn get_serialized(&self, object: &JsObject) -> Option<&SerializedRecord> {
        let addr = std::ptr::from_ref(object.as_ref()).addr();
        self.serialized.get(&addr)
    }
    fn insert_serialized(&mut self, object: &JsObject, record: SerializedRecord) {
        let addr = std::ptr::from_ref(object.as_ref()).addr();
        self.serialized.insert(addr, record);
    }
    fn get_serialized_by_addr_mut(&mut self, addr: usize) -> Option<&mut SerializedRecord> {
        self.serialized.get_mut(&addr)
    }
    fn get_deserialized(&self, record: &SerializedRecord) -> Option<JsObject> {
        let addr = std::ptr::from_ref(record).addr();
        self.deserialized.get(&addr).cloned()
    }
    fn insert_deserialized(&mut self, record: &SerializedRecord, object: JsObject) {
        let addr = std::ptr::from_ref(record).addr();
        self.deserialized.insert(addr, object);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DataCloneError helper
// ──────────────────────────────────────────────────────────────────────────────

fn data_clone_error(context: &mut Context) -> JsError {
    JsError::from_opaque(JsValue::from(
        create_interface_instance::<BoaTypes, DOMException>(
            DOMException::new(
                String::from("The object could not be cloned."),
                String::from("DataCloneError"),
            ),
            crate::js::context_as_ec(context),
        )
        .expect("DOMException construction should not fail"),
    ))
}

fn internal_error(message: &str) -> JsError {
    JsError::from(JsNativeError::typ().with_message(String::from(message)))
}

// ──────────────────────────────────────────────────────────────────────────────
// Bridge: JsValue → PrimitiveValue
// ──────────────────────────────────────────────────────────────────────────────

/// Convert a JsValue to its portable PrimitiveValue representation.
fn js_value_to_primitive(value: &JsValue) -> Option<PrimitiveValue> {
    match value.variant() {
        JsVariant::Null => Some(PrimitiveValue::Null),
        JsVariant::Undefined => Some(PrimitiveValue::Undefined),
        JsVariant::Boolean(b) => Some(PrimitiveValue::Boolean(b)),
        JsVariant::Float64(f) => Some(PrimitiveValue::Number(f)),
        JsVariant::Integer32(i) => Some(PrimitiveValue::Number(f64::from(i))),
        JsVariant::String(s) => Some(PrimitiveValue::String(s.as_str().to_vec())),
        JsVariant::BigInt(bi) => Some(PrimitiveValue::BigInt(bi.to_string())),
        JsVariant::Symbol(_) | JsVariant::Object(_) => None,
    }
}

/// Convert a PropertyKey to UTF-16 code units (for use in serialized property lists).
fn property_key_to_string(key: &PropertyKey, context: &mut Context) -> JsResult<Vec<u16>> {
    match key {
        PropertyKey::String(s) => Ok(s.as_str().to_vec()),
        PropertyKey::Symbol(_) => Err(data_clone_error(context)),
        PropertyKey::Index(i) => Ok(i.get().to_string().encode_utf16().collect()),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// StructuredSerializeInternal
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
fn structured_serialize_internal(
    value: &JsValue,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 1: If memory was not supplied, let memory be an empty map.
    //         (memory is always supplied here.)

    // Step 2: If memory[value] exists, then return memory[value].
    if let Some(object) = value.as_object() {
        if let Some(record) = memory.get_serialized(&object) {
            return Ok(record.clone());
        }
    }

    // Step 3: Let deep be false.
    // (This implementation handles deep serialization inline in the helper functions
    //  rather than via a unified deep flag — see serialize_map_contents, serialize_set_contents,
    //  serialize_array, and the ordinary object path below.)

    // Step 4: If value is undefined, null, a Boolean, a Number, a BigInt, or a String,
    //         then return { [[Type]]: "primitive", [[Value]]: value }.
    if let Some(prim) = js_value_to_primitive(value) {
        return Ok(SerializedRecord::Primitive(prim));
    }

    // Step 5: If value is a Symbol, then throw a "DataCloneError" DOMException.
    if value.is_symbol() {
        return Err(data_clone_error(context));
    }

    // Step 6: Let serialized be an uninitialized value.
    // (Implemented via individual return values in each branch below.)
    let object = value
        .as_object()
        .ok_or_else(|| internal_error("unexpected non-object value in serialize"))?;

    // Step 7: If value has a [[BooleanData]] internal slot, then set serialized to
    //           { [[Type]]: "Boolean", [[BooleanData]]: value.[[BooleanData]] }.
    if let Some(b) = object.downcast_ref::<bool>() {
        return Ok(SerializedRecord::Boolean(*b));
    }

    // Step 8: Otherwise, if value has a [[NumberData]] internal slot, then set serialized to
    //           { [[Type]]: "Number", [[NumberData]]: value.[[NumberData]] }.
    if let Some(n) = object.downcast_ref::<f64>() {
        return Ok(SerializedRecord::Number(*n));
    }

    // Step 9: Otherwise, if value has a [[BigIntData]] internal slot, then set serialized to
    //           { [[Type]]: "BigInt", [[BigIntData]]: value.[[BigIntData]] }.
    if let Some(bi) = object.downcast_ref::<JsBigInt>() {
        return Ok(SerializedRecord::BigInt(bi.to_string()));
    }

    // Step 10: Otherwise, if value has a [[StringData]] internal slot, then set serialized to
    //            { [[Type]]: "String", [[StringData]]: value.[[StringData]] }.
    if let Some(s) = object.downcast_ref::<JsString>() {
        return Ok(SerializedRecord::String(s.as_str().to_vec()));
    }

    // Step 11: Otherwise, if value has a [[DateValue]] internal slot, then set serialized to
    //            { [[Type]]: "Date", [[DateValue]]: value.[[DateValue]] }.
    if let Ok(date) = JsDate::from_object(object.clone()) {
        let time = date.get_time(context)?;
        let ms = time
            .as_number()
            .ok_or_else(|| internal_error("Date.getTime did not return a number"))?;
        return Ok(SerializedRecord::Date(ms));
    }

    // Step 12: Otherwise, if value has a [[RegExpMatcher]] internal slot, then set serialized to
    //            { [[Type]]: "RegExp", [[RegExpMatcher]]: value.[[RegExpMatcher]],
    //              [[OriginalSource]]: value.[[OriginalSource]],
    //              [[OriginalFlags]]: value.[[OriginalFlags]] }.
    if let Ok(regexp) = JsRegExp::from_object(object.clone()) {
        // Per spec, we must store [[OriginalSource]] and [[OriginalFlags]], not
        // the escaped source getter (EscapeRegExpPattern). Since the only escaping
        // EscapeRegExpPattern does is prefixing "/" with "\", we unescape by
        // removing the leading "\" on "/".
        let escaped_source = regexp.source(context)?;
        let source = unescape_regexp_source(&escaped_source);
        let flags = regexp.flags(context)?;
        return Ok(SerializedRecord::RegExp { source, flags });
    }

    // Step 13: Otherwise, if value has an [[ArrayBufferData]] internal slot.
    // Step 13.1: If IsSharedArrayBuffer(value) is true:
    if let Ok(sab) = JsSharedArrayBuffer::from_object(object.clone()) {
        return serialize_shared_array_buffer(&sab, for_storage, context);
    }
    // Step 13.2: Otherwise (non-shared ArrayBuffer):
    if let Ok(buffer) = JsArrayBuffer::from_object(object.clone()) {
        return serialize_array_buffer(&buffer, context);
    }

    // Step 14: Otherwise, if value has a [[ViewedArrayBuffer]] internal slot.
    if let Ok(dv) = JsDataView::from_object(object.clone()) {
        return serialize_dataview(&dv, for_storage, memory, context);
    }
    if let Ok(ta) = JsTypedArray::from_object(object.clone()) {
        return serialize_typed_array(&ta, for_storage, memory, context);
    }

    // Step 15: Otherwise, if value has a [[MapData]] internal slot.
    if let Ok(map) = JsMap::from_object(object.clone()) {
        return serialize_map_contents(&map, for_storage, memory, &object, context);
    }

    // Step 16: Otherwise, if value has a [[SetData]] internal slot.
    if let Ok(set) = JsSet::from_object(object.clone()) {
        return serialize_set_contents(&set, for_storage, memory, &object, context);
    }

    // Step 17: Otherwise, if value has an [[ErrorData]] internal slot and value is not a platform object.
    if object.is::<Error>() {
        return serialize_error(&object, context);
    }

    // Step 18: Otherwise, if value is an Array exotic object.
    if let Ok(array) = JsArray::from_object(object.clone()) {
        return serialize_array(&array, for_storage, memory, context);
    }

    // Step 19: Otherwise, if value is a platform object that is a serializable object.
    // TODO: Check registered [Serializable] platform objects.

    // Step 20: Otherwise, if value is a platform object, then throw a "DataCloneError" DOMException.
    // TODO: Add platform object detection.

    // Step 21: Otherwise, if IsCallable(value) is true, then throw a "DataCloneError" DOMException.
    if object.is_callable() {
        return Err(data_clone_error(context));
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
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(&object, serialized);

    // Step 26 ("If deep is true" block for the Object case):
    //   For each key in ! EnumerableOwnProperties(value, key):
    //     If ! HasOwnProperty(value, key) is true:
    //       Let inputValue be ? value.[[Get]](key, value).
    //       Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory).
    //       Append (key, outputValue) to serialized.[[Properties]].
    let keys = object.own_property_keys(context)?;
    let mut properties = Vec::new();
    for key in keys {
        if !object.has_own_property(key.clone(), context)? {
            continue;
        }
        // Check enumerability per EnumerableOwnProperties.
        let desc = object.borrow().properties().get(&key);
        let enumerable = desc.as_ref().and_then(|d| d.enumerable()).unwrap_or(false);
        if !enumerable {
            continue;
        }

        let input_value = object.get(key.clone(), context)?;
        let output_value =
            structured_serialize_internal(&input_value, for_storage, memory, context)?;
        let key_str = property_key_to_string(&key, context)?;
        properties.push((key_str, output_value));
    }

    // Step 28 (spec): Return serialized.
    if let Some(SerializedRecord::Object(props)) = memory.get_serialized_by_addr_mut(addr) {
        *props = properties;
    }
    Ok(memory
        .serialized
        .remove(&addr)
        .expect("entry must exist in memory"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Serialization helpers
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 13.2: non-shared ArrayBuffer.
fn serialize_array_buffer(
    buffer: &JsArrayBuffer,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 13.2.1: If IsDetachedBuffer(value) is true, then throw a "DataCloneError" DOMException.
    let data = buffer.data().ok_or_else(|| data_clone_error(context))?;

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
    sab: &JsSharedArrayBuffer,
    for_storage: bool,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 13.1.1: If IsSharedArrayBuffer(value) is true:
    //   Step 13.1.1.1: If the current settings object's cross-origin isolated capability is false,
    //                    then throw a "DataCloneError" DOMException.
    // TODO: Check cross-origin isolated capability.

    // Step 13.1.2: If forStorage is true, then throw a "DataCloneError" DOMException.
    if for_storage {
        return Err(data_clone_error(context));
    }

    // Step 13.1.3: If value has an [[ArrayBufferMaxByteLength]] internal slot, then
    //                set serialized to { [[Type]]: "GrowableSharedArrayBuffer", ... }.
    // TODO: Support GrowableSharedArrayBuffer.

    // Step 13.1.4: Otherwise, set serialized to { [[Type]]: "SharedArrayBuffer",
    //               [[ArrayBufferData]]: value.[[ArrayBufferData]],
    //               [[ArrayBufferByteLength]]: value.[[ArrayBufferByteLength]],
    //               [[AgentCluster]]: the surrounding agent's agent cluster }.
    // Copy raw bytes for IPC portability.
    let data = sab.to_vec();
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
    dataview: &JsDataView,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // TODO: Check IsArrayBufferViewOutOfBounds.

    // Step 14.2: Let buffer be the value of value's [[ViewedArrayBuffer]] internal slot.
    let dv_obj: JsObject = dataview.clone().into();
    let buffer_val = dv_obj.get(js_string!("buffer"), context)?;

    // Step 14.3: Let bufferSerialized be ? StructuredSerializeInternal(buffer, forStorage, memory).
    let buffer_serialized =
        structured_serialize_internal(&buffer_val, for_storage, memory, context)?;

    // Step 14.4: If value has a [[DataView]] internal slot, then set serialized to
    //              { [[Type]]: "ArrayBufferView", [[Constructor]]: "DataView",
    //                [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: value.[[ByteLength]],
    //                [[ByteOffset]]: value.[[ByteOffset]] }.
    let byte_length = dv_obj
        .get(js_string!("byteLength"), context)?
        .to_number(context)? as u64;
    let byte_offset = dv_obj
        .get(js_string!("byteOffset"), context)?
        .to_number(context)? as u64;

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
    typed_array: &JsTypedArray,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // TODO: Check IsArrayBufferViewOutOfBounds.

    // Step 14.2: Let buffer be the value of value's [[ViewedArrayBuffer]] internal slot.
    let buffer_val = typed_array.buffer(context)?;

    // Step 14.3: Let bufferSerialized be ? StructuredSerializeInternal(buffer, forStorage, memory).
    let buffer_serialized =
        structured_serialize_internal(&buffer_val, for_storage, memory, context)?;

    // Step 14.5 (spec numbering): Otherwise (value does not have a [[DataView]] internal slot):
    //   Step 14.5.1: Assert: value has a [[TypedArrayName]] internal slot.
    let kind = typed_array
        .kind()
        .ok_or_else(|| internal_error("TypedArray has no kind"))?;
    let constructor = typed_array_kind_name(kind);

    //   Step 14.5.2: Set serialized to { [[Type]]: "ArrayBufferView", [[Constructor]]: value.[[TypedArrayName]],
    //                   [[ArrayBufferSerialized]]: bufferSerialized, [[ByteLength]]: value.[[ByteLength]],
    //                   [[ByteOffset]]: value.[[ByteOffset]], [[ArrayLength]]: value.[[ArrayLength]] }.
    let byte_length = typed_array.byte_length(context)? as u64;
    let byte_offset = typed_array.byte_offset(context)? as u64;
    let array_length = typed_array.length(context)? as u64;

    Ok(SerializedRecord::ArrayBufferView {
        constructor,
        buffer_serialized: Box::new(buffer_serialized),
        byte_length,
        byte_offset,
        array_length: Some(array_length),
    })
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
    map: &JsMap,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 15.1: Set serialized to { [[Type]]: "Map", [[MapData]]: a new empty List }.
    let serialized = SerializedRecord::Map(Vec::new());

    // Step 15.2: Set memory[value] to serialized.
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(object, serialized);

    // Step 15.3: Let copiedList be a new empty List.
    // Step 15.4: For each Record { [[Key]], [[Value]] } entry of value.[[MapData]]:
    //   If entry.[[Key]] is not the special value empty, append copiedEntry to copiedList.
    let mut raw_entries: Vec<(JsValue, JsValue)> = Vec::new();
    map.for_each_native(|key, val| {
        raw_entries.push((key, val));
        Ok(())
    })?;

    // Step 15.5: For each Record { [[Key]], [[Value]] } entry of copiedList:
    let mut entries = Vec::new();
    for (key, val) in raw_entries {
        // Step 15.5.1: Let serializedKey be ? StructuredSerializeInternal(entry.[[Key]], forStorage, memory).
        let sk = structured_serialize_internal(&key, for_storage, memory, context)?;
        // Step 15.5.2: Let serializedValue be ? StructuredSerializeInternal(entry.[[Value]], forStorage, memory).
        let sv = structured_serialize_internal(&val, for_storage, memory, context)?;
        // Step 15.5.3: Append { [[Key]]: serializedKey, [[Value]]: serializedValue } to serialized.[[MapData]].
        entries.push((sk, sv));
    }

    // Update the entry in memory with the serialized entries.
    if let Some(SerializedRecord::Map(entry_list)) = memory.get_serialized_by_addr_mut(addr) {
        *entry_list = entries;
    }
    Ok(memory
        .serialized
        .remove(&addr)
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
    set: &JsSet,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 16.1: Set serialized to { [[Type]]: "Set", [[SetData]]: a new empty List }.
    let serialized = SerializedRecord::Set(Vec::new());

    // Step 16.2: Set memory[value] to serialized.
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(object, serialized);

    // Step 16.3: Let copiedList be a new empty List.
    // Step 16.4: For each entry of value.[[SetData]]:
    //   If entry is not the special value empty, append entry to copiedList.
    let mut raw_entries: Vec<JsValue> = Vec::new();
    set.for_each_native(|val| {
        raw_entries.push(val);
        Ok(())
    })?;

    // Step 16.5: For each entry of copiedList:
    let mut entries = Vec::new();
    for entry in raw_entries {
        // Step 16.5.1: Let serializedEntry be ? StructuredSerializeInternal(entry, forStorage, memory).
        let sv = structured_serialize_internal(&entry, for_storage, memory, context)?;
        // Step 16.5.2: Append serializedEntry to serialized.[[SetData]].
        entries.push(sv);
    }

    if let Some(SerializedRecord::Set(entry_list)) = memory.get_serialized_by_addr_mut(addr) {
        *entry_list = entries;
    }
    Ok(memory
        .serialized
        .remove(&addr)
        .expect("Set entry must exist in memory"))
}

/// <https://html.spec.whatwg.org/#structuredserializeinternal>
/// Step 17: Otherwise, if value has an [[ErrorData]] internal slot and value is not a platform object.
fn serialize_error(object: &JsObject, context: &mut Context) -> JsResult<SerializedRecord> {
    // Step 17.1: Let name be ? Get(value, "name").
    let name_val = object.get(js_string!("name"), context)?;
    // Step 17.2: If name is not a String value, then set name to "Error".
    let name = if name_val.is_string() {
        name_val.to_string(context)?.to_std_string_escaped()
    } else {
        String::from("Error")
    };
    // Step 17.3: If name is not one of "Error", "EvalError", "RangeError", "ReferenceError",
    //              "SyntaxError", "TypeError", or "URIError", then set name to "Error".
    let name = normalize_error_name(&name);

    // Step 17.4: Let valueMessageDesc be ? value.[[GetOwnProperty]]("message").
    // Step 17.5: Let message be undefined if IsDataDescriptor(valueMessageDesc) is false,
    //              and ? ToString(valueMessageDesc.[[Value]]) otherwise.
    let pk = PropertyKey::from(js_string!("message"));
    let msg_desc = object.borrow().properties().get(&pk);
    let message: Option<String> = match msg_desc {
        Some(desc) if desc.is_data_descriptor() => desc
            .value()
            .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
            .transpose()?,
        _ => None,
    };

    // Step 17.6: Let stack be an implementation-defined string that represents value.[[Stack]].
    let stack = object
        .get(js_string!("stack"), context)
        .ok()
        .and_then(|v| v.to_string(context).ok().map(|s| s.to_std_string_escaped()))
        .unwrap_or_default();

    // Step 17.7: Set serialized to { [[Type]]: "Error", [[Name]]: name, [[Message]]: message, [[Stack]]: stack }.
    // Additionally, serialize [[ErrorData]].[[Cause]] per ES2022.
    let pk = PropertyKey::from(js_string!("cause"));
    let cause_desc = object.borrow().properties().get(&pk);
    let cause = cause_desc
        .as_ref()
        .and_then(|d| {
            if d.is_data_descriptor() {
                let val = d.value()?;
                // cause is serialized as a sub-serialization. But we don't have
                // access to the forStorage flag here. Use a fresh memory map for
                // the self-contained cause value.
                let mut cause_memory = MemoryMap::default();
                structured_serialize_internal(val, false, &mut cause_memory, context).ok()
            } else {
                None
            }
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
    array: &JsArray,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 18.1-2: Get length.
    let length = array.length(context)? as u64;

    // Step 18.3: Set serialized to { [[Type]]: "Array", [[Length]]: valueLen, [[Properties]]: a new empty List }.
    let serialized = SerializedRecord::Array {
        length,
        properties: Vec::new(),
    };

    // Step 18.4: Set memory[value] to serialized.
    let object = JsObject::from(array.clone());
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(&object, serialized);

    // Step 18.5: For each key of ! EnumerableOwnProperties(value, key):
    let keys = object.own_property_keys(context)?;
    let mut properties = Vec::new();
    for key in keys {
        // If ! HasOwnProperty(value, key) is true:
        if !object.has_own_property(key.clone(), context)? {
            continue;
        }
        // Check enumerability per EnumerableOwnProperties.
        let desc = object.borrow().properties().get(&key);
        let enumerable = desc.as_ref().and_then(|d| d.enumerable()).unwrap_or(false);
        if !enumerable {
            continue;
        }

        // Let inputValue be ? value.[[Get]](key, value).
        let input_value = object.get(key.clone(), context)?;
        // Let outputValue be ? StructuredSerializeInternal(inputValue, forStorage, memory).
        let output_value =
            structured_serialize_internal(&input_value, for_storage, memory, context)?;
        let key_str = property_key_to_string(&key, context)?;
        properties.push((key_str, output_value));
    }

    if let Some(SerializedRecord::Array {
        properties: props, ..
    }) = memory.get_serialized_by_addr_mut(addr)
    {
        *props = properties;
    }
    Ok(memory
        .serialized
        .remove(&addr)
        .expect("Array entry must exist in memory"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API: simple serialize entry points
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#structuredserialize>
pub fn structured_serialize(value: &JsValue, context: &mut Context) -> JsResult<SerializedRecord> {
    let mut memory = MemoryMap::default();
    structured_serialize_internal(value, false, &mut memory, context)
}

/// <https://html.spec.whatwg.org/#structuredserializeforstorage>
pub fn structured_serialize_for_storage(
    value: &JsValue,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    let mut memory = MemoryMap::default();
    structured_serialize_internal(value, true, &mut memory, context)
}

// ──────────────────────────────────────────────────────────────────────────────
// StructuredSerializeWithTransfer
// ──────────────────────────────────────────────────────────────────────────────

/// Represents the result of [`structured_serialize_with_transfer`].
pub struct SerializeWithTransferResult {
    pub serialized: SerializedRecord,
    pub transfer_data_holders: Vec<TransferDataHolder>,
}

/// A data holder for a transferred value (pure data, IPC-safe).
///
/// Corresponds to a Record from StructuredSerializeWithTransfer step 5.
#[derive(Debug, Clone)]
pub enum TransferDataHolder {
    /// { [[Type]]: "ArrayBuffer", [[ArrayBufferData]]: dataCopy,
    ///   [[ArrayBufferByteLength]]: byteLength }
    /// or { [[Type]]: "ResizableArrayBuffer", [[ArrayBufferData]]: dataCopy,
    ///   [[ArrayBufferByteLength]]: byteLength, [[ArrayBufferMaxByteLength]]: maxByteLength }
    ArrayBuffer {
        data: Vec<u8>,
        byte_length: u64,
        max_byte_length: Option<u64>,
    },
    /// A platform object implementing [Transferable].
    PlatformObject {
        interface_name: String,
        fields: HashMap<String, JsValue>,
    },
}

/// <https://html.spec.whatwg.org/#structuredserializewithtransfer>
pub fn structured_serialize_with_transfer(
    value: &JsValue,
    transfer_list: Vec<JsValue>,
    context: &mut Context,
) -> JsResult<SerializeWithTransferResult> {
    // Step 1: Let memory be an empty map.
    let mut memory = MemoryMap::default();

    // Step 2: For each transferable of transferList:
    for transferable in &transfer_list {
        let Some(object) = transferable.as_object() else {
            // If transferable has neither an [[ArrayBufferData]] internal slot nor a
            // [[Detached]] internal slot, then throw a "DataCloneError" DOMException.
            return Err(data_clone_error(context));
        };
        let has_ab = JsArrayBuffer::from_object(object.clone()).is_ok();
        let has_sab = JsSharedArrayBuffer::from_object(object.clone()).is_ok();

        // Step 2.1: If transferable has neither an [[ArrayBufferData]] internal slot nor a
        //             [[Detached]] internal slot, then throw.
        if !has_ab && !has_sab && !is_transferable_platform_object(&object) {
            return Err(data_clone_error(context));
        }

        // Step 2.2: If transferable has an [[ArrayBufferData]] internal slot and
        //             IsSharedArrayBuffer(transferable) is true, then throw.
        if has_sab {
            return Err(data_clone_error(context));
        }

        // Step 2.3: If memory[transferable] exists, then throw.
        if memory.get_serialized(&object).is_some() {
            return Err(data_clone_error(context));
        }

        // Step 2.4: Set memory[transferable] to { [[Type]]: an uninitialized value }.
        let placeholder_addr = std::ptr::from_ref(object.as_ref()).addr();
        memory.serialized.insert(
            placeholder_addr,
            SerializedRecord::Primitive(PrimitiveValue::Undefined),
        );
    }

    // Step 3: Let serialized be ? StructuredSerializeInternal(value, false, memory).
    let serialized = structured_serialize_internal(value, false, &mut memory, context)?;

    // Step 4: Let transferDataHolders be a new empty List.
    let mut transfer_data_holders = Vec::new();

    // Step 5: For each transferable of transferList:
    for transferable in &transfer_list {
        let object = transferable
            .as_object()
            .ok_or_else(|| data_clone_error(context))?;
        if let Ok(buffer) = JsArrayBuffer::from_object(object.clone()) {
            // Step 5.1: If transferable has an [[ArrayBufferData]] internal slot:
            //   Step 5.1.1: If IsDetachedBuffer(transferable) is true, then throw.
            if buffer.data().is_none() {
                return Err(data_clone_error(context));
            }

            // TODO: Check for [[ArrayBufferMaxByteLength]] (ResizableArrayBuffer case).

            // Step 5.1.4: Perform ? DetachArrayBuffer(transferable).
            let data_vec = buffer.detach(&JsValue::undefined())?;
            let data = data_vec.to_vec();
            let byte_length = data.len() as u64;

            transfer_data_holders.push(TransferDataHolder::ArrayBuffer {
                data,
                byte_length,
                max_byte_length: None,
            });
        } else {
            // Step 5.2: Otherwise (platform object with [[Detached]] internal slot).
            // TODO: platform object transfer.
            return Err(data_clone_error(context));
        }
    }

    // Step 6: Return { [[Serialized]]: serialized, [[TransferDataHolders]]: transferDataHolders }.
    Ok(SerializeWithTransferResult {
        serialized,
        transfer_data_holders,
    })
}

fn is_transferable_platform_object(_object: &JsObject) -> bool {
    false // TODO
}

// ──────────────────────────────────────────────────────────────────────────────
// StructuredDeserialize
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#structureddeserialize>
fn structured_deserialize(
    serialized: &SerializedRecord,
    _target_realm: &JsValue,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: If memory was not supplied, let memory be an empty map.
    //         (memory is always supplied here.)

    // Step 2: If memory[serialized] exists, then return memory[serialized].
    if let Some(object) = memory.get_deserialized(serialized) {
        return Ok(JsValue::from(object));
    }

    // Step 3: Let deep be false.
    let mut _deep = false;

    // Step 4: Let value be an uninitialized value.
    let value: JsValue;

    match serialized {
        // Step 5: If serialized.[[Type]] is "primitive", then set value to serialized.[[Value]].
        SerializedRecord::Primitive(p) => {
            value = deserialize_primitive_value(p, context)?;
        }
        // Step 6: Otherwise, if serialized.[[Type]] is "Boolean", then set value to a new Boolean
        //           object in targetRealm whose [[BooleanData]] internal slot value is
        //           serialized.[[BooleanData]].
        SerializedRecord::Boolean(b) => {
            let prototype = context.intrinsics().constructors().boolean().prototype();
            value = JsValue::from(JsObject::from_proto_and_data(prototype, *b).upcast());
        }
        // Step 7: Otherwise, if serialized.[[Type]] is "Number", then set value to a new Number
        //           object in targetRealm whose [[NumberData]] internal slot value is
        //           serialized.[[NumberData]].
        SerializedRecord::Number(n) => {
            let prototype = context.intrinsics().constructors().number().prototype();
            value = JsValue::from(JsObject::from_proto_and_data(prototype, *n).upcast());
        }
        // Step 8: Otherwise, if serialized.[[Type]] is "BigInt", then set value to a new BigInt
        //           object in targetRealm whose [[BigIntData]] internal slot value is
        //           serialized.[[BigIntData]].
        SerializedRecord::BigInt(s) => {
            let prototype = context.intrinsics().constructors().bigint().prototype();
            let bi = if let Ok(bi) = s.parse::<boa_engine::bigint::RawBigInt>() {
                JsBigInt::new(bi)
            } else {
                JsBigInt::zero()
            };
            value = JsValue::from(JsObject::from_proto_and_data(prototype, bi).upcast());
        }
        // Step 9: Otherwise, if serialized.[[Type]] is "String", then set value to a new String
        //           object in targetRealm whose [[StringData]] internal slot value is
        //           serialized.[[StringData]].
        SerializedRecord::String(s) => {
            let prototype = context.intrinsics().constructors().string().prototype();
            let js_str = JsString::from(s.as_slice());
            value = JsValue::from(JsObject::from_proto_and_data(prototype, js_str).upcast());
        }
        // Step 10: Otherwise, if serialized.[[Type]] is "Date", then set value to a new Date
        //            object in targetRealm whose [[DateValue]] internal slot value is
        //            serialized.[[DateValue]].
        SerializedRecord::Date(ms) => {
            let date = JsDate::new(context);
            date.set_time(*ms, context)?;
            value = JsValue::from(date);
        }
        // Step 11: Otherwise, if serialized.[[Type]] is "RegExp", then set value to a new RegExp
        //            object in targetRealm whose [[RegExpMatcher]] internal slot value is
        //            serialized.[[RegExpMatcher]], whose [[OriginalSource]] internal slot value is
        //            serialized.[[OriginalSource]], and whose [[OriginalFlags]] internal slot value
        //            is serialized.[[OriginalFlags]].
        SerializedRecord::RegExp { source, flags } => {
            let regexp = JsRegExp::new(
                JsString::from(source.as_str()),
                JsString::from(flags.as_str()),
                context,
            )?;
            value = JsValue::from(regexp);
        }
        // Step 12: Otherwise, if serialized.[[Type]] is "SharedArrayBuffer":
        //   If targetRealm's corresponding agent cluster is not serialized.[[AgentCluster]],
        //   then throw a "DataCloneError" DOMException.
        //   Otherwise, set value to a new SharedArrayBuffer object in targetRealm
        //   whose [[ArrayBufferData]] internal slot value is serialized.[[ArrayBufferData]] and
        //   whose [[ArrayBufferByteLength]] internal slot value is
        //   serialized.[[ArrayBufferByteLength]].
        SerializedRecord::SharedArrayBuffer {
            data,
            agent_cluster,
        } => {
            let _ = agent_cluster; // TODO: check agent cluster.
            let sab = JsSharedArrayBuffer::new(data.len(), context)?;
            // TODO: Implement proper shared memory data transfer.
            value = JsValue::from(sab);
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
            let aligned = boa_engine::object::builtins::AlignedVec::from_slice(0, data_copy);
            // Catch any exception from ArrayBuffer creation and re-throw as DataCloneError.
            let buffer = JsArrayBuffer::from_byte_block(aligned, context)
                .map_err(|_| data_clone_error(context))?;
            value = JsValue::from(buffer);
        }
        // Step 15: Otherwise, if serialized.[[Type]] is "ResizableArrayBuffer":
        //   Set value to a new ArrayBuffer object in targetRealm whose [[ArrayBufferData]]
        //   internal slot value is serialized.[[ArrayBufferData]], whose
        //   [[ArrayBufferByteLength]] internal slot value is
        //   serialized.[[ArrayBufferByteLength]], and whose [[ArrayBufferMaxByteLength]] internal
        //   slot value is serialized.[[ArrayBufferMaxByteLength]].
        // TODO: Support ResizableArrayBuffer deserialization.

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
                structured_deserialize(buffer_serialized, _target_realm, memory, context)?;
            let buffer_obj = deserialized_buffer
                .as_object()
                .ok_or_else(|| internal_error("deserialized buffer is not an object"))?;
            let buffer = JsArrayBuffer::from_object(buffer_obj.clone())?;

            //   If serialized.[[Constructor]] is "DataView", then set value to a new DataView
            //   object in targetRealm whose [[ViewedArrayBuffer]] internal slot value is
            //   deserializedArrayBuffer, whose [[ByteLength]] internal slot value is
            //   serialized.[[ByteLength]], and whose [[ByteOffset]] internal slot value is
            //   serialized.[[ByteOffset]].
            if constructor == "DataView" {
                let data_view = JsDataView::from_js_array_buffer(
                    buffer,
                    Some(*byte_offset),
                    Some(*byte_length),
                    context,
                )?;
                value = JsValue::from(data_view);
            } else {
                //   Otherwise, set value to a new typed array object in targetRealm, using the
                //   constructor given by serialized.[[Constructor]], whose
                //   [[ViewedArrayBuffer]] internal slot value is deserializedArrayBuffer, whose
                //   [[TypedArrayName]] internal slot value is serialized.[[Constructor]], whose
                //   [[ByteLength]] internal slot value is serialized.[[ByteLength]], whose
                //   [[ByteOffset]] internal slot value is serialized.[[ByteOffset]], and whose
                //   [[ArrayLength]] internal slot value is serialized.[[ArrayLength]].
                let kind = parse_typed_array_kind(constructor)?;
                value = js_typed_array_from_kind(kind, buffer, context)?;
            }
        }
        // Step 17: Otherwise, if serialized.[[Type]] is "Map":
        //   Set value to a new Map object in targetRealm whose [[MapData]] internal slot value
        //   is a new empty List.
        //   Set deep to true.
        SerializedRecord::Map(_) => {
            value = JsValue::from(JsMap::new(context));
            _deep = true;
        }
        // Step 18: Otherwise, if serialized.[[Type]] is "Set":
        //   Set value to a new Set object in targetRealm whose [[SetData]] internal slot value
        //   is a new empty List.
        //   Set deep to true.
        SerializedRecord::Set(_) => {
            value = JsValue::from(JsSet::new(context));
            _deep = true;
        }
        // Step 19: Otherwise, if serialized.[[Type]] is "Array":
        //   Let outputProto be targetRealm.[[Intrinsics]].[[%Array.prototype%]].
        //   Set value to ! ArrayCreate(serialized.[[Length]], outputProto).
        //   Set deep to true.
        SerializedRecord::Array { length, .. } => {
            let array = JsArray::new(context)?;
            array.set(js_string!("length"), *length as f64, true, context)?;
            value = JsValue::from(array);
            _deep = true;
        }
        // Step 20: Otherwise, if serialized.[[Type]] is "Object":
        //   Set value to a new Object in targetRealm.
        //   Set deep to true.
        SerializedRecord::Object(_) => {
            value = JsValue::from(JsObject::with_object_proto(context.intrinsics()));
            _deep = true;
        }
        // Step 21: Otherwise, if serialized.[[Type]] is "Error":
        //   (see deserialize_error)
        SerializedRecord::Error {
            name,
            message,
            stack,
            cause,
        } => {
            value = deserialize_error(name, message, stack, cause, context)?;
        }
        // Step 22: Otherwise (platform object):
        //   Let interfaceName be serialized.[[Type]].
        //   If the interface identified by interfaceName is not exposed in targetRealm,
        //   then throw a "DataCloneError" DOMException.
        //   Set value to a new instance of the interface identified by interfaceName,
        //   created in targetRealm.
        //   Set deep to true.
        SerializedRecord::PlatformObject { .. } => {
            return Err(data_clone_error(context));
        }
    }

    // Step 23: Set memory[serialized] to value.
    if let Some(obj) = value.as_object() {
        memory.insert_deserialized(serialized, obj.clone());
    }

    // Step 24: If deep is true:
    if _deep {
        match serialized {
            // Step 24.a: If serialized.[[Type]] is "Map":
            SerializedRecord::Map(entries) => {
                let map = value
                    .as_object()
                    .and_then(|o| JsMap::from_object(o.clone()).ok())
                    .ok_or_else(|| internal_error("expected Map"))?;
                for (key_rec, val_rec) in entries {
                    let dk = structured_deserialize(key_rec, _target_realm, memory, context)?;
                    let dv = structured_deserialize(val_rec, _target_realm, memory, context)?;
                    map.set(dk, dv, context)?;
                }
            }
            // Step 24.b: If serialized.[[Type]] is "Set":
            SerializedRecord::Set(entries) => {
                let set = value
                    .as_object()
                    .and_then(|o| JsSet::from_object(o.clone()).ok())
                    .ok_or_else(|| internal_error("expected Set"))?;
                for entry in entries {
                    let de = structured_deserialize(entry, _target_realm, memory, context)?;
                    set.add(de, context)?;
                }
            }
            // Step 24.c: If serialized.[[Type]] is "Array" or "Object":
            SerializedRecord::Array { properties, .. } | SerializedRecord::Object(properties) => {
                let obj = value
                    .as_object()
                    .ok_or_else(|| internal_error("expected object"))?;
                for (key, val_rec) in properties {
                    let dv = structured_deserialize(val_rec, _target_realm, memory, context)?;
                    obj.set(JsString::from(key.as_slice()), dv, true, context)?;
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
fn deserialize_primitive_value(p: &PrimitiveValue, _context: &mut Context) -> JsResult<JsValue> {
    match p {
        PrimitiveValue::Undefined => Ok(JsValue::undefined()),
        PrimitiveValue::Null => Ok(JsValue::null()),
        PrimitiveValue::Boolean(b) => Ok(JsValue::new(*b)),
        PrimitiveValue::Number(n) => Ok(JsValue::new(*n)),
        PrimitiveValue::String(s) => Ok(JsValue::from(JsString::from(s.as_slice()))),
        PrimitiveValue::BigInt(s) => {
            if let Ok(bi) = s.parse::<boa_engine::bigint::RawBigInt>() {
                Ok(JsValue::from(JsBigInt::new(bi)))
            } else {
                Ok(JsValue::from(JsBigInt::zero()))
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
    context: &mut Context,
) -> JsResult<JsValue> {
    // Steps 21.1-21.7: Select prototype based on error name.
    let cc = context.intrinsics().constructors();
    let prototype = match name {
        "EvalError" => cc.eval_error().prototype(),
        "RangeError" => cc.range_error().prototype(),
        "ReferenceError" => cc.reference_error().prototype(),
        "SyntaxError" => cc.syntax_error().prototype(),
        "TypeError" => cc.type_error().prototype(),
        "URIError" => cc.uri_error().prototype(),
        _ => cc.error().prototype(),
    };

    // Step 21.8: Let message be serialized.[[Message]].
    // (message is already passed in as a parameter)

    // Step 21.9: Set value to OrdinaryObjectCreate(prototype, « [[ErrorData]], [[Stack]] »).
    let error_data = Error::new(ErrorKind::Error);
    let error_obj: JsObject = JsObject::from_proto_and_data(prototype, error_data).upcast();

    // Steps 21.10-21.11: Define "message" property if not undefined.
    if let Some(msg) = message {
        let desc = boa_engine::property::PropertyDescriptorBuilder::new()
            .value(JsString::from(msg.as_str()))
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build();
        let _ = error_obj.insert_property(js_string!("message"), desc);
    }

    // Step 21.12: Set value.[[Stack]] to serialized.[[Stack]].
    let _ = error_obj.set(js_string!("stack"), JsString::from(stack), true, context);
    let _ = error_obj.set(js_string!("name"), JsString::from(name), true, context);

    // Additionally deserialize [[ErrorData]].[[Cause]] if present.
    if let Some(cause_serialized) = cause {
        // Sub-deserialize the cause value in a fresh memory map.
        let mut cause_memory = MemoryMap::default();
        if let Ok(cause_val) = structured_deserialize(
            cause_serialized,
            &JsValue::undefined(),
            &mut cause_memory,
            context,
        ) {
            let _ = error_obj.set(js_string!("cause"), cause_val, true, context);
        }
    }

    Ok(JsValue::from(error_obj))
}

// ──────────────────────────────────────────────────────────────────────────────
// StructuredDeserializeWithTransfer
// ──────────────────────────────────────────────────────────────────────────────

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
    context: &mut Context,
) -> JsResult<DeserializeWithTransferResult> {
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
                let aligned = boa_engine::object::builtins::AlignedVec::from_slice(0, data);
                let buffer = JsArrayBuffer::from_byte_block(aligned, context)
                    .map_err(|_| data_clone_error(context))?;
                JsValue::from(buffer)
            }
            // Step 3.3: Otherwise (platform object).
            TransferDataHolder::PlatformObject { .. } => {
                // TODO: platform object transfer-receiving.
                return Err(data_clone_error(context));
            }
        };

        // Step 3.4: Set memory[transferDataHolder] to value.
        if let Some(obj) = value.as_object() {
            memory.insert_deserialized(
                &SerializedRecord::Primitive(PrimitiveValue::Undefined),
                obj.clone(),
            );
        }
        // Step 3.5: Append value to transferredValues.
        transferred_values.push(value);
    }

    // Step 4: Let deserialized be ? StructuredDeserialize(
    //           serializeWithTransferResult.[[Serialized]], targetRealm, memory).
    let deserialized = structured_deserialize(
        &serialize_result.serialized,
        target_realm,
        &mut memory,
        context,
    )?;

    // Step 5: Return { [[Deserialized]]: deserialized, [[TransferredValues]]: transferredValues }.
    Ok(DeserializeWithTransferResult {
        deserialized,
        transferred_values,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// structuredClone API (WindowOrWorkerGlobalScope)
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#dom-structuredclone>
pub fn structured_clone(
    value: JsValue,
    options: Option<StructuredCloneOptions>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: Let serialized be ? StructuredSerializeWithTransfer(value, options["transfer"]).
    let transfer = options
        .as_ref()
        .and_then(|o| o.transfer.clone())
        .unwrap_or_default();
    let serialized = structured_serialize_with_transfer(&value, transfer, context)?;

    // Step 2: Let deserializeRecord be ? StructuredDeserializeWithTransfer(...).
    let desc_result =
        structured_deserialize_with_transfer(&serialized, &JsValue::undefined(), context)?;

    // Step 3: Return deserializeRecord.[[Deserialized]].
    Ok(desc_result.deserialized)
}

/// Options for [`structured_clone`].
#[derive(Debug, Clone)]
pub struct StructuredCloneOptions {
    pub transfer: Option<Vec<JsValue>>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Utility functions
// ──────────────────────────────────────────────────────────────────────────────

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
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        }
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

fn typed_array_kind_name(kind: boa_engine::builtins::typed_array::TypedArrayKind) -> String {
    use boa_engine::builtins::typed_array::TypedArrayKind;
    match kind {
        TypedArrayKind::Int8 => String::from("Int8Array"),
        TypedArrayKind::Uint8 => String::from("Uint8Array"),
        TypedArrayKind::Uint8Clamped => String::from("Uint8ClampedArray"),
        TypedArrayKind::Int16 => String::from("Int16Array"),
        TypedArrayKind::Uint16 => String::from("Uint16Array"),
        TypedArrayKind::Int32 => String::from("Int32Array"),
        TypedArrayKind::Uint32 => String::from("Uint32Array"),
        TypedArrayKind::BigInt64 => String::from("BigInt64Array"),
        TypedArrayKind::BigUint64 => String::from("BigUint64Array"),
        TypedArrayKind::Float32 => String::from("Float32Array"),
        TypedArrayKind::Float64 => String::from("Float64Array"),
    }
}

fn parse_typed_array_kind(
    name: &str,
) -> JsResult<boa_engine::builtins::typed_array::TypedArrayKind> {
    use boa_engine::builtins::typed_array::TypedArrayKind;
    match name {
        "Int8Array" => Ok(TypedArrayKind::Int8),
        "Uint8Array" => Ok(TypedArrayKind::Uint8),
        "Uint8ClampedArray" => Ok(TypedArrayKind::Uint8Clamped),
        "Int16Array" => Ok(TypedArrayKind::Int16),
        "Uint16Array" => Ok(TypedArrayKind::Uint16),
        "Int32Array" => Ok(TypedArrayKind::Int32),
        "Uint32Array" => Ok(TypedArrayKind::Uint32),
        "BigInt64Array" => Ok(TypedArrayKind::BigInt64),
        "BigUint64Array" => Ok(TypedArrayKind::BigUint64),
        "Float32Array" => Ok(TypedArrayKind::Float32),
        "Float64Array" => Ok(TypedArrayKind::Float64),
        _ => Err(internal_error(&format!("Unknown typed array kind: {name}"))),
    }
}
