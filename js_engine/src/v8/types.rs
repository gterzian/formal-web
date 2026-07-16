use std::borrow::Borrow;
use std::cell::Cell;
use std::ffi::c_void;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;
use std::sync::Arc;

use rusty_v8 as v8;

use crate::{JsTypes, JsTypesWithRealm};
use crate::TypedArrayElementType;

#[derive(Clone, Debug)]
pub(crate) enum CachedPrimitive {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(Arc<[u16]>),
    BigInt(Arc<str>),
    Other,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ObjectProfile {
    pub is_array_buffer: bool,
    pub is_shared_array_buffer: bool,
    pub is_typed_array: bool,
    pub is_data_view: bool,
    pub is_promise: bool,
    pub is_function: bool,
    pub is_constructor: bool,
    pub is_map: bool,
    pub is_set: bool,
    pub is_weak_map: bool,
    pub is_weak_set: bool,
    pub is_generator: bool,
    pub is_boolean_wrapper: bool,
    pub is_number_wrapper: bool,
    pub is_string_wrapper: bool,
    pub is_bigint_wrapper: bool,
    pub is_date: bool,
    pub is_regexp: bool,
    pub is_error: bool,
    pub wrapper_primitive: Option<CachedPrimitive>,
    pub array_buffer_state: Option<V8ArrayBufferState>,
    pub typed_array_element_type: Option<TypedArrayElementType>,
}

#[derive(Clone)]
pub(crate) struct V8ArrayBufferState {
    pub backing_store: v8::SharedRef<v8::BackingStore>,
    pub detached: std::rc::Rc<Cell<bool>>,
    pub resizable: bool,
}

impl std::fmt::Debug for V8ArrayBufferState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("V8ArrayBufferState")
            .field("byte_length", &self.backing_store.byte_length())
            .field("detached", &self.detached.get())
            .field("resizable", &self.resizable)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct V8Value {
    pub(crate) isolate_id: u64,
    pub(crate) handle: v8::Global<v8::Value>,
    pub(crate) primitive: CachedPrimitive,
    pub(crate) object_profile: Option<ObjectProfile>,
    pub(crate) host_data: Option<NonNull<c_void>>,
}

impl V8Value {
    pub fn is_undefined(&self) -> bool {
        matches!(self.primitive, CachedPrimitive::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self.primitive, CachedPrimitive::Null)
    }

    pub fn as_object(&self) -> Option<V8Object> {
        self.object_profile.as_ref()?;
        Some(V8Object(self.clone()))
    }

    pub fn as_string(&self) -> Option<V8String> {
        V8Types::cached_string(self)
    }

    pub fn display(&self) -> String {
        match &self.primitive {
            CachedPrimitive::Undefined => String::from("undefined"),
            CachedPrimitive::Null => String::from("null"),
            CachedPrimitive::Boolean(boolean) => boolean.to_string(),
            CachedPrimitive::Number(number) => number.to_string(),
            CachedPrimitive::String(utf16) => String::from_utf16_lossy(utf16),
            CachedPrimitive::BigInt(canonical) => canonical.to_string(),
            CachedPrimitive::Other if self.object_profile.is_some() => String::from("[object]"),
            CachedPrimitive::Other => String::from("[value]"),
        }
    }
}

impl fmt::Display for V8Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display())
    }
}

#[derive(Clone, Debug)]
pub struct V8Object(pub(crate) V8Value);

impl From<V8Object> for V8Value {
    fn from(object: V8Object) -> Self {
        object.0
    }
}

impl PartialEq for V8Object {
    fn eq(&self, other: &Self) -> bool {
        self.0.isolate_id == other.0.isolate_id && self.0.handle == other.0.handle
    }
}

#[derive(Clone, Debug)]
pub struct V8String {
    pub(crate) value: Option<V8Value>,
    pub(crate) utf16: Arc<[u16]>,
}

impl PartialEq for V8String {
    fn eq(&self, other: &Self) -> bool {
        self.utf16 == other.utf16
    }
}

