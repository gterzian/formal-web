//! JSC engine wrapper implementing `JsEngine<JscTypes>`.
//!
//! # Hard problems (not yet implemented)
//!
//! - **Jobs/microtasks** — JSC's C API doesn't expose the microtask queue.
//! - **Promise operations** — `JSObjectMakePromise` is not in the public C API.
//! - **TypedArray/ArrayBuffer** — basic creation available, GetValueFromBuffer etc. not.
//! - **Generator operations** — no public C API for generator control.
//! - **Module evaluation** — requires SPI.
//! - **SharedArrayBuffer** — available on newer macOS only.

use std::marker::PhantomData;

use super::sys::*;
use super::types::*;
use crate::{
    Completion, HostHooks, IntegrityLevel, IteratorKind, JsEngine, JsTypes, JsTypesWithRealm,
    Numeric, PreferredType, SharedMemoryOrder, TypedArrayElementType,
    records::{IteratorRecord, PromiseCapability, PropertyDescriptor, RealmIntrinsics},
};

/// Marker type for JSC engine implementations.
pub struct JscTypes;

impl JsTypes for JscTypes {
    type JsString = JscString;
    type JsSymbol = JscSymbol;
    type JsBigInt = JscBigInt;
    type JsValue = JscValue;
    type JsObject = JscObject;
    type ArrayBuffer = JscArrayBuffer;
    type SharedArrayBuffer = JscSharedArrayBuffer;
    type TypedArray = JscTypedArray;
    type DataView = JscDataView;
    type Promise = JscPromise;
    type Map = JscMap;
    type Set = JscSet;
    type WeakMap = JscWeakMap;
    type WeakSet = JscWeakSet;
    type WeakRef = JscWeakRef;
    type Generator = JscGenerator;
    type AsyncGenerator = JscAsyncGenerator;
    type Function = JscFunction;
    type Constructor = JscConstructor;
    type PropertyKey = JscPropertyKey;

    // ── Upcasts ──────────────────────────────────────────────────────
    fn object_from_array_buffer(ab: Self::ArrayBuffer) -> Self::JsObject { ab }
    fn object_from_shared_array_buffer(sab: Self::SharedArrayBuffer) -> Self::JsObject { sab }
    fn object_from_typed_array(ta: Self::TypedArray) -> Self::JsObject { ta }
    fn object_from_data_view(dv: Self::DataView) -> Self::JsObject { dv }
    fn object_from_promise(p: Self::Promise) -> Self::JsObject { p }
    fn object_from_map(m: Self::Map) -> Self::JsObject { m }
    fn object_from_set(s: Self::Set) -> Self::JsObject { s }
    fn object_from_function(f: Self::Function) -> Self::JsObject { f }
    fn object_from_constructor(c: Self::Constructor) -> Self::JsObject { c }

    fn value_from_object(o: Self::JsObject) -> Self::JsValue { o.as_value() }
    fn value_from_string(_s: Self::JsString) -> Self::JsValue {
        unimplemented!("value_from_string requires a context in JSC")
    }
    fn value_from_symbol(sym: Self::JsSymbol) -> Self::JsValue { sym.as_value().clone() }
    fn value_from_bool(_b: bool) -> Self::JsValue {
        unimplemented!("value_from_bool requires a context in JSC")
    }
    fn value_from_number(_n: f64) -> Self::JsValue {
        unimplemented!("value_from_number requires a context in JSC")
    }
    fn value_from_bigint(n: Self::JsBigInt) -> Self::JsValue { n.as_value().clone() }
    fn value_undefined() -> Self::JsValue { unimplemented!("requires a context in JSC") }
    fn value_null() -> Self::JsValue { unimplemented!("requires a context in JSC") }

    // ── Downcasts ────────────────────────────────────────────────────
    fn value_as_object(_v: &Self::JsValue) -> Option<Self::JsObject> { None /* needs context */ }
    fn value_as_string(_v: &Self::JsValue) -> Option<Self::JsString> { None }
    fn value_as_symbol(_v: &Self::JsValue) -> Option<Self::JsSymbol> { None }
    fn value_as_number(_v: &Self::JsValue) -> Option<f64> { None }
    fn value_as_bool(_v: &Self::JsValue) -> Option<bool> { None }
    fn value_is_undefined(_v: &Self::JsValue) -> bool { false }
    fn value_is_null(_v: &Self::JsValue) -> bool { false }

