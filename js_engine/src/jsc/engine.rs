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

use super::types::*;
use crate::jsc_sys::*;
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
    fn object_from_array_buffer(ab: Self::ArrayBuffer) -> Self::JsObject {
        ab
    }
    fn object_from_shared_array_buffer(sab: Self::SharedArrayBuffer) -> Self::JsObject {
        sab
    }
    fn object_from_typed_array(ta: Self::TypedArray) -> Self::JsObject {
        ta
    }
    fn object_from_data_view(dv: Self::DataView) -> Self::JsObject {
        dv
    }
    fn object_from_promise(p: Self::Promise) -> Self::JsObject {
        p
    }
    fn object_from_map(m: Self::Map) -> Self::JsObject {
        m
    }
    fn object_from_set(s: Self::Set) -> Self::JsObject {
        s
    }
    fn object_from_function(f: Self::Function) -> Self::JsObject {
        f
    }
    fn object_from_constructor(c: Self::Constructor) -> Self::JsObject {
        c
    }

    fn value_from_object(o: Self::JsObject) -> Self::JsValue {
        o.as_value()
    }
    fn value_from_symbol(sym: Self::JsSymbol) -> Self::JsValue {
        sym.as_value().clone()
    }
    fn value_from_bigint(n: Self::JsBigInt) -> Self::JsValue {
        n.as_value().clone()
    }

    // ── Downcasts ────────────────────────────────────────────────────
    fn value_as_object(_v: &Self::JsValue) -> Option<Self::JsObject> {
        None /* needs context */
    }
    fn value_as_string(_v: &Self::JsValue) -> Option<Self::JsString> {
        None
    }
    fn value_as_symbol(_v: &Self::JsValue) -> Option<Self::JsSymbol> {
        None
    }
    fn value_as_number(_v: &Self::JsValue) -> Option<f64> {
        None
    }
    fn value_as_bool(_v: &Self::JsValue) -> Option<bool> {
        None
    }
    fn value_is_undefined(_v: &Self::JsValue) -> bool {
        false
    }
    fn value_is_null(_v: &Self::JsValue) -> bool {
        false
    }

    fn object_as_array_buffer(o: &Self::JsObject) -> Option<Self::ArrayBuffer> {
        Some(*o)
    }
    fn object_as_shared_array_buffer(o: &Self::JsObject) -> Option<Self::SharedArrayBuffer> {
        Some(*o)
    }
    fn object_as_typed_array(o: &Self::JsObject) -> Option<Self::TypedArray> {
        Some(*o)
    }
    fn object_as_data_view(o: &Self::JsObject) -> Option<Self::DataView> {
        Some(*o)
    }
    fn object_as_promise(o: &Self::JsObject) -> Option<Self::Promise> {
        Some(*o)
    }
    fn object_as_function(o: &Self::JsObject) -> Option<Self::Function> {
        Some(*o)
    }
    fn object_as_constructor(o: &Self::JsObject) -> Option<Self::Constructor> {
        Some(*o)
    }
    fn object_as_map(o: &Self::JsObject) -> Option<Self::Map> {
        Some(*o)
    }
    fn object_as_set(o: &Self::JsObject) -> Option<Self::Set> {
        Some(*o)
    }
    fn object_as_weak_map(o: &Self::JsObject) -> Option<Self::WeakMap> {
        Some(*o)
    }
    fn object_as_weak_set(o: &Self::JsObject) -> Option<Self::WeakSet> {
        Some(*o)
    }
    fn object_as_generator(o: &Self::JsObject) -> Option<Self::Generator> {
        Some(*o)
    }
    fn object_as_async_generator(o: &Self::JsObject) -> Option<Self::AsyncGenerator> {
        Some(*o)
    }
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
    pub fn new() -> Self {
        Self {
            context: JscContext::new(),
        }
    }
    pub fn context(&self) -> &JscContext {
        &self.context
    }

    #[allow(dead_code)]
    fn make_string(&self, s: &str) -> JscValue {
        let js_str = JscString::from_rust(s);
        JscValue {
            raw: unsafe { JSValueMakeString(self.context.as_context_ref(), js_str.raw) },
        }
    }

    #[allow(dead_code)]
    fn make_number(&self, n: f64) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeNumber(self.context.as_context_ref(), n) },
        }
    }

    #[allow(dead_code)]
    fn make_bool(&self, b: bool) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeBoolean(self.context.as_context_ref(), b) },
        }
    }

    fn property_key_to_jsstring(&self, key: &JscPropertyKey) -> Option<JscString> {
        match key {
            JscPropertyKey::String(s) => Some(s.clone()),
            JscPropertyKey::Symbol(_) => None,
        }
    }
}