impl PartialEq<str> for V8String {
    fn eq(&self, other: &str) -> bool {
        self.utf16.iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for V8String {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl Eq for V8String {}

impl Hash for V8String {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.utf16.hash(state);
    }
}

#[derive(Clone, Debug)]
pub struct V8Symbol(pub(crate) V8Value);

impl PartialEq for V8Symbol {
    fn eq(&self, other: &Self) -> bool {
        self.0.isolate_id == other.0.isolate_id && self.0.handle == other.0.handle
    }
}

impl Eq for V8Symbol {}

#[derive(Clone, Debug)]
pub struct V8BigInt {
    pub(crate) value: V8Value,
    pub(crate) canonical: Arc<str>,
}

impl PartialEq for V8BigInt {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for V8BigInt {}

#[derive(Clone, Debug)]
pub enum V8PropertyKey {
    String(V8String),
    Symbol(V8Symbol),
    Index(u32),
}

#[derive(Clone, Debug)]
pub struct V8Realm {
    pub(crate) isolate_id: u64,
    pub(crate) context: v8::Global<v8::Context>,
}

#[derive(Clone, Debug)]
pub struct V8Types;

impl V8Types {
    fn object_if(value: &V8Value, predicate: impl FnOnce(&ObjectProfile) -> bool) -> Option<V8Object> {
        value
            .object_profile
            .as_ref()
            .is_some_and(predicate)
            .then(|| V8Object(value.clone()))
    }

    fn cached_string(value: &V8Value) -> Option<V8String> {
        let CachedPrimitive::String(utf16) = &value.primitive else {
            return None;
        };
        Some(V8String {
            value: Some(value.clone()),
            utf16: utf16.clone(),
        })
    }

    fn cached_bigint(value: &V8Value) -> Option<V8BigInt> {
        let CachedPrimitive::BigInt(canonical) = &value.primitive else {
            return None;
        };
        Some(V8BigInt {
            value: value.clone(),
            canonical: canonical.clone(),
        })
    }
}

impl JsTypes for V8Types {
    type JsString = V8String;
    type JsSymbol = V8Symbol;
    type JsBigInt = V8BigInt;
    type JsValue = V8Value;
    type JsObject = V8Object;
    type ArrayBuffer = V8Object;
    type SharedArrayBuffer = V8Object;
    type TypedArray = V8Object;
    type DataView = V8Object;
    type Promise = V8Object;
    type Map = V8Object;
    type Set = V8Object;
    type WeakMap = V8Object;
    type WeakSet = V8Object;
    type WeakRef = V8Object;
    type Generator = V8Object;
    type AsyncGenerator = V8Object;
    type Function = V8Object;
    type Constructor = V8Object;
    type PropertyKey = V8PropertyKey;

    fn object_from_array_buffer(value: Self::ArrayBuffer) -> Self::JsObject {
        value
    }

    fn object_from_shared_array_buffer(value: Self::SharedArrayBuffer) -> Self::JsObject {
        value
    }

    fn object_from_typed_array(value: Self::TypedArray) -> Self::JsObject {
        value
    }

    fn object_from_data_view(value: Self::DataView) -> Self::JsObject {
        value
    }

    fn object_from_promise(value: Self::Promise) -> Self::JsObject {
        value
    }

    fn object_from_map(value: Self::Map) -> Self::JsObject {
        value
    }

    fn object_from_set(value: Self::Set) -> Self::JsObject {
        value
    }

    fn object_from_function(value: Self::Function) -> Self::JsObject {
        value
    }

    fn object_from_constructor(value: Self::Constructor) -> Self::JsObject {
        value
    }

    fn value_from_object(value: Self::JsObject) -> Self::JsValue {
        value.0
    }

    fn value_from_symbol(value: Self::JsSymbol) -> Self::JsValue {
        value.0
    }

    fn value_from_bigint(value: Self::JsBigInt) -> Self::JsValue {
        value.value
    }

    fn value_as_object(value: &Self::JsValue) -> Option<Self::JsObject> {
        value.object_profile.as_ref()?;
        Some(V8Object(value.clone()))
    }

    fn value_as_string(value: &Self::JsValue) -> Option<Self::JsString> {
        Self::cached_string(value)
    }

    fn value_as_symbol(value: &Self::JsValue) -> Option<Self::JsSymbol> {
        <v8::Global<v8::Value> as Borrow<v8::Value>>::borrow(&value.handle)
            .is_symbol()
            .then(|| V8Symbol(value.clone()))
    }

    fn value_as_number(value: &Self::JsValue) -> Option<f64> {
        match value.primitive {
            CachedPrimitive::Number(number) => Some(number),
            _ => None,
        }
    }

    fn value_as_bool(value: &Self::JsValue) -> Option<bool> {
        match value.primitive {
            CachedPrimitive::Boolean(boolean) => Some(boolean),
            _ => None,
        }
    }

    fn value_as_bigint(value: &Self::JsValue) -> Option<Self::JsBigInt> {
        Self::cached_bigint(value)
    }

    fn value_is_undefined(value: &Self::JsValue) -> bool {
        matches!(value.primitive, CachedPrimitive::Undefined)
    }

    fn value_is_null(value: &Self::JsValue) -> bool {
        matches!(value.primitive, CachedPrimitive::Null)
    }

    fn object_as_array_buffer(object: &Self::JsObject) -> Option<Self::ArrayBuffer> {
        Self::object_if(&object.0, |profile| profile.is_array_buffer)
    }

    fn object_as_shared_array_buffer(object: &Self::JsObject) -> Option<Self::SharedArrayBuffer> {
        Self::object_if(&object.0, |profile| profile.is_shared_array_buffer)
    }

    fn object_as_typed_array(object: &Self::JsObject) -> Option<Self::TypedArray> {
        Self::object_if(&object.0, |profile| profile.is_typed_array)
    }

    fn object_as_data_view(object: &Self::JsObject) -> Option<Self::DataView> {
        Self::object_if(&object.0, |profile| profile.is_data_view)
    }

    fn object_as_promise(object: &Self::JsObject) -> Option<Self::Promise> {
        Self::object_if(&object.0, |profile| profile.is_promise)
    }

    fn object_as_function(object: &Self::JsObject) -> Option<Self::Function> {
        Self::object_if(&object.0, |profile| profile.is_function)
    }

    fn object_as_constructor(object: &Self::JsObject) -> Option<Self::Constructor> {
        Self::object_if(&object.0, |profile| profile.is_constructor)
    }

    fn object_as_map(object: &Self::JsObject) -> Option<Self::Map> {
        Self::object_if(&object.0, |profile| profile.is_map)
    }

    fn object_as_set(object: &Self::JsObject) -> Option<Self::Set> {
        Self::object_if(&object.0, |profile| profile.is_set)
    }

    fn object_as_weak_map(object: &Self::JsObject) -> Option<Self::WeakMap> {
        Self::object_if(&object.0, |profile| profile.is_weak_map)
    }

    fn object_as_weak_set(object: &Self::JsObject) -> Option<Self::WeakSet> {
        Self::object_if(&object.0, |profile| profile.is_weak_set)
    }

    fn object_as_weak_ref(_object: &Self::JsObject) -> Option<Self::WeakRef> {
        None
    }

    fn object_as_generator(object: &Self::JsObject) -> Option<Self::Generator> {
        Self::object_if(&object.0, |profile| profile.is_generator)
    }

    fn object_as_async_generator(_object: &Self::JsObject) -> Option<Self::AsyncGenerator> {
        None
    }

    fn object_is_boolean_wrapper(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_boolean_wrapper)
    }

    fn object_is_number_wrapper(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_number_wrapper)
    }

    fn object_is_string_wrapper(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_string_wrapper)
    }

