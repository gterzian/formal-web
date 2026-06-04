//! <https://html.spec.whatwg.org/#safe-passing-of-structured-data>
//!
//! Implements the structured clone algorithm, including serialization and
//! deserialization of JavaScript values across realm boundaries, and support
//! for transferable and serializable platform objects.
//!
//! The `SerializedRecord` and `PrimitiveValue` types are pure data (serde-serializable)
//! so they can cross IPC boundaries. The Boa engine integration is confined to the
//! `structured_serialize_internal` and `structured_deserialize` functions.

use std::collections::HashMap;

use boa_engine::{
    Context, JsBigInt, JsError, JsNativeError, JsResult, JsString, JsValue, JsVariant,
    builtins::error::{Error, ErrorKind},
    class::Class,
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
    /// A string.
    String(String),
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
    /// { [[Type]]: "String", [[StringData]]: string }
    String(String),
    /// { [[Type]]: "Date", [[DateValue]]: f64 }
    Date(f64),
    /// { [[Type]]: "RegExp", [[OriginalSource]], [[OriginalFlags]] }
    RegExp { source: String, flags: String },
    /// { [[Type]]: "SharedArrayBuffer" }  — raw bytes + metadata
    SharedArrayBuffer { data: Vec<u8>, agent_cluster: String },
    /// { [[Type]]: "ArrayBuffer" or "ResizableArrayBuffer" }
    ArrayBuffer { data: Vec<u8>, max_byte_length: Option<u64> },
    /// { [[Type]]: "ArrayBufferView" }
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
    Error { name: String, message: Option<String>, stack: String },
    /// { [[Type]]: "Array" }
    Array { length: u64, properties: Vec<(String, SerializedRecord)> },
    /// Platform object implementing [Serializable].
    PlatformObject { interface_name: String, fields: HashMap<String, SerializedRecord> },
    /// { [[Type]]: "Object" }
    Object(Vec<(String, SerializedRecord)>),
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
        DOMException::from_data(
            DOMException::new(
                String::from("The object could not be cloned."),
                String::from("DataCloneError"),
            ),
            context,
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
        JsVariant::String(s) => Some(PrimitiveValue::String(s.to_std_string_escaped())),
        JsVariant::BigInt(bi) => Some(PrimitiveValue::BigInt(bi.to_string())),
        JsVariant::Symbol(_) | JsVariant::Object(_) => None,
    }
}