impl Default for JscEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl JsEngine<JscTypes> for JscEngine {
    // ── §7.1 Type Conversion ──────────────────────────────────────────────

    fn to_primitive(
        &mut self,
        input: JscValue,
        _preferred_type: Option<PreferredType>,
    ) -> Completion<JscValue, JscTypes> {
        Ok(input)
    }

    fn to_boolean(&self, value: &JscValue) -> bool {
        unsafe { JSValueToBoolean(self.context.as_context_ref(), value.raw) }
    }

    fn to_number(&mut self, value: JscValue) -> Completion<f64, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result =
            unsafe { JSValueToNumber(self.context.as_context_ref(), value.raw, &mut exception) };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(result)
    }

    fn to_numeric(&mut self, value: JscValue) -> Completion<Numeric<JscTypes>, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type {
            JSType::kJSTypeBigInt => Ok(Numeric::BigInt(JscBigInt { value })),
            _ => self.to_number(value).map(Numeric::Number),
        }
    }

    fn to_int32(&mut self, value: JscValue) -> Completion<i32, JscTypes> {
        self.to_number(value).map(|n| n as i32)
    }
    fn to_uint32(&mut self, value: JscValue) -> Completion<u32, JscTypes> {
        self.to_number(value).map(|n| n as u32)
    }
    fn to_int16(&mut self, value: JscValue) -> Completion<i16, JscTypes> {
        self.to_number(value).map(|n| n as i16)
    }
    fn to_uint16(&mut self, value: JscValue) -> Completion<u16, JscTypes> {
        self.to_number(value).map(|n| n as u16)
    }
    fn to_int8(&mut self, value: JscValue) -> Completion<i8, JscTypes> {
        self.to_number(value).map(|n| n as i8)
    }
    fn to_uint8(&mut self, value: JscValue) -> Completion<u8, JscTypes> {
        self.to_number(value).map(|n| n as u8)
    }

    fn to_uint8_clamp(&mut self, value: JscValue) -> Completion<u8, JscTypes> {
        self.to_number(value).map(|n| {
            if n <= 0.0 {
                0
            } else if n >= 255.0 {
                255
            } else {
                (n + 0.5).floor() as u8
            }
        })
    }

    fn to_bigint(&mut self, _value: JscValue) -> Completion<JscBigInt, JscTypes> {
        todo!("JSC BigInt conversion")
    }
    fn string_to_bigint(&mut self, _string: JscString) -> Option<JscBigInt> {
        todo!("JSC string_to_bigint")
    }

    fn to_js_string(&mut self, value: JscValue) -> Completion<JscString, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSValueToStringCopy(self.context.as_context_ref(), value.raw, &mut exception)
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(unsafe { JscString::from_raw(raw) })
    }

    fn to_object(&mut self, value: JscValue) -> Completion<JscObject, JscTypes> {
        // §7.1.13 ToObject
        // Step 1: If value is Object...
        // Step 2: If value is Boolean, Number, String, Symbol, or BigInt...
        // Step 3: If value is Undefined or Null, throw a TypeError.
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type {
            JSType::kJSTypeObject => Ok(JscObject {
                raw: value.raw as *mut JSObjectRef,
                _phantom: std::marker::PhantomData,
            }),
            JSType::kJSTypeUndefined | JSType::kJSTypeNull => {
                // Create a TypeError exception value.
                let message = JscString::from_rust("Cannot convert undefined or null to object");
                let exc_value = JscValue {
                    raw: unsafe { JSValueMakeString(self.context.as_context_ref(), message.raw) },
                };
                Err(exc_value)
            }
            _ => {
                // Other types (Boolean, Number, String, Symbol, BigInt):
                // JSC automatically wraps primitives.  The value is already
                // heap-allocated and can be used as an object reference.
                Ok(JscObject {
                    raw: value.raw as *mut JSObjectRef,
                    _phantom: std::marker::PhantomData,
                })
            }
        }
    }
    fn to_property_key(&mut self, value: JscValue) -> Completion<JscPropertyKey, JscTypes> {
        // §7.1.14 ToPropertyKey
        // Step 1: Let key be ? ToPrimitive(argument, hint String).
        // Note: `to_primitive` is currently a pass-through; this works for
        // values that are already strings or symbols.
        //
        // Step 2: If key is a Symbol, return key.
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        if js_type == JSType::kJSTypeSymbol {
            // SAFETY: value type has been checked.
            return Ok(JscPropertyKey::Symbol(unsafe {
                JscSymbol::from_value(value)
            }));
        }
        // Step 3: Return ! ToString(key).
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSValueToStringCopy(self.context.as_context_ref(), value.raw, &mut exception)
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscPropertyKey::String(unsafe { JscString::from_raw(raw) }))
    }

    fn to_length(&mut self, value: JscValue) -> Completion<u64, JscTypes> {
        self.to_number(value).map(|n| {
            if n <= 0.0 {
                0
            } else {
                n.min(f64::from(u32::MAX)) as u64
            }
        })
    }

    fn canonical_numeric_index_string(&self, argument: &JscString) -> Option<f64> {
        let s = argument.to_rust();
        if let Ok(n) = s.parse::<f64>() {
            if n.to_string() == s || (n.is_infinite() && (s.starts_with('-') || s.starts_with('+')))
            {
                return Some(n);
            }
        }
        None
    }

    fn to_index(&mut self, value: JscValue) -> Completion<u64, JscTypes> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() || n < 0.0 {
            return Ok(0);
        }
        Ok(n.trunc() as u64)
    }

    // ── §7.2 Testing and Comparison ───────────────────────────────────────

    fn require_object_coercible(&mut self, value: JscValue) -> Completion<JscValue, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type {
            JSType::kJSTypeUndefined | JSType::kJSTypeNull => Err(value),
            _ => Ok(value),
        }
    }

    fn is_array(&mut self, _value: &JscValue) -> Completion<bool, JscTypes> {
        Ok(false)
    }

    fn is_callable(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            != JSType::kJSTypeObject
        {
            return false;
        }
        unsafe { JSObjectIsFunction(self.context.as_context_ref(), value.raw as *mut JSObjectRef) }
    }

    fn is_constructor(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            != JSType::kJSTypeObject
        {
            return false;
        }
        unsafe {
            JSObjectIsConstructor(self.context.as_context_ref(), value.raw as *mut JSObjectRef)
        }
    }

    fn is_extensible(&mut self, _object: &JscObject) -> Completion<bool, JscTypes> {
        Ok(true)
    }
    fn is_integral_number(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            != JSType::kJSTypeNumber
        {
            return false;
        }
        let n = unsafe {
            JSValueToNumber(
                self.context.as_context_ref(),
                value.raw,
                std::ptr::null_mut(),
            )
        };
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
        let result =
            unsafe { JSValueIsEqual(self.context.as_context_ref(), x.raw, y.raw, &mut exception) };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(result)
    }

    fn is_strictly_equal(&self, x: &JscValue, y: &JscValue) -> bool {
        unsafe { JSValueIsStrictEqual(self.context.as_context_ref(), x.raw, y.raw) }
    }

    // ── §7.3 Operations on Objects ────────────────────────────────────────

    fn get(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
    ) -> Completion<JscValue, JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Err(JscUndefined::get(&self.context));
        };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                object.raw,
                prop_str.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscValue { raw: result })
    }

    fn get_v(
        &mut self,
        value: JscValue,
        property_key: JscPropertyKey,
    ) -> Completion<JscValue, JscTypes> {
        let t = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        if t == JSType::kJSTypeObject {
            self.get(
                JscObject {
                    raw: value.raw as *mut JSObjectRef,
                    _phantom: PhantomData,
                },
                property_key,
            )
        } else {
            Err(JscUndefined::get(&self.context))
        }
    }

    fn set(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
        value: JscValue,
        _throw: bool,
    ) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(());
        };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                object.raw,
                prop_str.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(())
    }

    fn create_data_property(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
        value: JscValue,
    ) -> Completion<bool, JscTypes> {
        self.set(object, property_key, value, false)?;
        Ok(true)
    }

    fn define_property_or_throw(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
        _descriptor: PropertyDescriptor<JscTypes>,
    ) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(());
        };
        if let Some(value) = &_descriptor.value {
            let mut exception: *mut JSValueRef = std::ptr::null_mut();
            unsafe {
                JSObjectSetProperty(
                    self.context.as_context_ref(),
                    object.raw,
                    prop_str.raw,
                    value.raw,
                    kJSPropertyAttributeNone,
                    &mut exception,
                )
            };
            if !exception.is_null() {
                return Err(JscValue { raw: exception });
            }
        }
        Ok(())
    }

    fn delete_property_or_throw(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
    ) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(());
        };
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                object.raw,
                prop_str.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(())
    }

    fn get_method(
        &mut self,
        value: JscValue,
        property_key: JscPropertyKey,
    ) -> Completion<Option<JscFunction>, JscTypes> {
        let prop = self.get_v(value, property_key)?;
        if self.is_callable(&prop) {
            Ok(Some(JscObject {
                raw: prop.raw as *mut JSObjectRef,
                _phantom: PhantomData,
            }))
        } else {
            Ok(None)
        }
    }

    fn has_property(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
    ) -> Completion<bool, JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(false);
        };
        Ok(unsafe { JSObjectHasProperty(self.context.as_context_ref(), object.raw, prop_str.raw) })
    }

    fn has_own_property(
        &mut self,
        _object: JscObject,
        _property_key: JscPropertyKey,
    ) -> Completion<bool, JscTypes> {
        Ok(false)
    }

    fn call(
        &mut self,
        function: JscFunction,
        this: JscValue,
        args: &[JscValue],
    ) -> Completion<JscValue, JscTypes> {
        let this_obj = if unsafe { JSValueGetType(self.context.as_context_ref(), this.raw) }
            == JSType::kJSTypeObject
        {
            this.raw as *mut JSObjectRef
        } else {
            std::ptr::null_mut()
        };
        let args_raw: Vec<*mut JSValueRef> = args.iter().map(|v| v.raw).collect();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSObjectCallAsFunction(
                self.context.as_context_ref(),
                function.raw,
                this_obj,
                args_raw.len(),
                args_raw.as_ptr(),
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscValue { raw: result })
    }

    fn construct(
        &mut self,
        function: JscConstructor,
        args: &[JscValue],
        _new_target: Option<JscConstructor>,
    ) -> Completion<JscObject, JscTypes> {
        let args_raw: Vec<*mut JSValueRef> = args.iter().map(|v| v.raw).collect();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSObjectCallAsConstructor(
                self.context.as_context_ref(),
                function.raw,
                args_raw.len(),
                args_raw.as_ptr(),
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscObject {
            raw: result,
            _phantom: PhantomData,
        })
    }

    fn set_integrity_level(
        &mut self,
        _object: JscObject,
        _level: IntegrityLevel,
    ) -> Completion<bool, JscTypes> {
        Ok(false)
    }
    fn test_integrity_level(
        &mut self,
        _object: JscObject,
        _level: IntegrityLevel,
    ) -> Completion<bool, JscTypes> {
        Ok(false)
    }
    fn species_constructor(
        &mut self,
        _object: JscObject,
        default_constructor: JscConstructor,
    ) -> Completion<JscConstructor, JscTypes> {
        Ok(default_constructor)
    }
    fn get_iterator(
        &mut self,
        object: JscValue,
        _kind: IteratorKind,
        _method: Option<JscFunction>,
    ) -> Completion<IteratorRecord<JscTypes>, JscTypes> {
        // §7.4.4 GetIterator
        // Get the `@@iterator` symbol from the global Symbol object.
        let global = self.context.global_object();
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let symbol_str = JscString::from_rust("Symbol");
        let symbol_val = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                global.raw,
                symbol_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        let symbol_obj = JscObject {
            raw: symbol_val as *mut JSObjectRef,
            _phantom: std::marker::PhantomData,
        };
        let iter_str = JscString::from_rust("iterator");
        let iterator_sym = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                symbol_obj.raw,
                iter_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        // Step 2: GetMethod(obj, @@iterator) via our trait.
        let method = self.get_method(
            object,
            JscPropertyKey::Symbol(unsafe {
                JscSymbol::from_value(JscValue { raw: iterator_sym })
            }),
        )?;
        let method = method.ok_or_else(|| JscUndefined::get(&self.context))?;
        // Step 3: Call(method, obj) → iterator.
        let iter_val = self.call(method, JscUndefined::get(&self.context), &[])?;
        let iter_obj = iter_val.raw as *mut JSObjectRef;
        // Get `next` method from the iterator.
        let next_str = JscString::from_rust("next");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let next_val = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                iter_obj,
                next_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        let next_method = JscObject {
            raw: next_val as *mut JSObjectRef,
            _phantom: std::marker::PhantomData,
        };
        Ok(IteratorRecord {
            iterator: JscObject {
                raw: iter_obj,
                _phantom: std::marker::PhantomData,
            },
            next_method,
            done: false,
        })
    }

    fn iterator_step_value(
        &mut self,
        iterator: &mut IteratorRecord<JscTypes>,
    ) -> Completion<Option<JscValue>, JscTypes> {
        // §7.4.10 IteratorStepValue
        // Call(next, iterator)
        let iter_val = JscValue {
            raw: iterator.iterator.raw as *mut JSValueRef,
        };
        let result = self.call(iterator.next_method, iter_val, &[])?;
        let result_obj = result.raw as *mut JSObjectRef;
        // Get `done` property.
        let done_str = JscString::from_rust("done");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let done_val = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                result_obj,
                done_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            iterator.done = true;
            return Err(JscValue { raw: exc });
        }
        let done = unsafe { JSValueToBoolean(self.context.as_context_ref(), done_val) };
        if done {
            iterator.done = true;
            return Ok(None);
        }
        // Get `value` property.
        let value_str = JscString::from_rust("value");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let value = unsafe {
            JSObjectCopyProperty(
                self.context.as_context_ref(),
                result_obj,
                value_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            iterator.done = true;
            return Err(JscValue { raw: exc });
        }
        Ok(Some(JscValue { raw: value }))
    }

    fn iterator_close(
        &mut self,
        iterator: IteratorRecord<JscTypes>,
        completion: Completion<JscValue, JscTypes>,
    ) -> Completion<JscValue, JscTypes> {
        // §7.4.11 IteratorClose
        // Get `return` method from the iterator.
        let return_str = JscString::from_rust("return");
        let return_key = JscPropertyKey::String(return_str);
        let inner_result = self.get_method(
            JscValue {
                raw: iterator.iterator.raw as *mut JSValueRef,
            },
            return_key,
        );
        match inner_result {
            Ok(Some(return_fn)) => {
                let iter_val = JscValue {
                    raw: iterator.iterator.raw as *mut JSValueRef,
                };
                match self.call(return_fn, iter_val, &[]) {
                    Ok(_) => completion,
                    Err(e) => {
                        if completion.is_err() {
                            return completion;
                        }
                        Err(e)
                    }
                }
            }
            Ok(None) => completion,
            Err(e) => {
                if completion.is_err() {
                    return completion;
                }
                Err(e)
            }
        }
    }

    fn async_iterator_close(
        &mut self,
        iterator: IteratorRecord<JscTypes>,
        completion: Completion<JscValue, JscTypes>,
    ) -> Completion<JscValue, JscTypes> {
        // §7.4.15 AsyncIteratorClose
        // Same as IteratorClose but result from Call(return)
        // would need Await.  Synchronous fallback for now.
        self.iterator_close(iterator, completion)
    }

    // ── §9.3 Realm ────────────────────────────────────────────────────────

    fn create_realm(&mut self) -> JscRealm
    where
        JscTypes: JsTypesWithRealm,
    {
        JscRealm {
            raw: unsafe { JSGlobalContextCreate(std::ptr::null_mut()) },
        }
    }

    fn set_realm_global_object(
        &mut self,
        _realm: &JscRealm,
        _global: JscObject,
        _this_value: Option<JscObject>,
    ) where
        JscTypes: JsTypesWithRealm,
    {
        todo!("JSC set_realm_global_object")
    }
    fn set_default_global_bindings(&mut self, _realm: &JscRealm) -> Completion<(), JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        Ok(())
    }
    fn current_realm(&self) -> JscRealm
    where
        JscTypes: JsTypesWithRealm,
    {
        JscRealm {
            raw: self.context.raw,
        }
    }

    fn realm_intrinsics(&self, _realm: &JscRealm) -> RealmIntrinsics<JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        todo!("JSC realm_intrinsics")
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────
    fn enqueue_job(&mut self, _job: Box<dyn FnOnce() + Send>) {}
    fn run_jobs(&mut self) {}

    // ── §16 Script ────────────────────────────────────────────────────────

    fn evaluate_script(&mut self, source: &str, _realm: &JscRealm) -> Completion<JscValue, JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        let script = JscString::from_rust(source);
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSEvaluateScript(
                self.context.as_context_ref(),
                script.raw,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscValue { raw: result })
    }

    fn evaluate_module(
        &mut self,
        _source: &str,
        _realm: &JscRealm,
    ) -> Completion<JscObject, JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        todo!("JSC module evaluation not available via C API")
    }

    // ── §25 ArrayBuffer ───────────────────────────────────────────────────
    fn allocate_array_buffer(
        &mut self,
        _constructor: JscConstructor,
        byte_length: u64,
        _max_byte_length: Option<u64>,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        // §25.1.3.1 AllocateArrayBuffer
        // Note: Uses JSC's C API with no deallocator (buffer memory is
        // managed by Rust and intentionally leaked).  A proper
        // implementation would register a C deallocator callback.
        let len = byte_length as usize;
        let mut buf = vec![0u8; len].into_boxed_slice();
        let ptr = buf.as_mut_ptr() as *mut std::ffi::c_void;
        // Forget the Box — JSC holds the pointer.  Memory is leaked
        // if no deallocator is provided (acceptable for now).
        std::mem::forget(buf);
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSObjectMakeArrayBufferWithBytesNoCopy(
                self.context.as_context_ref(),
                ptr,
                len,
                std::ptr::null_mut(), // no deallocator
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue { raw: exception });
        }
        Ok(JscObject {
            raw,
            _phantom: std::marker::PhantomData,
        })
    }
    fn is_detached_buffer(&self, _array_buffer: &JscArrayBuffer) -> bool {
        false
    }
    fn detach_array_buffer(
        &mut self,
        _array_buffer: JscArrayBuffer,
        _key: Option<JscValue>,
    ) -> Completion<(), JscTypes> {
        Ok(())
    }
    fn clone_array_buffer(
        &mut self,
        src: JscArrayBuffer,
        src_byte_offset: u64,
        src_length: u64,
        _clone_constructor: JscConstructor,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        // §25.1.3.6 CloneArrayBuffer
        // Uses typed array slice through script evaluation.
        let global = self.context.global_object();
        let src_key = JscString::from_rust("__formal_web_clone_src");
        // Store the source buffer temporarily on the global object.
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                src_key.raw,
                src.as_value_ref(),
                kJSPropertyAttributeNone,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        // Build and evaluate the cloning script.
        let script_str = format!(
            "new Uint8Array(__formal_web_clone_src).slice({}, {}).buffer",
            src_byte_offset, src_length
        );
        let script = JscString::from_rust(&script_str);
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSEvaluateScript(
                self.context.as_context_ref(),
                script.raw,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut exc,
            )
        };
        // Clean up the temporary property.
        let mut exc2: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                src_key.raw,
                &mut exc2,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        if result.is_null() {
            Err(JscUndefined::get(&self.context))
        } else {
            Ok(JscObject {
                raw: result as *mut JSObjectRef,
                _phantom: std::marker::PhantomData,
            })
        }
    }
    fn is_fixed_length_array_buffer(&self, _array_buffer: &JscArrayBuffer) -> bool {
        true
    }
    fn get_value_from_buffer(
        &self,
        _array_buffer: &JscArrayBuffer,
        _byte_index: u64,
        _element_type: TypedArrayElementType,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> JscValue {
        JscUndefined::get(&self.context)
    }
    fn set_value_in_buffer(
        &mut self,
        _array_buffer: &JscArrayBuffer,
        _byte_index: u64,
        _element_type: TypedArrayElementType,
        _value: JscValue,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> Completion<(), JscTypes> {
        Ok(())
    }
    fn allocate_shared_array_buffer(
        &mut self,
        _constructor: JscConstructor,
        byte_length: u64,
    ) -> Completion<JscSharedArrayBuffer, JscTypes> {
        // §25.2.1.1 AllocateSharedArrayBuffer
        // Uses script evaluation since there's no direct C API.
        let script_str = format!("new SharedArrayBuffer({})", byte_length);
        let script = JscString::from_rust(&script_str);
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSEvaluateScript(
                self.context.as_context_ref(),
                script.raw,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue { raw: exc });
        }
        Ok(JscObject {
            raw: result as *mut JSObjectRef,
            _phantom: std::marker::PhantomData,
        })
    }

    // ── §27 Promise ───────────────────────────────────────────────────────
    fn promise_resolve(
        &mut self,
        _constructor: JscConstructor,
        _x: JscValue,
    ) -> Completion<JscPromise, JscTypes> {
        todo!("JSC promise")
    }
    fn new_promise_capability(
        &mut self,
        _constructor: JscConstructor,
    ) -> Completion<PromiseCapability<JscTypes>, JscTypes> {
        todo!("JSC promise")
    }
    fn perform_promise_then(
        &mut self,
        _promise: JscPromise,
        _on_fulfilled: Option<JscFunction>,
        _on_rejected: Option<JscFunction>,
        _result_capability: Option<PromiseCapability<JscTypes>>,
    ) -> Completion<JscValue, JscTypes> {
        todo!("JSC promise")
    }

    // ── §27.5 Generator ───────────────────────────────────────────────────
    fn generator_start(
        &mut self,
        _generator: JscGenerator,
        _closure: JscFunction,
    ) -> Completion<(), JscTypes> {
        todo!("JSC generator")
    }

    // ── Value Construction ───────────────────────────────────────────────

    fn value_from_string(&mut self, s: JscString) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeString(self.context.as_context_ref(), s.raw) },
        }
    }

    fn value_from_bool(&mut self, b: bool) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeBoolean(self.context.as_context_ref(), b) },
        }
    }

    fn value_from_number(&mut self, n: f64) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeNumber(self.context.as_context_ref(), n) },
        }
    }

    fn value_undefined(&mut self) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeUndefined(self.context.as_context_ref()) },
        }
    }

    fn value_null(&mut self) -> JscValue {
        JscValue {
            raw: unsafe { JSValueMakeNull(self.context.as_context_ref()) },
        }
    }

    // ── Host Hooks ────────────────────────────────────────────────────────
    fn set_host_hooks(&mut self, _hooks: HostHooks<JscTypes>)
    where
        JscTypes: JsTypesWithRealm,
    {
    }
}
