//! # `JsTypes` — ECMAScript language type vocabulary
//!
//! ECMA-262 §6 defines language types (Undefined, Null, Boolean, String,
//! Symbol, Number, BigInt, Object) and object subtypes by internal slot
//! profile (`[[ArrayBufferData]]`, `[[PromiseState]]`, etc.).  Each distinct
//! slot profile becomes an associated type.
//!
//! Upcasts (subtype → `JsObject` → `JsValue`) are infallible — every
//! ArrayBuffer IS an Object IS a Value.  Downcasts are fallible — not every
//! Value is a String.
//!
//! See `js_engine/README.md` for the design rationale.

/// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
pub trait JsTypes: Sized + 'static {
    // ── Primitives (§6.1) ────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    type JsString: Clone + Eq + std::hash::Hash;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    type JsSymbol: Clone + Eq;

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    type JsBigInt: Clone + Eq;

    // ── Universal value ─────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-ecmascript-language-types>
    type JsValue: Clone;

    // ── Object types by internal slot profile ───────────────────────────

    /// <https://tc39.es/ecma262/#sec-arraybuffer-objects>
    type JsObject: Clone;

    /// <https://tc39.es/ecma262/#sec-arraybuffer-objects>
    type ArrayBuffer: Clone;

    /// <https://tc39.es/ecma262/#sec-sharedarraybuffer-objects>
    type SharedArrayBuffer: Clone;

    /// <https://tc39.es/ecma262/#sec-typedarray-objects>
    type TypedArray: Clone;

    /// <https://tc39.es/ecma262/#sec-dataview-objects>
    type DataView: Clone;

    /// <https://tc39.es/ecma262/#sec-promise-objects>
    type Promise: Clone;

    /// <https://tc39.es/ecma262/#sec-map-objects>
    type Map: Clone;

    /// <https://tc39.es/ecma262/#sec-set-objects>
    type Set: Clone;

    /// <https://tc39.es/ecma262/#sec-weakmap-objects>
    type WeakMap: Clone;

    /// <https://tc39.es/ecma262/#sec-weakset-objects>
    type WeakSet: Clone;

    /// <https://tc39.es/ecma262/#sec-weakref-objects>
    type WeakRef: Clone;

    /// <https://tc39.es/ecma262/#sec-generator-objects>
    type Generator: Clone;

    /// <https://tc39.es/ecma262/#sec-asyncgenerator-objects>
    type AsyncGenerator: Clone;

    /// <https://tc39.es/ecma262/#sec-ecmascript-function-objects>
    type Function: Clone;

    /// <https://tc39.es/ecma262/#sec-ecmascript-function-objects>
    type Constructor: Clone;

    // ── Property key ────────────────────────────────────────────────────

    /// <https://tc39.es/ecma262/#sec-property-key>
    type PropertyKey: Clone;

    // ── Realm ───────────────────────────────────────────────────────────

    // ── Infallible upcasts ──────────────────────────────────────────────

    fn object_from_array_buffer(ab: Self::ArrayBuffer) -> Self::JsObject;
    fn object_from_shared_array_buffer(sab: Self::SharedArrayBuffer) -> Self::JsObject;
    fn object_from_typed_array(ta: Self::TypedArray) -> Self::JsObject;
    fn object_from_data_view(dv: Self::DataView) -> Self::JsObject;
    fn object_from_promise(p: Self::Promise) -> Self::JsObject;
    fn object_from_map(m: Self::Map) -> Self::JsObject;
    fn object_from_set(s: Self::Set) -> Self::JsObject;
    fn object_from_function(f: Self::Function) -> Self::JsObject;
    fn object_from_constructor(c: Self::Constructor) -> Self::JsObject;

    fn value_from_object(o: Self::JsObject) -> Self::JsValue;
    fn value_from_symbol(sym: Self::JsSymbol) -> Self::JsValue;
    fn value_from_bigint(n: Self::JsBigInt) -> Self::JsValue;

    // ── Fallible downcasts ──────────────────────────────────────────────

    fn value_as_object(v: &Self::JsValue) -> Option<Self::JsObject>;
    fn value_as_string(v: &Self::JsValue) -> Option<Self::JsString>;
    fn value_as_symbol(v: &Self::JsValue) -> Option<Self::JsSymbol>;
    fn value_as_number(v: &Self::JsValue) -> Option<f64>;
    fn value_as_bool(v: &Self::JsValue) -> Option<bool>;
    fn value_as_bigint(v: &Self::JsValue) -> Option<Self::JsBigInt>;
    fn value_is_undefined(v: &Self::JsValue) -> bool;
    fn value_is_null(v: &Self::JsValue) -> bool;

    fn object_as_array_buffer(o: &Self::JsObject) -> Option<Self::ArrayBuffer>;
    fn object_as_shared_array_buffer(o: &Self::JsObject) -> Option<Self::SharedArrayBuffer>;
    fn object_as_typed_array(o: &Self::JsObject) -> Option<Self::TypedArray>;
    fn object_as_data_view(o: &Self::JsObject) -> Option<Self::DataView>;
    fn object_as_promise(o: &Self::JsObject) -> Option<Self::Promise>;
    fn object_as_function(o: &Self::JsObject) -> Option<Self::Function>;
    fn object_as_constructor(o: &Self::JsObject) -> Option<Self::Constructor>;
    fn object_as_map(o: &Self::JsObject) -> Option<Self::Map>;
    fn object_as_set(o: &Self::JsObject) -> Option<Self::Set>;
    fn object_as_weak_map(o: &Self::JsObject) -> Option<Self::WeakMap>;
    fn object_as_weak_set(o: &Self::JsObject) -> Option<Self::WeakSet>;
    fn object_as_weak_ref(o: &Self::JsObject) -> Option<Self::WeakRef>;
    fn object_as_generator(o: &Self::JsObject) -> Option<Self::Generator>;
    fn object_as_async_generator(o: &Self::JsObject) -> Option<Self::AsyncGenerator>;

    // ── ECMAScript wrapper object downcasts (§6.1) ──────────────────

    /// Returns `true` if the object has a [[BooleanData]] internal slot.
    fn object_is_boolean_wrapper(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has a [[NumberData]] internal slot.
    fn object_is_number_wrapper(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has a [[StringData]] internal slot.
    fn object_is_string_wrapper(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has a [[BigIntData]] internal slot.
    fn object_is_bigint_wrapper(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has a [[DateValue]] internal slot.
    fn object_is_date(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has a [[RegExpMatcher]] internal slot.
    fn object_is_regexp(o: &Self::JsObject) -> bool;
    /// Returns `true` if the object has an [[ErrorData]] internal slot.
    fn object_is_error(o: &Self::JsObject) -> bool;

    /// Extract the [[BooleanData]] from a Boolean wrapper object.
    fn boolean_wrapper_data(o: &Self::JsObject) -> Option<bool>;
    /// Extract the [[NumberData]] from a Number wrapper object.
    fn number_wrapper_data(o: &Self::JsObject) -> Option<f64>;
    /// Extract the [[StringData]] from a String wrapper object.
    fn string_wrapper_data(o: &Self::JsObject) -> Option<Self::JsString>;
    /// Extract the [[BigIntData]] from a BigInt wrapper object.
    fn bigint_wrapper_data(o: &Self::JsObject) -> Option<Self::JsBigInt>;
}

/// <https://tc39.es/ecma262/#sec-code-realms>
pub trait JsTypesWithRealm: JsTypes {
    type Realm: Clone;
}