/// Convert a PropertyKey to a String (for use in serialized property lists).
fn property_key_to_string(key: &PropertyKey, context: &mut Context) -> JsResult<String> {
    match key {
        PropertyKey::String(s) => Ok(s.to_std_string_escaped()),
        PropertyKey::Symbol(_) => Err(data_clone_error(context)),
        PropertyKey::Index(i) => Ok(i.get().to_string()),
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

    // Step 4: If value is undefined, null, a Boolean, a Number, a BigInt, or a String,
    //         then return { [[Type]]: "primitive", [[Value]]: value }.
    if let Some(prim) = js_value_to_primitive(value) {
        return Ok(SerializedRecord::Primitive(prim));
    }

    // Step 5: If value is a Symbol, then throw.
    if value.is_symbol() {
        return Err(data_clone_error(context));
    }

    let object = value.as_object().ok_or_else(|| {
        internal_error("unexpected non-object value in serialize")
    })?;

    // Step 7: [[BooleanData]] internal slot
    if let Some(b) = object.downcast_ref::<bool>() {
        return Ok(SerializedRecord::Boolean(*b));
    }

    // Step 8: [[NumberData]] internal slot
    if let Some(n) = object.downcast_ref::<f64>() {
        return Ok(SerializedRecord::Number(*n));
    }

    // Step 9: [[BigIntData]] internal slot
    if let Some(bi) = object.downcast_ref::<JsBigInt>() {
        return Ok(SerializedRecord::BigInt(bi.to_string()));
    }

    // Step 10: [[StringData]] internal slot
    if let Some(s) = object.downcast_ref::<JsString>() {
        return Ok(SerializedRecord::String(s.to_std_string_escaped()));
    }

    // Step 11: [[DateValue]] internal slot
    if let Ok(date) = JsDate::from_object(object.clone()) {
        let time = date.get_time(context)?;
        let ms = time.as_number().ok_or_else(|| {
            internal_error("Date.getTime did not return a number")
        })?;
        return Ok(SerializedRecord::Date(ms));
    }

    // Step 12: [[RegExpMatcher]] internal slot
    if let Ok(regexp) = JsRegExp::from_object(object.clone()) {
        let source = regexp.source(context)?;
        let flags = regexp.flags(context)?;
        return Ok(SerializedRecord::RegExp { source, flags });
    }

    // Step 13: [[ArrayBufferData]] internal slot
    // Step 13.1: IsSharedArrayBuffer first
    if let Ok(sab) = JsSharedArrayBuffer::from_object(object.clone()) {
        return serialize_shared_array_buffer(&sab, for_storage, context);
    }
    // Step 13.2: non-shared ArrayBuffer
    if let Ok(buffer) = JsArrayBuffer::from_object(object.clone()) {
        return serialize_array_buffer(&buffer, context);
    }

    // Step 14: [[ViewedArrayBuffer]] internal slot
    if let Ok(dv) = JsDataView::from_object(object.clone()) {
        return serialize_dataview(&dv, for_storage, memory, context);
    }
    if let Ok(ta) = JsTypedArray::from_object(object.clone()) {
        return serialize_typed_array(&ta, for_storage, memory, context);
    }

    // Step 15: [[MapData]] internal slot
    if let Ok(map) = JsMap::from_object(object.clone()) {
        return serialize_map_contents(&map, for_storage, memory, &object, context);
    }

    // Step 16: [[SetData]] internal slot
    if let Ok(set) = JsSet::from_object(object.clone()) {
        return serialize_set_contents(&set, for_storage, memory, &object, context);
    }

    // Step 17: [[ErrorData]] internal slot (not platform object)
    if object.is::<Error>() {
        return serialize_error(&object, context);
    }

    // Step 18: Array exotic object
    if let Ok(array) = JsArray::from_object(object.clone()) {
        return serialize_array(&array, for_storage, memory, context);
    }

    // Step 19: Platform object that is a serializable object
    // TODO: Check registered [Serializable] platform objects.

    // Step 20: Platform object → throw
    // TODO: Add platform object detection.

    // Step 21: IsCallable → throw
    if object.is_callable() {
        return Err(data_clone_error(context));
    }

    // Steps 22-23: exotic/extra slot checks
    // TODO: Add these checks.

    // Step 24: Ordinary object
    let serialized = SerializedRecord::Object(Vec::new());

    // Step 25: Set memory[value] to serialized (for cycle detection).
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(&object, serialized);

    // Step 26.4: For each key in ! EnumerableOwnProperties(value, key):
    let keys = object.own_property_keys(context)?;
    let mut properties = Vec::new();
    for key in keys {
        if object.has_own_property(key.clone(), context)? {
            let input_value = object.get(key.clone(), context)?;
            let output_value =
                structured_serialize_internal(&input_value, for_storage, memory, context)?;
            let key_str = property_key_to_string(&key, context)?;
            properties.push((key_str, output_value));
        }
    }

    // Update the entry in memory with the serialized properties, then return it.
    if let Some(SerializedRecord::Object(props)) = memory.get_serialized_by_addr_mut(addr) {
        *props = properties;
    }
    Ok(memory.serialized.remove(&addr).expect("entry must exist in memory"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Serialization helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Step 13.2: non-shared ArrayBuffer.
fn serialize_array_buffer(buffer: &JsArrayBuffer, context: &mut Context) -> JsResult<SerializedRecord> {
    // Step 13.2.1: If IsDetachedBuffer → throw.
    let data = buffer.data().ok_or_else(|| data_clone_error(context))?;

    // Steps 13.2.2-4: Copy data bytes.
    let data_copy = data.to_vec();

    // Steps 13.2.5-6: Check resizable (TODO).
    Ok(SerializedRecord::ArrayBuffer { data: data_copy, max_byte_length: None })
}

/// Step 13.1: SharedArrayBuffer.
fn serialize_shared_array_buffer(
    sab: &JsSharedArrayBuffer,
    for_storage: bool,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 13.1.2: If forStorage is true, throw.
    if for_storage {
        return Err(data_clone_error(context));
    }
    // Step 13.1.1: Check cross-origin isolated capability (TODO).
    // Copy raw bytes for IPC portability.
    let data = sab.to_vec();
    let agent_cluster = String::from("default"); // TODO: use actual agent cluster.
    Ok(SerializedRecord::SharedArrayBuffer { data, agent_cluster })
}

/// Steps 14.1-14.5: DataView.
fn serialize_dataview(
    dataview: &JsDataView,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    // Step 14.2: Access underlying buffer via property lookup.
    let dv_obj: JsObject = dataview.clone().into();
    let buffer_val = dv_obj.get(js_string!("buffer"), context)?;
    // Step 14.3: Serialize buffer.
    let buffer_serialized =
        structured_serialize_internal(&buffer_val, for_storage, memory, context)?;
    let byte_length = dv_obj.get(js_string!("byteLength"), context)?.to_number(context)? as u64;
    let byte_offset = dv_obj.get(js_string!("byteOffset"), context)?.to_number(context)? as u64;

    Ok(SerializedRecord::ArrayBufferView {
        constructor: String::from("DataView"),
        buffer_serialized: Box::new(buffer_serialized),
        byte_length, byte_offset, array_length: None,
    })
}

/// Steps 14.1, 14.6: TypedArray.
fn serialize_typed_array(
    typed_array: &JsTypedArray,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    let buffer_val = typed_array.buffer(context)?;
    let buffer_serialized =
        structured_serialize_internal(&buffer_val, for_storage, memory, context)?;
    let kind = typed_array.kind().ok_or_else(|| internal_error("TypedArray has no kind"))?;
    let constructor = typed_array_kind_name(kind);
    let byte_length = typed_array.byte_length(context)? as u64;
    let byte_offset = typed_array.byte_offset(context)? as u64;
    let array_length = typed_array.length(context)? as u64;

    Ok(SerializedRecord::ArrayBufferView {
        constructor,
        buffer_serialized: Box::new(buffer_serialized),
        byte_length, byte_offset, array_length: Some(array_length),
    })
}

/// Step 26.1: deep-serialize Map (copy entries first per spec).
fn serialize_map_contents(
    map: &JsMap,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    let serialized = SerializedRecord::Map(Vec::new());
    // Step 25: Record in memory BEFORE deep serialization (cycle detection).
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(object, serialized);

    // Step 26.1.1-2: Copy entries first.
    let mut raw_entries: Vec<(JsValue, JsValue)> = Vec::new();
    map.for_each_native(|key, val| {
        raw_entries.push((key, val));
        Ok(())
    })?;

    // Step 26.1.3: Deep-serialize each entry.
    let mut entries = Vec::new();
    for (key, val) in raw_entries {
        let sk = structured_serialize_internal(&key, for_storage, memory, context)?;
        let sv = structured_serialize_internal(&val, for_storage, memory, context)?;
        entries.push((sk, sv));
    }

    // Update the entry in memory with the serialized entries, then return it.
    if let Some(SerializedRecord::Map(entry_list)) = memory.get_serialized_by_addr_mut(addr) {
        *entry_list = entries;
    }
    Ok(memory.serialized.remove(&addr).expect("Map entry must exist in memory"))
}

/// Step 26.2: deep-serialize Set (copy entries first).
fn serialize_set_contents(
    set: &JsSet,
    for_storage: bool,
    memory: &mut MemoryMap,
    object: &JsObject,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    let serialized = SerializedRecord::Set(Vec::new());
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(object, serialized);

    let mut raw_entries: Vec<JsValue> = Vec::new();
    set.for_each_native(|val| {
        raw_entries.push(val);
        Ok(())
    })?;

    let mut entries = Vec::new();
    for entry in raw_entries {
        let sv = structured_serialize_internal(&entry, for_storage, memory, context)?;
        entries.push(sv);
    }

    if let Some(SerializedRecord::Set(entry_list)) = memory.get_serialized_by_addr_mut(addr) {
        *entry_list = entries;
    }
    Ok(memory.serialized.remove(&addr).expect("Set entry must exist in memory"))
}

/// Step 17: Error objects.
fn serialize_error(object: &JsObject, context: &mut Context) -> JsResult<SerializedRecord> {
    let name_val = object.get(js_string!("name"), context)?;
    let name = if name_val.is_string() {
        name_val.to_string(context)?.to_std_string_escaped()
    } else {
        String::from("Error")
    };
    let name = normalize_error_name(&name);

    let message_val = object.get(js_string!("message"), context)?;
    let message = if !message_val.is_undefined() {
        message_val.to_string(context).ok().map(|s| s.to_std_string_escaped())
    } else { None };

    let stack = object.get(js_string!("stack"), context).ok()
        .and_then(|v| v.to_string(context).ok().map(|s| s.to_std_string_escaped()))
        .unwrap_or_default();

    Ok(SerializedRecord::Error { name, message, stack })
}

/// Step 18: Array objects.
fn serialize_array(
    array: &JsArray,
    for_storage: bool,
    memory: &mut MemoryMap,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
    let length = array.length(context)? as u64;
    let serialized = SerializedRecord::Array { length, properties: Vec::new() };
    let object = JsObject::from(array.clone());
    let addr = std::ptr::from_ref(object.as_ref()).addr();
    memory.insert_serialized(&object, serialized);

    let keys = object.own_property_keys(context)?;
    let mut properties = Vec::new();
    for key in keys {
        if object.has_own_property(key.clone(), context)? {
            let input_value = object.get(key.clone(), context)?;
            let output_value =
                structured_serialize_internal(&input_value, for_storage, memory, context)?;
            let key_str = property_key_to_string(&key, context)?;
            properties.push((key_str, output_value));
        }
    }

    if let Some(SerializedRecord::Array { properties: props, .. }) =
        memory.get_serialized_by_addr_mut(addr)
    {
        *props = properties;
    }
    Ok(memory.serialized.remove(&addr).expect("Array entry must exist in memory"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Public API: simple serialize entry points
// ──────────────────────────────────────────────────────────────────────────────

/// <https://html.spec.whatwg.org/#structuredserialize>
pub fn structured_serialize(
    value: &JsValue,
    context: &mut Context,
) -> JsResult<SerializedRecord> {
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
#[derive(Debug, Clone)]
pub enum TransferDataHolder {
    ArrayBuffer { data: Vec<u8>, max_byte_length: Option<u64> },
    PlatformObject { interface_name: String, fields: HashMap<String, JsValue> },
}

/// <https://html.spec.whatwg.org/#structuredserializewithtransfer>
pub fn structured_serialize_with_transfer(
    value: &JsValue,
    transfer_list: Vec<JsValue>,
    context: &mut Context,
) -> JsResult<SerializeWithTransferResult> {
    let mut memory = MemoryMap::default();

    for transferable in &transfer_list {
        let Some(object) = transferable.as_object() else {
            return Err(data_clone_error(context));
        };
        let has_ab = JsArrayBuffer::from_object(object.clone()).is_ok();
        let has_sab = JsSharedArrayBuffer::from_object(object.clone()).is_ok();
        if !has_ab && !has_sab && !is_transferable_platform_object(&object) {
            return Err(data_clone_error(context));
        }
        if has_sab {
            return Err(data_clone_error(context));
        }
        if memory.get_serialized(&object).is_some() {
            return Err(data_clone_error(context));
        }
        // Placeholder in memory.
        let placeholder_addr = std::ptr::from_ref(object.as_ref()).addr();
        memory.serialized.insert(placeholder_addr, SerializedRecord::Primitive(PrimitiveValue::Undefined));
    }

    let serialized = structured_serialize_internal(value, false, &mut memory, context)?;
    let mut transfer_data_holders = Vec::new();

    for transferable in &transfer_list {
        let object = transferable.as_object().ok_or_else(|| data_clone_error(context))?;
        if let Ok(buffer) = JsArrayBuffer::from_object(object.clone()) {
            if buffer.data().is_none() {
                return Err(data_clone_error(context));
            }
            let data = buffer.detach(&JsValue::undefined())?;
            transfer_data_holders.push(TransferDataHolder::ArrayBuffer {
                data: data.to_vec(),
                max_byte_length: None,
            });
        } else {
            // TODO: platform object transfer.
            return Err(data_clone_error(context));
        }
    }

    Ok(SerializeWithTransferResult { serialized, transfer_data_holders })
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
    // Step 2: If memory[serialized] exists, return it.
    if let Some(object) = memory.get_deserialized(serialized) {
        return Ok(JsValue::from(object));
    }

    let mut _deep = false;
    let value: JsValue;

    match serialized {
        // Step 5: primitive.
        SerializedRecord::Primitive(p) => {
            value = deserialize_primitive_value(p, context)?;
        }
        // Step 6: Boolean wrapper.
        SerializedRecord::Boolean(b) => {
            value = JsValue::new(*b);
        }
        // Step 7: Number wrapper.
        SerializedRecord::Number(n) => {
            value = JsValue::new(*n);
        }
        // Step 8: BigInt wrapper.
        SerializedRecord::BigInt(s) => {
            // Parse BigInt from string representation.
            if let Ok(bi) = s.parse::<boa_engine::bigint::RawBigInt>() {
                value = JsValue::from(JsBigInt::new(bi));
            } else {
                value = JsValue::from(JsBigInt::zero());
            }
        }
        // Step 9: String wrapper.
        SerializedRecord::String(s) => {
            value = JsValue::from(JsString::from(s.as_str()));
        }
        // Step 10: Date.
        SerializedRecord::Date(ms) => {
            let date = JsDate::new(context);
            date.set_time(*ms, context)?;
            value = JsValue::from(date);
        }
        // Step 11: RegExp.
        SerializedRecord::RegExp { source, flags } => {
            let regexp = JsRegExp::new(
                JsString::from(source.as_str()),
                JsString::from(flags.as_str()),
                context,
            )?;
            value = JsValue::from(regexp);
        }
        // Step 12: SharedArrayBuffer.
        SerializedRecord::SharedArrayBuffer { data, agent_cluster } => {
            let _ = agent_cluster; // TODO: check agent cluster.
            let sab = JsSharedArrayBuffer::new(data.len(), context)?;
            // Copy data into the SAB using to_vec roundtrip.
            // We create the buffer then use the public API to write data.
            // For now, accept the limitation that SAB data is read-only after creation.
            // TODO: Implement proper shared memory data transfer.
            value = JsValue::from(sab);
        }
        // Step 14: ArrayBuffer.
        SerializedRecord::ArrayBuffer { data: data_copy, .. } => {
            let aligned = boa_engine::object::builtins::AlignedVec::from_slice(0, data_copy);
            let buffer = match JsArrayBuffer::from_byte_block(aligned, context) {
                Ok(buf) => buf,
                Err(_) => return Err(data_clone_error(context)),
            };
            value = JsValue::from(buffer);
        }
        // Step 16: ArrayBufferView.
        SerializedRecord::ArrayBufferView {
            constructor, buffer_serialized, byte_length, byte_offset, array_length,
        } => {
            let deserialized_buffer =
                structured_deserialize(buffer_serialized, _target_realm, memory, context)?;
            let buffer_obj = deserialized_buffer.as_object().ok_or_else(|| {
                internal_error("deserialized buffer is not an object")
            })?;
            let buffer = JsArrayBuffer::from_object(buffer_obj.clone())?;

            if constructor == "DataView" {
                let data_view = JsDataView::from_js_array_buffer(
                    buffer, Some(*byte_offset), Some(*byte_length), context,
                )?;
                value = JsValue::from(data_view);
            } else {
                let kind = parse_typed_array_kind(constructor)?;
                value = js_typed_array_from_kind(kind, buffer, context)?;
            }
        }
        // Step 17: Map.
        SerializedRecord::Map(_) => {
            value = JsValue::from(JsMap::new(context));
            _deep = true;
        }
        // Step 18: Set.
        SerializedRecord::Set(_) => {
            value = JsValue::from(JsSet::new(context));
            _deep = true;
        }
        // Step 19: Array.
        SerializedRecord::Array { length, .. } => {
            let array = JsArray::new(context)?;
            array.set(js_string!("length"), *length as f64, true, context)?;
            value = JsValue::from(array);
            _deep = true;
        }
        // Step 20: Object.
        SerializedRecord::Object(_) => {
            value = JsValue::from(JsObject::with_object_proto(context.intrinsics()));
            _deep = true;
        }
        // Step 21: Error.
        SerializedRecord::Error { name, message, stack } => {
            value = deserialize_error(name, message, stack, context)?;
        }
        // Step 22: Platform object.
        SerializedRecord::PlatformObject { .. } => {
            return Err(data_clone_error(context));
        }
    }

    // Step 23: Set memory[serialized] to value.
    if let Some(obj) = value.as_object() {
        memory.insert_deserialized(serialized, obj.clone());
    }

    // Step 24: If deep is true, populate contents.
    if _deep {
        match serialized {
            SerializedRecord::Map(entries) => {
                let map = value.as_object()
                    .and_then(|o| JsMap::from_object(o.clone()).ok())
                    .ok_or_else(|| internal_error("expected Map"))?;
                for (key_rec, val_rec) in entries {
                    let dk = structured_deserialize(key_rec, _target_realm, memory, context)?;
                    let dv = structured_deserialize(val_rec, _target_realm, memory, context)?;
                    map.set(dk, dv, context)?;
                }
            }
            SerializedRecord::Set(entries) => {
                let set = value.as_object()
                    .and_then(|o| JsSet::from_object(o.clone()).ok())
                    .ok_or_else(|| internal_error("expected Set"))?;
                for entry in entries {
                    let de = structured_deserialize(entry, _target_realm, memory, context)?;
                    set.add(de, context)?;
                }
            }
            SerializedRecord::Array { properties, .. }
            | SerializedRecord::Object(properties) => {
                let obj = value.as_object().ok_or_else(|| internal_error("expected object"))?;
                for (key, val_rec) in properties {
                    let dv = structured_deserialize(val_rec, _target_realm, memory, context)?;
                    obj.set(JsString::from(key.as_str()), dv, true, context)?;
                }
            }
            SerializedRecord::PlatformObject { .. } => {}
            _ => {}
        }
    }

    Ok(value)
}

/// Deserialize a PrimitiveValue back to JsValue.
fn deserialize_primitive_value(p: &PrimitiveValue, _context: &mut Context) -> JsResult<JsValue> {
    match p {
        PrimitiveValue::Undefined => Ok(JsValue::undefined()),
        PrimitiveValue::Null => Ok(JsValue::null()),
        PrimitiveValue::Boolean(b) => Ok(JsValue::new(*b)),
        PrimitiveValue::Number(n) => Ok(JsValue::new(*n)),
        PrimitiveValue::String(s) => Ok(JsValue::from(JsString::from(s.as_str()))),
        PrimitiveValue::BigInt(s) => {
            if let Ok(bi) = s.parse::<boa_engine::bigint::RawBigInt>() {
                Ok(JsValue::from(JsBigInt::new(bi)))
            } else {
                Ok(JsValue::from(JsBigInt::zero()))
            }
        }
    }
}

/// Deserialize an Error per spec steps 21.1-21.12.
fn deserialize_error(
    name: &str,
    message: &Option<String>,
    stack: &str,
    context: &mut Context,
) -> JsResult<JsValue> {
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

    // Step 21.9: OrdinaryObjectCreate with [[ErrorData]] and [[Stack]].
    let error_data = Error::new(ErrorKind::Error);
    let error_obj: JsObject = JsObject::from_proto_and_data(prototype, error_data).upcast();

    // Steps 21.10-11: Define "message" property.
    if let Some(msg) = message {
        let desc = boa_engine::property::PropertyDescriptorBuilder::new()
            .value(JsString::from(msg.as_str()))
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build();
        let _ = error_obj.insert_property(js_string!("message"), desc);
    }

    // Step 21.12: Set [[Stack]].
    let _ = error_obj.set(js_string!("stack"), JsString::from(stack), true, context);
    let _ = error_obj.set(js_string!("name"), JsString::from(name), true, context);

    Ok(JsValue::from(error_obj))
}

// ──────────────────────────────────────────────────────────────────────────────
// StructuredDeserializeWithTransfer
// ──────────────────────────────────────────────────────────────────────────────

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
    let mut memory = MemoryMap::default();
    let mut transferred_values = Vec::new();

    for holder in &serialize_result.transfer_data_holders {
        let value: JsValue = match holder {
            TransferDataHolder::ArrayBuffer { data, .. } => {
                let aligned = boa_engine::object::builtins::AlignedVec::from_slice(0, data);
                let buffer = JsArrayBuffer::from_byte_block(aligned, context)
                    .map_err(|_| data_clone_error(context))?;
                JsValue::from(buffer)
            }
            TransferDataHolder::PlatformObject { .. } => {
                // TODO: platform object transfer-receiving.
                return Err(data_clone_error(context));
            }
        };

        if let Some(obj) = value.as_object() {
            memory.insert_deserialized(
                &SerializedRecord::Primitive(PrimitiveValue::Undefined),
                obj.clone(),
            );
        }
        transferred_values.push(value);
    }

    let deserialized = structured_deserialize(
        &serialize_result.serialized,
        target_realm,
        &mut memory,
        context,
    )?;

    Ok(DeserializeWithTransferResult { deserialized, transferred_values })
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
    let transfer = options.as_ref().and_then(|o| o.transfer.clone()).unwrap_or_default();
    let serialized = structured_serialize_with_transfer(&value, transfer, context)?;

    // Step 2: Let deserializeRecord be ? StructuredDeserializeWithTransfer(...).
    let desc_result = structured_deserialize_with_transfer(
        &serialized, &JsValue::undefined(), context,
    )?;

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

fn normalize_error_name(name: &str) -> String {
    match name {
        "Error" | "EvalError" | "RangeError" | "ReferenceError"
        | "SyntaxError" | "TypeError" | "URIError" => name.to_string(),
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

fn parse_typed_array_kind(name: &str) -> JsResult<boa_engine::builtins::typed_array::TypedArrayKind> {
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