    fn object_is_bigint_wrapper(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_bigint_wrapper)
    }

    fn object_is_date(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_date)
    }

    fn object_is_regexp(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_regexp)
    }

    fn object_is_error(object: &Self::JsObject) -> bool {
        object.0.object_profile.as_ref().is_some_and(|profile| profile.is_error)
    }

    fn boolean_wrapper_data(object: &Self::JsObject) -> Option<bool> {
        match object.0.object_profile.as_ref()?.wrapper_primitive.as_ref()? {
            CachedPrimitive::Boolean(boolean) => Some(*boolean),
            _ => None,
        }
    }

    fn number_wrapper_data(object: &Self::JsObject) -> Option<f64> {
        match object.0.object_profile.as_ref()?.wrapper_primitive.as_ref()? {
            CachedPrimitive::Number(number) => Some(*number),
            _ => None,
        }
    }

    fn string_wrapper_data(object: &Self::JsObject) -> Option<Self::JsString> {
        let CachedPrimitive::String(utf16) = object
            .0
            .object_profile
            .as_ref()?
            .wrapper_primitive
            .as_ref()?
        else {
            return None;
        };
        Some(V8String {
            value: Some(object.0.clone()),
            utf16: utf16.clone(),
        })
    }

    fn bigint_wrapper_data(object: &Self::JsObject) -> Option<Self::JsBigInt> {
        let CachedPrimitive::BigInt(canonical) = object
            .0
            .object_profile
            .as_ref()?
            .wrapper_primitive
            .as_ref()?
        else {
            return None;
        };
        Some(V8BigInt {
            value: object.0.clone(),
            canonical: canonical.clone(),
        })
    }
}

impl JsTypesWithRealm for V8Types {
    type Realm = V8Realm;
}