    fn object_as_array_buffer(o: &Self::JsObject) -> Option<Self::ArrayBuffer> { Some(*o) }
    fn object_as_shared_array_buffer(o: &Self::JsObject) -> Option<Self::SharedArrayBuffer> { Some(*o) }
    fn object_as_typed_array(o: &Self::JsObject) -> Option<Self::TypedArray> { Some(*o) }
    fn object_as_data_view(o: &Self::JsObject) -> Option<Self::DataView> { Some(*o) }
    fn object_as_promise(o: &Self::JsObject) -> Option<Self::Promise> { Some(*o) }
    fn object_as_function(o: &Self::JsObject) -> Option<Self::Function> { Some(*o) }
    fn object_as_constructor(o: &Self::JsObject) -> Option<Self::Constructor> { Some(*o) }
    fn object_as_map(o: &Self::JsObject) -> Option<Self::Map> { Some(*o) }
    fn object_as_set(o: &Self::JsObject) -> Option<Self::Set> { Some(*o) }
    fn object_as_weak_map(o: &Self::JsObject) -> Option<Self::WeakMap> { Some(*o) }
    fn object_as_weak_set(o: &Self::JsObject) -> Option<Self::WeakSet> { Some(*o) }
    fn object_as_generator(o: &Self::JsObject) -> Option<Self::Generator> { Some(*o) }
    fn object_as_async_generator(o: &Self::JsObject) -> Option<Self::AsyncGenerator> { Some(*o) }
}

impl JsTypesWithRealm for JscTypes {
    type Realm = JscRealm;
}

/// JSC engine wrapper.  Owns a `JSGlobalContextRef` and provides access to
/// ECMAScript abstract operations.
pub struct JscEngine {
    context: JscContext,
}

impl JscEngine {
    pub fn new() -> Self { Self { context: JscContext::new() } }
    pub fn context(&self) -> &JscContext { &self.context }

    #[allow(dead_code)]
    fn make_string(&self, s: &str) -> JscValue {
        let js_str = JscString::from_rust(s);
        JscValue { raw: unsafe { JSValueMakeString(self.context.as_context_ref(), js_str.raw) } }
    }

    #[allow(dead_code)]
    fn make_number(&self, n: f64) -> JscValue {
        JscValue { raw: unsafe { JSValueMakeNumber(self.context.as_context_ref(), n) } }
    }

    #[allow(dead_code)]
    fn make_bool(&self, b: bool) -> JscValue {
        JscValue { raw: unsafe { JSValueMakeBoolean(self.context.as_context_ref(), b) } }
    }

    fn property_key_to_jsstring(&self, key: &JscPropertyKey) -> Option<JscString> {
        match key {
            JscPropertyKey::String(s) => Some(s.clone()),
            JscPropertyKey::Symbol(_) => None,
        }
    }
}

impl Default for JscEngine { fn default() -> Self { Self::new() } }

impl JsEngine<JscTypes> for JscEngine {
    // ── §7.1 Type Conversion ──────────────────────────────────────────────

    fn to_primitive(&mut self, input: JscValue, _preferred_type: Option<PreferredType>) -> Completion<JscValue, JscTypes> {
        Ok(input)
    }

    fn to_boolean(&self, value: &JscValue) -> bool {
        unsafe { JSValueToBoolean(self.context.as_context_ref(), value.raw) }
    }

    fn to_number(&mut self, value: JscValue) -> Completion<f64, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSValueToNumber(self.context.as_context_ref(), value.raw, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(result)
    }

