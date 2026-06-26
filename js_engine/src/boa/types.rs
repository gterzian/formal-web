//! `BoaTypes` — the `JsTypes` / `JsTypesWithRealm` marker for the Boa backend.
//!
//! Maps every ECMAScript language type and object subtype (§6.1) to its Boa
//! counterpart.  Upcasts use `From` impls (`JsObject::from(array_buffer)`);
//! downcasts use `JsArrayBuffer::from_object(object).ok()`.
//!
//! See `js_engine/README.md` and the [`super`] module docs for the full
//! Boa backend status.

use boa_engine::{
    object::builtins::{
        JsArrayBuffer, JsAsyncGenerator, JsDataView, JsFunction, JsGenerator, JsMap, JsPromise,
        JsSet, JsSharedArrayBuffer, JsTypedArray, JsWeakMap, JsWeakSet,
    },
    object::JsObject,
    JsValue,
};

use crate::{JsTypes, JsTypesWithRealm};

/// Marker type for Boa engine implementations.
#[derive(Debug, Clone, Copy)]
pub struct BoaTypes;

impl JsTypes for BoaTypes {
    type JsString = boa_engine::JsString;
    type JsSymbol = boa_engine::JsSymbol;
    type JsBigInt = boa_engine::JsBigInt;
    type JsValue = boa_engine::JsValue;
    type JsObject = boa_engine::JsObject;
    type ArrayBuffer = boa_engine::object::builtins::JsArrayBuffer;
    type SharedArrayBuffer = boa_engine::object::builtins::JsSharedArrayBuffer;
    type TypedArray = boa_engine::object::builtins::JsTypedArray;
    type DataView = boa_engine::object::builtins::JsDataView;
    type Promise = boa_engine::object::builtins::JsPromise;
    type Map = boa_engine::object::builtins::JsMap;
    type Set = boa_engine::object::builtins::JsSet;
    type WeakMap = boa_engine::object::builtins::JsWeakMap;
    type WeakSet = boa_engine::object::builtins::JsWeakSet;
    type WeakRef = boa_engine::JsObject;
    type Generator = boa_engine::object::builtins::JsGenerator;
    type AsyncGenerator = boa_engine::object::builtins::JsAsyncGenerator;
    type Function = boa_engine::object::builtins::JsFunction;
    type Constructor = boa_engine::object::builtins::JsFunction;
    type PropertyKey = boa_engine::property::PropertyKey;

    // ── Upcasts (owned conversions using From impls) ─────────────────────

    fn object_from_array_buffer(ab: Self::ArrayBuffer) -> Self::JsObject {
        JsObject::from(ab)
    }
    fn object_from_shared_array_buffer(sab: Self::SharedArrayBuffer) -> Self::JsObject {
        JsObject::from(sab)
    }
    fn object_from_typed_array(ta: Self::TypedArray) -> Self::JsObject {
        JsObject::from(ta)
    }
    fn object_from_data_view(dv: Self::DataView) -> Self::JsObject {
        JsObject::from(dv)
    }
    fn object_from_promise(p: Self::Promise) -> Self::JsObject {
        JsObject::from(p)
    }
    fn object_from_map(m: Self::Map) -> Self::JsObject {
        JsObject::from(m)
    }
    fn object_from_set(s: Self::Set) -> Self::JsObject {
        JsObject::from(s)
    }
    fn object_from_function(f: Self::Function) -> Self::JsObject {
        JsObject::from(f)
    }
    fn object_from_constructor(c: Self::Constructor) -> Self::JsObject {
        JsObject::from(c)
    }

    fn value_from_object(o: Self::JsObject) -> Self::JsValue {
        JsValue::from(o)
    }
    fn value_from_symbol(sym: Self::JsSymbol) -> Self::JsValue {
        JsValue::from(sym)
    }
    fn value_from_bigint(n: Self::JsBigInt) -> Self::JsValue {
        JsValue::from(n)
    }

    // ── Downcasts ────────────────────────────────────────────────────

    fn value_as_object(v: &Self::JsValue) -> Option<Self::JsObject> {
        v.as_object()
    }
    fn value_as_string(v: &Self::JsValue) -> Option<Self::JsString> {
        v.as_string()
    }
    fn value_as_symbol(v: &Self::JsValue) -> Option<Self::JsSymbol> {
        v.as_symbol()
    }
    fn value_as_number(v: &Self::JsValue) -> Option<f64> {
        v.as_number()
    }
    fn value_as_bool(v: &Self::JsValue) -> Option<bool> {
        v.as_boolean()
    }
    fn value_is_undefined(v: &Self::JsValue) -> bool {
        v.is_undefined()
    }
    fn value_is_null(v: &Self::JsValue) -> bool {
        v.is_null()
    }

    fn object_as_array_buffer(o: &Self::JsObject) -> Option<Self::ArrayBuffer> {
        JsArrayBuffer::from_object(o.clone()).ok()
    }
    fn object_as_shared_array_buffer(o: &Self::JsObject) -> Option<Self::SharedArrayBuffer> {
        JsSharedArrayBuffer::from_object(o.clone()).ok()
    }
    fn object_as_typed_array(o: &Self::JsObject) -> Option<Self::TypedArray> {
        JsTypedArray::from_object(o.clone()).ok()
    }
    fn object_as_data_view(o: &Self::JsObject) -> Option<Self::DataView> {
        JsDataView::from_object(o.clone()).ok()
    }
    fn object_as_promise(o: &Self::JsObject) -> Option<Self::Promise> {
        JsPromise::from_object(o.clone()).ok()
    }
    fn object_as_function(o: &Self::JsObject) -> Option<Self::Function> {
        JsFunction::from_object(o.clone())
    }
    fn object_as_constructor(o: &Self::JsObject) -> Option<Self::Constructor> {
        JsFunction::from_object(o.clone())
    }
    fn object_as_map(o: &Self::JsObject) -> Option<Self::Map> {
        JsMap::from_object(o.clone()).ok()
    }
    fn object_as_set(o: &Self::JsObject) -> Option<Self::Set> {
        JsSet::from_object(o.clone()).ok()
    }
    fn object_as_weak_map(o: &Self::JsObject) -> Option<Self::WeakMap> {
        JsWeakMap::from_object(o.clone()).ok()
    }
    fn object_as_weak_set(o: &Self::JsObject) -> Option<Self::WeakSet> {
        JsWeakSet::from_object(o.clone()).ok()
    }
    fn object_as_generator(o: &Self::JsObject) -> Option<Self::Generator> {
        JsGenerator::from_object(o.clone()).ok()
    }
    fn object_as_async_generator(o: &Self::JsObject) -> Option<Self::AsyncGenerator> {
        JsAsyncGenerator::from_object(o.clone()).ok()
    }
}

impl JsTypesWithRealm for BoaTypes {
    type Realm = boa_engine::realm::Realm;
}
