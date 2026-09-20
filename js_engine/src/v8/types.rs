use std::cell::Cell;
use std::ffi::c_void;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;
use std::sync::Arc;

use rusty_v8 as v8;

use crate::TypedArrayElementType;
use crate::{JsTypes, JsTypesWithRealm};

#[derive(Clone, Debug)]
pub(crate) enum CachedPrimitive {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(Arc<[u16]>),
    BigInt(Arc<str>),
    Symbol,
    Other,
}

/// Whether an optional handle still needs `store_edge`.
pub(crate) fn optional_handle_is_root<T>(handle: &Option<V8Handle<T>>) -> bool {
    handle.as_ref().is_some_and(V8Handle::is_root)
}

#[derive(Clone, Debug)]
pub(crate) struct ObjectProfile {
    pub object_handle: V8Handle<v8::Object>,
    pub array_buffer_handle: Option<V8Handle<v8::ArrayBuffer>>,
    pub shared_array_buffer_handle: Option<V8Handle<v8::SharedArrayBuffer>>,
    pub typed_array_handle: Option<V8Handle<v8::TypedArray>>,
    pub data_view_handle: Option<V8Handle<v8::DataView>>,
    pub promise_handle: Option<V8Handle<v8::Promise>>,
    pub function_handle: Option<V8Handle<v8::Function>>,
    pub map_handle: Option<V8Handle<v8::Map>>,
    pub set_handle: Option<V8Handle<v8::Set>>,
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
    pub is_constructor: bool,
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

impl ObjectProfile {
    /// Whether any handle in this profile is still a strong root. Used to
    /// skip opening a V8 value scope when there is nothing to convert.
    pub(crate) fn needs_store(&self) -> bool {
        self.object_handle.is_root()
            || optional_handle_is_root(&self.array_buffer_handle)
            || optional_handle_is_root(&self.shared_array_buffer_handle)
            || optional_handle_is_root(&self.typed_array_handle)
            || optional_handle_is_root(&self.data_view_handle)
            || optional_handle_is_root(&self.promise_handle)
            || optional_handle_is_root(&self.function_handle)
            || optional_handle_is_root(&self.map_handle)
            || optional_handle_is_root(&self.set_handle)
    }
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

/// A JS-object handle with two representations.
///
/// - [`V8Handle::Root`] — a strong `v8::Global` root. Values created from
///   V8 locals and held on the Rust stack use this: a bare cppgc edge would
///   not keep the referent alive while Rust holds it.
/// - [`V8Handle::Edge`] — a `TracedReference` edge. Values stored inside
///   traced storage (cells, platform objects) use this: the referent stays
///   alive only while an owning cppgc object traces the edge, so cycles
///   spanning the JS heap and the cppgc heap are collectable.
///
/// Conversion happens at storage boundaries ([`V8Handle::store_edge`]);
/// clones share a single edge through the `Rc`.
pub(crate) enum V8Handle<T> {
    Root(v8::Global<T>),
    Edge(std::rc::Rc<v8::TracedReference<T>>),
}

impl<T> Clone for V8Handle<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Root(global) => Self::Root(global.clone()),
            Self::Edge(edge) => Self::Edge(std::rc::Rc::clone(edge)),
        }
    }
}