    fn to_numeric(&mut self, value: JscValue) -> Completion<Numeric<JscTypes>, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type {
            JSType::kJSTypeBigInt => Ok(Numeric::BigInt(JscBigInt { value })),
            _ => self.to_number(value).map(Numeric::Number),
        }
    }

    fn to_int32(&mut self, value: JscValue) -> Completion<i32, JscTypes> { self.to_number(value).map(|n| n as i32) }
    fn to_uint32(&mut self, value: JscValue) -> Completion<u32, JscTypes> { self.to_number(value).map(|n| n as u32) }
    fn to_int16(&mut self, value: JscValue) -> Completion<i16, JscTypes> { self.to_number(value).map(|n| n as i16) }
    fn to_uint16(&mut self, value: JscValue) -> Completion<u16, JscTypes> { self.to_number(value).map(|n| n as u16) }
    fn to_int8(&mut self, value: JscValue) -> Completion<i8, JscTypes> { self.to_number(value).map(|n| n as i8) }
    fn to_uint8(&mut self, value: JscValue) -> Completion<u8, JscTypes> { self.to_number(value).map(|n| n as u8) }

    fn to_uint8_clamp(&mut self, value: JscValue) -> Completion<u8, JscTypes> {
        self.to_number(value).map(|n| {
            if n <= 0.0 { 0 } else if n >= 255.0 { 255 } else { (n + 0.5).floor() as u8 }
        })
    }

    fn to_bigint(&mut self, _value: JscValue) -> Completion<JscBigInt, JscTypes> { todo!("JSC BigInt conversion") }
    fn string_to_bigint(&mut self, _string: JscString) -> Option<JscBigInt> { todo!("JSC string_to_bigint") }

    fn to_js_string(&mut self, value: JscValue) -> Completion<JscString, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe { JSValueToStringCopy(self.context.as_context_ref(), value.raw, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(unsafe { JscString::from_raw(raw) })
    }

    fn to_object(&mut self, _value: JscValue) -> Completion<JscObject, JscTypes> { todo!("JSC to_object") }
    fn to_property_key(&mut self, _value: JscValue) -> Completion<JscPropertyKey, JscTypes> { todo!("JSC to_property_key") }

    fn to_length(&mut self, value: JscValue) -> Completion<u64, JscTypes> {
        self.to_number(value).map(|n| if n <= 0.0 { 0 } else { n.min(f64::from(u32::MAX)) as u64 })
    }

    fn canonical_numeric_index_string(&self, argument: &JscString) -> Option<f64> {
        let s = argument.to_rust();
        if let Ok(n) = s.parse::<f64>() {
            if n.to_string() == s || (n.is_infinite() && (s.starts_with('-') || s.starts_with('+'))) {
                return Some(n);
            }
        }
        None
    }

    fn to_index(&mut self, value: JscValue) -> Completion<u64, JscTypes> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() || n < 0.0 { return Ok(0); }
        Ok(n.trunc() as u64)
    }

    // ── §7.2 Testing and Comparison ───────────────────────────────────────

    fn require_object_coercible(&mut self, value: JscValue) -> Completion<JscValue, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type { JSType::kJSTypeUndefined | JSType::kJSTypeNull => Err(value), _ => Ok(value) }
    }

    fn is_array(&mut self, _value: &JscValue) -> Completion<bool, JscTypes> { Ok(false) }

    fn is_callable(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) } != JSType::kJSTypeObject { return false; }
        unsafe { JSObjectIsFunction(self.context.as_context_ref(), value.raw as *mut JSObjectRef) }
    }

    fn is_constructor(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) } != JSType::kJSTypeObject { return false; }
        unsafe { JSObjectIsConstructor(self.context.as_context_ref(), value.raw as *mut JSObjectRef) }
    }

    fn is_extensible(&mut self, _object: &JscObject) -> Completion<bool, JscTypes> { Ok(true) }
    fn is_integral_number(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) } != JSType::kJSTypeNumber { return false; }
        let n = unsafe { JSValueToNumber(self.context.as_context_ref(), value.raw, std::ptr::null_mut()) };
        n.is_finite() && n.trunc() == n
    }

    fn is_property_key(&self, value: &JscValue) -> bool {
        let t = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        matches!(t, JSType::kJSTypeString | JSType::kJSTypeSymbol)
    }

    fn same_value(&self, x: &JscValue, y: &JscValue) -> bool {
        unsafe { JSValueIsStrictEqual(self.context.as_context_ref(), x.raw, y.raw) }
    }
    fn same_value_zero(&self, x: &JscValue, y: &JscValue) -> bool {
        unsafe { JSValueIsStrictEqual(self.context.as_context_ref(), x.raw, y.raw) }
    }

    fn is_loosely_equal(&mut self, x: JscValue, y: JscValue) -> Completion<bool, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSValueIsEqual(self.context.as_context_ref(), x.raw, y.raw, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(result)
    }

    fn is_strictly_equal(&self, x: &JscValue, y: &JscValue) -> bool {
        unsafe { JSValueIsStrictEqual(self.context.as_context_ref(), x.raw, y.raw) }
    }

    // ── §7.3 Operations on Objects ────────────────────────────────────────

    fn get(&mut self, object: JscObject, property_key: JscPropertyKey) -> Completion<JscValue, JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else { return Err(JscUndefined::get(&self.context)); };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSObjectCopyProperty(self.context.as_context_ref(), object.raw, prop_str.raw, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(JscValue { raw: result })
    }

    fn get_v(&mut self, value: JscValue, property_key: JscPropertyKey) -> Completion<JscValue, JscTypes> {
        let t = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        if t == JSType::kJSTypeObject {
            self.get(JscObject { raw: value.raw as *mut JSObjectRef, _phantom: PhantomData }, property_key)
        } else { Err(JscUndefined::get(&self.context)) }
    }

    fn set(&mut self, object: JscObject, property_key: JscPropertyKey, value: JscValue, _throw: bool) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else { return Ok(()); };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe { JSObjectSetProperty(self.context.as_context_ref(), object.raw, prop_str.raw, value.raw, kJSPropertyAttributeNone, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(())
    }

    fn create_data_property(&mut self, object: JscObject, property_key: JscPropertyKey, value: JscValue) -> Completion<bool, JscTypes> {
        self.set(object, property_key, value, false)?; Ok(true)
    }

    fn define_property_or_throw(&mut self, object: JscObject, property_key: JscPropertyKey, _descriptor: PropertyDescriptor<JscTypes>) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else { return Ok(()); };
        if let Some(value) = &_descriptor.value {
            let mut exception: *mut JSValueRef = std::ptr::null_mut();
            unsafe { JSObjectSetProperty(self.context.as_context_ref(), object.raw, prop_str.raw, value.raw, kJSPropertyAttributeNone, &mut exception) };
            if !exception.is_null() { return Err(JscValue { raw: exception }); }
        }
        Ok(())
    }

    fn delete_property_or_throw(&mut self, object: JscObject, property_key: JscPropertyKey) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else { return Ok(()); };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe { JSObjectDeleteProperty(self.context.as_context_ref(), object.raw, prop_str.raw, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(())
    }

    fn get_method(&mut self, value: JscValue, property_key: JscPropertyKey) -> Completion<Option<JscFunction>, JscTypes> {
        let prop = self.get_v(value, property_key)?;
        if self.is_callable(&prop) {
            Ok(Some(JscObject { raw: prop.raw as *mut JSObjectRef, _phantom: PhantomData }))
        } else { Ok(None) }
    }

    fn has_property(&mut self, object: JscObject, property_key: JscPropertyKey) -> Completion<bool, JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else { return Ok(false); };
        Ok(unsafe { JSObjectHasProperty(self.context.as_context_ref(), object.raw, prop_str.raw) })
    }

    fn has_own_property(&mut self, _object: JscObject, _property_key: JscPropertyKey) -> Completion<bool, JscTypes> { Ok(false) }

    fn call(&mut self, function: JscFunction, this: JscValue, args: &[JscValue]) -> Completion<JscValue, JscTypes> {
        let this_obj = if unsafe { JSValueGetType(self.context.as_context_ref(), this.raw) } == JSType::kJSTypeObject {
            this.raw as *mut JSObjectRef
        } else { std::ptr::null_mut() };
        let args_raw: Vec<*mut JSValueRef> = args.iter().map(|v| v.raw).collect();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSObjectCallAsFunction(self.context.as_context_ref(), function.raw, this_obj, args_raw.len(), args_raw.as_ptr(), &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(JscValue { raw: result })
    }

    fn construct(&mut self, function: JscConstructor, args: &[JscValue], _new_target: Option<JscConstructor>) -> Completion<JscObject, JscTypes> {
        let args_raw: Vec<*mut JSValueRef> = args.iter().map(|v| v.raw).collect();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSObjectCallAsConstructor(self.context.as_context_ref(), function.raw, args_raw.len(), args_raw.as_ptr(), &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(JscObject { raw: result, _phantom: PhantomData })
    }

    fn set_integrity_level(&mut self, _object: JscObject, _level: IntegrityLevel) -> Completion<bool, JscTypes> { Ok(false) }
    fn test_integrity_level(&mut self, _object: JscObject, _level: IntegrityLevel) -> Completion<bool, JscTypes> { Ok(false) }
    fn species_constructor(&mut self, _object: JscObject, default_constructor: JscConstructor) -> Completion<JscConstructor, JscTypes> { Ok(default_constructor) }
    fn get_iterator(&mut self, _object: JscValue, _kind: IteratorKind, _method: Option<JscFunction>) -> Completion<IteratorRecord<JscTypes>, JscTypes> { todo!("JSC get_iterator") }
    fn iterator_step_value(&mut self, _iterator: &mut IteratorRecord<JscTypes>) -> Completion<Option<JscValue>, JscTypes> { todo!("JSC iterator_step_value") }
    fn iterator_close(&mut self, _iterator: IteratorRecord<JscTypes>, completion: Completion<JscValue, JscTypes>) -> Completion<JscValue, JscTypes> { completion }
    fn async_iterator_close(&mut self, _iterator: IteratorRecord<JscTypes>, completion: Completion<JscValue, JscTypes>) -> Completion<JscValue, JscTypes> { completion }

    // ── §9.3 Realm ────────────────────────────────────────────────────────

    fn create_realm(&mut self) -> JscRealm where JscTypes: JsTypesWithRealm {
        JscRealm { raw: unsafe { JSGlobalContextCreate(std::ptr::null_mut()) } }
    }

    fn set_realm_global_object(&mut self, _realm: &JscRealm, _global: JscObject, _this_value: Option<JscObject>) where JscTypes: JsTypesWithRealm { todo!("JSC set_realm_global_object") }
    fn set_default_global_bindings(&mut self, _realm: &JscRealm) -> Completion<(), JscTypes> where JscTypes: JsTypesWithRealm { Ok(()) }
    fn current_realm(&self) -> JscRealm where JscTypes: JsTypesWithRealm { JscRealm { raw: self.context.raw } }

    fn realm_intrinsics(&self, _realm: &JscRealm) -> RealmIntrinsics<JscTypes> where JscTypes: JsTypesWithRealm {
        todo!("JSC realm_intrinsics")
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────
    fn enqueue_job(&mut self, _job: Box<dyn FnOnce() + Send>) {}
    fn run_jobs(&mut self) {}

    // ── §16 Script ────────────────────────────────────────────────────────

    fn evaluate_script(&mut self, source: &str, _realm: &JscRealm) -> Completion<JscValue, JscTypes> where JscTypes: JsTypesWithRealm {
        let script = JscString::from_rust(source);
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe { JSEvaluateScript(self.context.as_context_ref(), script.raw, std::ptr::null_mut(), std::ptr::null_mut(), 1, &mut exception) };
        if !exception.is_null() { return Err(JscValue { raw: exception }); }
        Ok(JscValue { raw: result })
    }

    fn evaluate_module(&mut self, _source: &str, _realm: &JscRealm) -> Completion<JscObject, JscTypes> where JscTypes: JsTypesWithRealm {
        todo!("JSC module evaluation not available via C API")
    }

    // ── §25 ArrayBuffer ───────────────────────────────────────────────────
    fn allocate_array_buffer(&mut self, _constructor: JscConstructor, _byte_length: u64, _max_byte_length: Option<u64>) -> Completion<JscArrayBuffer, JscTypes> { todo!("JSC allocate_array_buffer") }
    fn is_detached_buffer(&self, _array_buffer: &JscArrayBuffer) -> bool { false }
    fn detach_array_buffer(&mut self, _array_buffer: JscArrayBuffer, _key: Option<JscValue>) -> Completion<(), JscTypes> { Ok(()) }
    fn clone_array_buffer(&mut self, _src: JscArrayBuffer, _src_byte_offset: u64, _src_length: u64, _clone_constructor: JscConstructor) -> Completion<JscArrayBuffer, JscTypes> { todo!("JSC clone_array_buffer") }
    fn is_fixed_length_array_buffer(&self, _array_buffer: &JscArrayBuffer) -> bool { true }
    fn get_value_from_buffer(&self, _array_buffer: &JscArrayBuffer, _byte_index: u64, _element_type: TypedArrayElementType, _is_typed_array: bool, _order: SharedMemoryOrder) -> JscValue { JscUndefined::get(&self.context) }
    fn set_value_in_buffer(&mut self, _array_buffer: &JscArrayBuffer, _byte_index: u64, _element_type: TypedArrayElementType, _value: JscValue, _is_typed_array: bool, _order: SharedMemoryOrder) -> Completion<(), JscTypes> { Ok(()) }
    fn allocate_shared_array_buffer(&mut self, _constructor: JscConstructor, _byte_length: u64) -> Completion<JscSharedArrayBuffer, JscTypes> { todo!("JSC SharedArrayBuffer") }

    // ── §27 Promise ───────────────────────────────────────────────────────
    fn promise_resolve(&mut self, _constructor: JscConstructor, _x: JscValue) -> Completion<JscPromise, JscTypes> { todo!("JSC promise") }
    fn new_promise_capability(&mut self, _constructor: JscConstructor) -> Completion<PromiseCapability<JscTypes>, JscTypes> { todo!("JSC promise") }
    fn perform_promise_then(&mut self, _promise: JscPromise, _on_fulfilled: Option<JscFunction>, _on_rejected: Option<JscFunction>, _result_capability: Option<PromiseCapability<JscTypes>>) -> Completion<JscValue, JscTypes> { todo!("JSC promise") }

    // ── §27.5 Generator ───────────────────────────────────────────────────
    fn generator_start(&mut self, _generator: JscGenerator, _closure: JscFunction) -> Completion<(), JscTypes> { todo!("JSC generator") }

    // ── Host Hooks ────────────────────────────────────────────────────────
    fn set_host_hooks(&mut self, _hooks: HostHooks<JscTypes>) where JscTypes: JsTypesWithRealm {}
}