impl<T> V8Handle<T> {
    /// Materialize a `Local` for this handle in the given scope. `None` when
    /// the referent has been reclaimed (a stale edge) or the handle is
    /// empty.
    pub(crate) fn to_local<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_, ()>,
    ) -> Option<v8::Local<'s, T>> {
        match self {
            Self::Root(global) => Some(v8::Local::new(scope, global)),
            Self::Edge(edge) => edge.get(scope),
        }
    }

    /// Whether this handle is still a strong `v8::Global` root that must be
    /// converted to a cppgc edge before it enters traced storage.
    pub(crate) fn is_root(&self) -> bool {
        matches!(self, Self::Root(_))
    }

    /// Convert a rooted handle into a cppgc edge. Idempotent for edges.
    pub(crate) fn store_edge(&mut self, scope: &mut v8::PinScope<'_, '_, ()>) {
        if let Self::Root(global) = self {
            let local = v8::Local::new(scope, &*global);
            *self = Self::Edge(std::rc::Rc::new(v8::TracedReference::new(scope, local)));
        }
    }

    /// Scope-free identity comparison. Root handles compare by object
    /// identity (V8 keeps the handle cell updated); edges compare by shared
    /// storage cell, which is exact for clones of one edge but under-approximates
    /// two independently-created edges to the same object (use the engine's
    /// `same_value` for those).
    pub(crate) fn same_identity(&self, other: &Self) -> bool
    where
        T: PartialEq,
    {
        match (self, other) {
            (Self::Root(left), Self::Root(right)) => left == right,
            (Self::Edge(left), Self::Edge(right)) => std::rc::Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for V8Handle<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root(handle) => formatter
                .debug_tuple("V8Handle::Root")
                .field(handle)
                .finish(),
            Self::Edge(_) => formatter.write_str("V8Handle::Edge"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct V8Value {
    pub(crate) isolate_id: u64,
    pub(crate) handle: V8Handle<v8::Value>,
    pub(crate) primitive: CachedPrimitive,
    pub(crate) object_profile: Option<Box<ObjectProfile>>,
    pub(crate) host_data: Option<NonNull<c_void>>,
}

impl V8Value {
    /// Whether any handle in this value is still a strong root. Used to skip
    /// opening a V8 value scope when there is nothing to convert.
    pub(crate) fn needs_store(&self) -> bool {
        self.handle.is_root()
            || self
                .object_profile
                .as_ref()
                .is_some_and(|profile| profile.needs_store())
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self.primitive, CachedPrimitive::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self.primitive, CachedPrimitive::Null)
    }

    pub fn as_object(&self) -> Option<V8Object> {
        V8Object::from_value(self.clone())
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
            CachedPrimitive::Symbol => String::from("[symbol]"),
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
pub struct V8Object(pub(crate) V8Value, pub(crate) V8Handle<v8::Object>);

impl V8Object {
    pub(crate) fn from_value(value: V8Value) -> Option<Self> {
        let handle = value.object_profile.as_ref()?.object_handle.clone();
        Some(Self(value, handle))
    }
}

impl From<V8Object> for V8Value {
    fn from(object: V8Object) -> Self {
        object.0
    }
}

impl PartialEq for V8Object {
    fn eq(&self, other: &Self) -> bool {
        self.0.isolate_id == other.0.isolate_id
            && self.0.handle.same_identity(&other.0.handle)
            && self.1.same_identity(&other.1)
    }
}

macro_rules! typed_v8_object {
    ($name:ident, $handle:ty) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub(crate) V8Object, pub(crate) V8Handle<$handle>);

        impl From<$name> for V8Object {
            fn from(value: $name) -> Self {
                let $name(object, _handle) = value;
                object
            }
        }

        impl AsRef<V8Object> for $name {
            fn as_ref(&self) -> &V8Object {
                &self.0
            }
        }
    };
}

macro_rules! tagged_v8_object {
    ($name:ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub(crate) V8Object);

        impl From<$name> for V8Object {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<V8Object> for $name {
            fn as_ref(&self) -> &V8Object {
                &self.0
            }
        }
    };
}

typed_v8_object!(V8ArrayBuffer, v8::ArrayBuffer);
typed_v8_object!(V8SharedArrayBuffer, v8::SharedArrayBuffer);
typed_v8_object!(V8TypedArray, v8::TypedArray);
typed_v8_object!(V8DataView, v8::DataView);
typed_v8_object!(V8Promise, v8::Promise);
typed_v8_object!(V8Map, v8::Map);
typed_v8_object!(V8Set, v8::Set);
tagged_v8_object!(V8WeakMap);
tagged_v8_object!(V8WeakSet);
tagged_v8_object!(V8WeakRef);
tagged_v8_object!(V8Generator);
tagged_v8_object!(V8AsyncGenerator);
typed_v8_object!(V8Function, v8::Function);
typed_v8_object!(V8Constructor, v8::Function);

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
        self.0.isolate_id == other.0.isolate_id && self.0.handle.same_identity(&other.0.handle)
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
    type ArrayBuffer = V8ArrayBuffer;
    type SharedArrayBuffer = V8SharedArrayBuffer;
    type TypedArray = V8TypedArray;
    type DataView = V8DataView;
    type Promise = V8Promise;
    type Map = V8Map;
    type Set = V8Set;
    type WeakMap = V8WeakMap;
    type WeakSet = V8WeakSet;
    type WeakRef = V8WeakRef;
    type Generator = V8Generator;
    type AsyncGenerator = V8AsyncGenerator;
    type Function = V8Function;
    type Constructor = V8Constructor;
    type PropertyKey = V8PropertyKey;

    fn object_from_array_buffer(value: Self::ArrayBuffer) -> Self::JsObject {
        value.into()
    }

    fn object_from_shared_array_buffer(value: Self::SharedArrayBuffer) -> Self::JsObject {
        value.into()
    }

    fn object_from_typed_array(value: Self::TypedArray) -> Self::JsObject {
        value.into()
    }

    fn object_from_data_view(value: Self::DataView) -> Self::JsObject {
        value.into()
    }

    fn object_from_promise(value: Self::Promise) -> Self::JsObject {
        value.into()
    }

    fn object_from_map(value: Self::Map) -> Self::JsObject {
        value.into()
    }

    fn object_from_set(value: Self::Set) -> Self::JsObject {
        value.into()
    }

    fn object_from_function(value: Self::Function) -> Self::JsObject {
        value.into()
    }

    fn object_from_constructor(value: Self::Constructor) -> Self::JsObject {
        value.into()
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
        V8Object::from_value(value.clone())
    }

    fn value_as_string(value: &Self::JsValue) -> Option<Self::JsString> {
        Self::cached_string(value)
    }

    fn value_as_symbol(value: &Self::JsValue) -> Option<Self::JsSymbol> {
        matches!(value.primitive, CachedPrimitive::Symbol).then(|| V8Symbol(value.clone()))
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
        let handle = object
            .0
            .object_profile
            .as_ref()?
            .array_buffer_handle
            .clone()?;
        Some(V8ArrayBuffer(object.clone(), handle))
    }

    fn object_as_shared_array_buffer(object: &Self::JsObject) -> Option<Self::SharedArrayBuffer> {
        let handle = object
            .0
            .object_profile
            .as_ref()?
            .shared_array_buffer_handle
            .clone()?;
        Some(V8SharedArrayBuffer(object.clone(), handle))
    }

    fn object_as_typed_array(object: &Self::JsObject) -> Option<Self::TypedArray> {
        let handle = object
            .0
            .object_profile
            .as_ref()?
            .typed_array_handle
            .clone()?;
        Some(V8TypedArray(object.clone(), handle))
    }

    fn object_as_data_view(object: &Self::JsObject) -> Option<Self::DataView> {
        let handle = object.0.object_profile.as_ref()?.data_view_handle.clone()?;
        Some(V8DataView(object.clone(), handle))
    }

    fn object_as_promise(object: &Self::JsObject) -> Option<Self::Promise> {
        let handle = object.0.object_profile.as_ref()?.promise_handle.clone()?;
        Some(V8Promise(object.clone(), handle))
    }

    fn object_as_function(object: &Self::JsObject) -> Option<Self::Function> {
        let handle = object.0.object_profile.as_ref()?.function_handle.clone()?;
        Some(V8Function(object.clone(), handle))
    }

    fn object_as_constructor(object: &Self::JsObject) -> Option<Self::Constructor> {
        let profile = object.0.object_profile.as_ref()?;
        // Arrow, async, generator, and bound functions are callable but have
        // no [[Construct]]; the profile records this at wrap time (see
        // `wrap_local_value`), so only true constructors convert.
        if !profile.is_constructor {
            return None;
        }
        Some(V8Constructor(
            object.clone(),
            profile.function_handle.clone()?,
        ))
    }

    fn object_as_map(object: &Self::JsObject) -> Option<Self::Map> {
        let handle = object.0.object_profile.as_ref()?.map_handle.clone()?;
        Some(V8Map(object.clone(), handle))
    }

    fn object_as_set(object: &Self::JsObject) -> Option<Self::Set> {
        let handle = object.0.object_profile.as_ref()?.set_handle.clone()?;
        Some(V8Set(object.clone(), handle))
    }

    fn object_as_weak_map(object: &Self::JsObject) -> Option<Self::WeakMap> {
        object
            .0
            .object_profile
            .as_ref()?
            .is_weak_map
            .then(|| V8WeakMap(object.clone()))
    }

    fn object_as_weak_set(object: &Self::JsObject) -> Option<Self::WeakSet> {
        object
            .0
            .object_profile
            .as_ref()?
            .is_weak_set
            .then(|| V8WeakSet(object.clone()))
    }

    fn object_as_weak_ref(_object: &Self::JsObject) -> Option<Self::WeakRef> {
        None
    }

    fn object_as_generator(object: &Self::JsObject) -> Option<Self::Generator> {
        object
            .0
            .object_profile
            .as_ref()?
            .is_generator
            .then(|| V8Generator(object.clone()))
    }

    fn object_as_async_generator(_object: &Self::JsObject) -> Option<Self::AsyncGenerator> {
        None
    }

    fn object_is_boolean_wrapper(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_boolean_wrapper)
    }

    fn object_is_number_wrapper(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_number_wrapper)
    }

    fn object_is_string_wrapper(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_string_wrapper)
    }

    fn object_is_bigint_wrapper(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_bigint_wrapper)
    }

    fn object_is_date(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_date)
    }

    fn object_is_regexp(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_regexp)
    }

    fn object_is_error(object: &Self::JsObject) -> bool {
        object
            .0
            .object_profile
            .as_ref()
            .is_some_and(|profile| profile.is_error)
    }

    fn boolean_wrapper_data(object: &Self::JsObject) -> Option<bool> {
        match object
            .0
            .object_profile
            .as_ref()?
            .wrapper_primitive
            .as_ref()?
        {
            CachedPrimitive::Boolean(boolean) => Some(*boolean),
            _ => None,
        }
    }

    fn number_wrapper_data(object: &Self::JsObject) -> Option<f64> {
        match object
            .0
            .object_profile
            .as_ref()?
            .wrapper_primitive
            .as_ref()?
        {
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
