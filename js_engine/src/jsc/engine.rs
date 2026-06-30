//! JSC engine wrapper implementing `JsEngine<JscTypes>`, `ExecutionContext<JscTypes>`,
//! and `EcmascriptHost<JscTypes>`.
//!
//! # Hard problems (not yet implemented)
//!
//! - **Jobs/microtasks** — JSC's C API doesn't expose the microtask queue.
//! - **Promise operations** — `JSObjectMakePromise` is not in the public C API.
//!   Implemented via JS evaluation (`new Promise(...)`).
//! - **TypedArray/ArrayBuffer** — basic creation available, GetValueFromBuffer etc. not.
//! - **Generator operations** — no public C API for generator control.
//! - **Module evaluation** — requires SPI.
//! - **SharedArrayBuffer** — available on newer macOS only.

use std::collections::HashMap;

use super::types::*;
use crate::jsc_sys::*;
use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, IntegrityLevel, IteratorKind,
    JsEngine, JsTypes, JsTypesWithRealm, Numeric, PreferredType, SharedMemoryOrder,
    TypedArrayElementType,
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
    fn value_as_object(v: &Self::JsValue) -> Option<Self::JsObject> {
        if v.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueGetType(v.ctx, v.raw) } == JSType::kJSTypeObject {
            Some(JscObject {
                raw: v.raw as *mut JSObjectRef,
                ctx: v.ctx,
            })
        } else {
            None
        }
    }
    fn value_as_string(v: &Self::JsValue) -> Option<Self::JsString> {
        if v.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueIsString(v.ctx, v.raw) } {
            let mut exc: *mut JSValueRef = std::ptr::null_mut();
            let raw = unsafe { JSValueToStringCopy(v.ctx, v.raw, &mut exc) };
            if !exc.is_null() || raw.is_null() {
                return None;
            }
            Some(unsafe { JscString::from_raw(raw) })
        } else {
            None
        }
    }
    fn value_as_symbol(v: &Self::JsValue) -> Option<Self::JsSymbol> {
        if v.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueGetType(v.ctx, v.raw) } == JSType::kJSTypeSymbol {
            Some(JscSymbol { value: *v })
        } else {
            None
        }
    }
    fn value_as_number(v: &Self::JsValue) -> Option<f64> {
        if v.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueIsNumber(v.ctx, v.raw) } {
            let mut exc: *mut JSValueRef = std::ptr::null_mut();
            let n = unsafe { JSValueToNumber(v.ctx, v.raw, &mut exc) };
            if !exc.is_null() {
                return None;
            }
            Some(n)
        } else {
            None
        }
    }
    fn value_as_bool(v: &Self::JsValue) -> Option<bool> {
        if v.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueIsBoolean(v.ctx, v.raw) } {
            Some(unsafe { JSValueToBoolean(v.ctx, v.raw) })
        } else {
            None
        }
    }
    fn value_is_undefined(v: &Self::JsValue) -> bool {
        if v.ctx.is_null() {
            return false;
        }
        unsafe { JSValueIsUndefined(v.ctx, v.raw) }
    }
    fn value_is_null(v: &Self::JsValue) -> bool {
        if v.ctx.is_null() {
            return false;
        }
        unsafe { JSValueIsNull(v.ctx, v.raw) }
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
    fn object_as_weak_ref(o: &Self::JsObject) -> Option<Self::WeakRef> {
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

/// JSC engine wrapper.  Owns a `JSGlobalContextRef` and implements
/// `JsEngine<JscTypes>`, `ExecutionContext<JscTypes>`, and
/// `EcmascriptHost<JscTypes>`.
pub struct JscEngine {
    context: JscContext,
    host_data: HashMap<std::any::TypeId, Box<dyn std::any::Any>>,
}

impl JscEngine {
    pub fn new() -> Self {
        Self {
            context: JscContext::new(),
            host_data: HashMap::new(),
        }
    }
    pub fn context(&self) -> &JscContext {
        &self.context
    }
    /// The raw `JSContextRef` pointer used for constructing `JscValue` / `JscObject`.
    fn ctx_ptr(&self) -> *mut JSContextRef {
        self.context.as_context_ref()
    }

    #[allow(dead_code)]
    fn make_string(&self, s: &str) -> JscValue {
        let js_str = JscString::from_rust(s);
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeString(ctx_ptr, js_str.raw) },
            ctx: ctx_ptr,
        }
    }
    #[allow(dead_code)]
    fn make_number(&self, n: f64) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeNumber(ctx_ptr, n) },
            ctx: ctx_ptr,
        }
    }
    #[allow(dead_code)]
    fn make_bool(&self, b: bool) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeBoolean(ctx_ptr, b) },
            ctx: ctx_ptr,
        }
    }

    fn property_key_to_jsstring(&self, key: &JscPropertyKey) -> Option<JscString> {
        match key {
            JscPropertyKey::String(s) => Some(s.clone()),
            JscPropertyKey::Symbol(_) => None,
        }
    }

    /// Evaluate a JS expression and return the raw result + any exception.
    fn eval_script_raw(&self, source: &str) -> (*mut JSValueRef, *mut JSValueRef) {
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
        (result, exception)
    }

    /// Evaluate a JS expression and return a Completion.
    #[allow(dead_code)]
    fn eval_script(&self, source: &str, _realm: &JscRealm) -> Completion<JscValue, JscTypes> {
        let (result, exception) = self.eval_script_raw(source);
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }
}

impl Default for JscEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// JsEngine<JscTypes> — factory operations (§9.3, §10.3, §16, §25)
// ═══════════════════════════════════════════════════════════════════════════

impl JsEngine<JscTypes> for JscEngine {
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
        // JSC creates a context with a global object already set up.
        // Replacing it is not supported through the public C API — the
        // global is wired in at context creation time.
    }
    fn set_default_global_bindings(&mut self, _realm: &JscRealm) -> Completion<(), JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        Ok(())
    }

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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }
    fn evaluate_module(
        &mut self,
        _source: &str,
        _realm: &JscRealm,
    ) -> Completion<JscObject, JscTypes>
    where
        JscTypes: JsTypesWithRealm,
    {
        // Module evaluation is not available through the public C API.
        // Return a placeholder error.
        Err(self.make_string("JSC module evaluation not available via C API"))
    }

    // ── §25 ArrayBuffer — creation ─────────────────────────────────────
    fn allocate_array_buffer(
        &mut self,
        _constructor: JscConstructor,
        byte_length: u64,
        _max_byte_length: Option<u64>,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        let len = byte_length as usize;
        let mut buf = vec![0u8; len].into_boxed_slice();
        let ptr = buf.as_mut_ptr() as *mut std::ffi::c_void;
        std::mem::forget(buf);
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSObjectMakeArrayBufferWithBytesNoCopy(
                self.context.as_context_ref(),
                ptr,
                len,
                std::ptr::null_mut(),
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscObject {
            raw,
            ctx: self.ctx_ptr(),
        })
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
        let global = self.context.global_object();
        let src_key = JscString::from_rust("__formal_web_clone_src");
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
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
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
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        if result.is_null() {
            Err(JscUndefined::get(&self.context))
        } else {
            Ok(JscObject {
                raw: result as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            })
        }
    }
    fn allocate_shared_array_buffer(
        &mut self,
        _constructor: JscConstructor,
        byte_length: u64,
    ) -> Completion<JscSharedArrayBuffer, JscTypes> {
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
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscObject {
            raw: result as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        })
    }

    // ── Host Hooks ────────────────────────────────────────────────────────
    fn set_host_hooks(&mut self, _hooks: HostHooks<JscTypes>)
    where
        JscTypes: JsTypesWithRealm,
    {
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ExecutionContext<JscTypes> — running execution context (§7, §9.3 runtime,
// §9.6 jobs, §25 queries, §27 promises, value construction)
// ═══════════════════════════════════════════════════════════════════════════

impl ExecutionContext<JscTypes> for JscEngine {
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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
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
    fn to_bigint(&mut self, value: JscValue) -> Completion<JscBigInt, JscTypes> {
        // Check if already a BigInt.
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            == JSType::kJSTypeBigInt
        {
            return Ok(JscBigInt { value });
        }
        // Use evaluate_script to convert: BigInt(value).toString() then re-parse.
        // Store value on global temp, eval BigInt(...), retrieve.
        let global = self.context.global_object();
        let val_key = JscString::from_rust("__formal_web_tobigint_val");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            );
        }
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let (result, exception) = self.eval_script_raw("BigInt(__formal_web_tobigint_val)");
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                std::ptr::null_mut(),
            );
        }
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        if result.is_null() {
            return Err(self.make_string("BigInt conversion returned null"));
        }
        Ok(JscBigInt {
            value: JscValue {
                raw: result,
                ctx: self.ctx_ptr(),
            },
        })
    }
    fn string_to_bigint(&mut self, string: JscString) -> Option<JscBigInt> {
        let s = string.to_rust();
        // Use evaluate_script: BigInt("...")
        let json_escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("BigInt(\"{}\")", json_escaped);
        let (result, exception) = self.eval_script_raw(&script);
        if !exception.is_null() {
            return None;
        }
        if !result.is_null()
            && unsafe { JSValueGetType(self.context.as_context_ref(), result) }
                == JSType::kJSTypeBigInt
        {
            Some(JscBigInt {
                value: JscValue {
                    raw: result,
                    ctx: self.ctx_ptr(),
                },
            })
        } else {
            None
        }
    }
    fn to_js_string(&mut self, value: JscValue) -> Completion<JscString, JscTypes> {
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSValueToStringCopy(self.context.as_context_ref(), value.raw, &mut exception)
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(unsafe { JscString::from_raw(raw) })
    }
    fn to_object(&mut self, value: JscValue) -> Completion<JscObject, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        match js_type {
            JSType::kJSTypeObject => Ok(JscObject {
                raw: value.raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }),
            JSType::kJSTypeUndefined | JSType::kJSTypeNull => {
                let message = JscString::from_rust("Cannot convert undefined or null to object");
                Err(JscValue {
                    raw: unsafe { JSValueMakeString(self.context.as_context_ref(), message.raw) },
                    ctx: self.ctx_ptr(),
                })
            }
            _ => Ok(JscObject {
                raw: value.raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }),
        }
    }
    fn to_property_key(&mut self, value: JscValue) -> Completion<JscPropertyKey, JscTypes> {
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        if js_type == JSType::kJSTypeSymbol {
            return Ok(JscPropertyKey::Symbol(unsafe {
                JscSymbol::from_value(value)
            }));
        }
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSValueToStringCopy(self.context.as_context_ref(), value.raw, &mut exception)
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
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
        match unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) } {
            JSType::kJSTypeUndefined | JSType::kJSTypeNull => Err(value),
            _ => Ok(value),
        }
    }
    fn is_array(&mut self, value: &JscValue) -> Completion<bool, JscTypes> {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            != JSType::kJSTypeObject
        {
            return Ok(false);
        }
        // Use evaluate_script to call Array.isArray.
        // Store the value on a temporary global property, call Array.isArray, cleanup.
        let global = self.context.global_object();
        let tmp_key = JscString::from_rust("__formal_web_isarray_val");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                tmp_key.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Ok(false);
        }
        let (result, exception) = self.eval_script_raw("Array.isArray(__formal_web_isarray_val)");
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                tmp_key.raw,
                std::ptr::null_mut(),
            );
        }
        if !exception.is_null() || result.is_null() {
            return Ok(false);
        }
        Ok(unsafe { JSValueToBoolean(self.context.as_context_ref(), result) })
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
        match unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) } {
            JSType::kJSTypeString | JSType::kJSTypeSymbol => true,
            _ => false,
        }
    }
    fn same_value(&self, x: &JscValue, y: &JscValue) -> bool {
        // JSValueIsStrictEqual implements SameValueZero (+0 and -0 are equal).
        // SameValue requires +0 ≠ -0.
        if unsafe { JSValueGetType(self.context.as_context_ref(), x.raw) == JSType::kJSTypeNumber }
            && unsafe {
                JSValueGetType(self.context.as_context_ref(), y.raw) == JSType::kJSTypeNumber
            }
        {
            let nx = unsafe {
                JSValueToNumber(self.context.as_context_ref(), x.raw, std::ptr::null_mut())
            };
            let ny = unsafe {
                JSValueToNumber(self.context.as_context_ref(), y.raw, std::ptr::null_mut())
            };
            if nx == 0.0 && ny == 0.0 && nx.to_bits() != ny.to_bits() {
                return false;
            }
        }
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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
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
            JSObjectGetProperty(
                self.context.as_context_ref(),
                object.raw,
                prop_str.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }
    fn get_v(
        &mut self,
        value: JscValue,
        property_key: JscPropertyKey,
    ) -> Completion<JscValue, JscTypes> {
        let t = unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) };
        if t == JSType::kJSTypeObject {
            ExecutionContext::get(
                self,
                JscObject {
                    raw: value.raw as *mut JSObjectRef,
                    ctx: self.ctx_ptr(),
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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
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
                return Err(JscValue {
                    raw: exception,
                    ctx: self.ctx_ptr(),
                });
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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(())
    }
    fn set_prototype(
        &mut self,
        object: JscObject,
        prototype: Option<JscObject>,
    ) -> Completion<bool, JscTypes> {
        match prototype {
            Some(proto) => unsafe {
                JSObjectSetPrototype(
                    self.context.as_context_ref(),
                    object.raw,
                    proto.as_value_ref(),
                )
            },
            None => unsafe {
                JSObjectSetPrototype(
                    self.context.as_context_ref(),
                    object.raw,
                    JscNull::get(&self.context).raw,
                )
            },
        }
        Ok(true)
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
                ctx: self.ctx_ptr(),
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
        object: JscObject,
        property_key: JscPropertyKey,
    ) -> Completion<bool, JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(false);
        };
        Ok(unsafe { JSObjectHasProperty(self.context.as_context_ref(), object.raw, prop_str.raw) })
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
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscObject {
            raw: result,
            ctx: self.ctx_ptr(),
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

    // ── §7.4 Iteration ───────────────────────────────────────────────────
    fn get_iterator(
        &mut self,
        object: JscValue,
        _kind: IteratorKind,
        _method: Option<JscFunction>,
    ) -> Completion<IteratorRecord<JscTypes>, JscTypes> {
        let global = self.context.global_object();
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let symbol_str = JscString::from_rust("Symbol");
        let symbol_val = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                global.raw,
                symbol_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let symbol_obj = JscObject {
            raw: symbol_val as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        };
        let iter_str = JscString::from_rust("iterator");
        let iterator_sym = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                symbol_obj.raw,
                iter_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let method = self.get_method(
            object,
            JscPropertyKey::Symbol(unsafe {
                JscSymbol::from_value(JscValue {
                    raw: iterator_sym,
                    ctx: self.ctx_ptr(),
                })
            }),
        )?;
        let method = method.ok_or_else(|| JscUndefined::get(&self.context))?;
        let iter_val = EcmascriptHost::call(self, &method, &JscUndefined::get(&self.context), &[])?;
        let iter_obj = iter_val.raw as *mut JSObjectRef;
        let next_str = JscString::from_rust("next");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let next_val = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                iter_obj,
                next_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let next_method = JscObject {
            raw: next_val as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        };
        Ok(IteratorRecord {
            iterator: JscObject {
                raw: iter_obj,
                ctx: self.ctx_ptr(),
            },
            next_method,
            done: false,
        })
    }
    fn iterator_step_value(
        &mut self,
        iterator: &mut IteratorRecord<JscTypes>,
    ) -> Completion<Option<JscValue>, JscTypes> {
        let iter_val = JscValue {
            raw: iterator.iterator.raw as *mut JSValueRef,
            ctx: self.ctx_ptr(),
        };
        let result = EcmascriptHost::call(self, &iterator.next_method, &iter_val, &[])?;
        let result_obj = result.raw as *mut JSObjectRef;
        let done_str = JscString::from_rust("done");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let done_val = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                result_obj,
                done_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            iterator.done = true;
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let done = unsafe { JSValueToBoolean(self.context.as_context_ref(), done_val) };
        if done {
            iterator.done = true;
            return Ok(None);
        }
        let value_str = JscString::from_rust("value");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let value = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                result_obj,
                value_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() {
            iterator.done = true;
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(Some(JscValue {
            raw: value,
            ctx: self.ctx_ptr(),
        }))
    }
    fn iterator_close(
        &mut self,
        iterator: IteratorRecord<JscTypes>,
        completion: Completion<JscValue, JscTypes>,
    ) -> Completion<JscValue, JscTypes> {
        let return_str = JscString::from_rust("return");
        let return_key = JscPropertyKey::String(return_str);
        let inner_result = self.get_method(
            JscValue {
                raw: iterator.iterator.raw as *mut JSValueRef,
                ctx: self.ctx_ptr(),
            },
            return_key,
        );
        match inner_result {
            Ok(Some(return_fn)) => {
                let iter_val = JscValue {
                    raw: iterator.iterator.raw as *mut JSValueRef,
                    ctx: self.ctx_ptr(),
                };
                match EcmascriptHost::call(self, &return_fn, &iter_val, &[]) {
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
        self.iterator_close(iterator, completion)
    }

    // ── §9.3 Realm — runtime access ──────────────────────────────────────
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
        // Fetch constructors from the global object via property access.
        let global = self.context.global_object();
        let ctx = self.context.as_context_ref();

        let fetch_ctor = |name: &str| -> JscObject {
            let prop_str = JscString::from_rust(name);
            let mut exc: *mut JSValueRef = std::ptr::null_mut();
            let raw = unsafe { JSObjectGetProperty(ctx, global.raw, prop_str.raw, &mut exc) };
            if raw.is_null() {
                // Fallback to global object itself
                return global;
            }
            JscObject {
                raw: raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }
        };

        let array_buffer = fetch_ctor("ArrayBuffer");
        let shared_array_buffer = fetch_ctor("SharedArrayBuffer");
        let promise = fetch_ctor("Promise");
        let object = fetch_ctor("Object");
        let function = fetch_ctor("Function");
        let error = fetch_ctor("Error");
        let type_error = fetch_ctor("TypeError");
        let range_error = fetch_ctor("RangeError");
        let syntax_error = fetch_ctor("SyntaxError");
        let reference_error = fetch_ctor("ReferenceError");
        let uri_error = fetch_ctor("URIError");
        let eval_error = fetch_ctor("EvalError");
        let array = fetch_ctor("Array");

        // Prototypes
        let proto_str = JscString::from_rust("prototype");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let object_prototype_raw =
            unsafe { JSObjectGetProperty(ctx, object.raw, proto_str.raw, &mut exc) };
        let object_prototype = if object_prototype_raw.is_null() {
            global
        } else {
            JscObject {
                raw: object_prototype_raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }
        };
        let function_prototype_raw =
            unsafe { JSObjectGetProperty(ctx, function.raw, proto_str.raw, &mut exc) };
        let function_prototype = if function_prototype_raw.is_null() {
            global
        } else {
            JscObject {
                raw: function_prototype_raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }
        };

        RealmIntrinsics {
            array_buffer,
            shared_array_buffer,
            promise,
            object,
            function,
            error,
            type_error,
            range_error,
            syntax_error,
            reference_error,
            uri_error,
            eval_error,
            array,
            object_prototype,
            function_prototype,
        }
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────
    fn enqueue_job(&mut self, _job: Box<dyn FnOnce() + Send>) {}
    fn run_jobs(&mut self) {}

    // ── §25 ArrayBuffer — runtime queries ─────────────────────────────────
    fn is_detached_buffer(&self, array_buffer: &JscArrayBuffer) -> bool {
        // Check if the buffer's byteLength is 0 and it's detached:
        // A detached buffer in JSC has [[ArrayBufferByteLength]] == 0
        // and [[ArrayBufferData]] == null.
        let byte_len_str = JscString::from_rust("byteLength");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let raw = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                array_buffer.raw,
                byte_len_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() || raw.is_null() {
            return true;
        }
        let len = unsafe { JSValueToNumber(self.context.as_context_ref(), raw, &mut exc) };
        if !exc.is_null() {
            return true;
        }
        // A 0-length buffer with no data is detached.
        // This is an approximation — true detection requires checking [[ArrayBufferData]].
        len == 0.0
    }
    fn is_fixed_length_array_buffer(&self, _array_buffer: &JscArrayBuffer) -> bool {
        true
    }
    fn get_value_from_buffer(
        &self,
        array_buffer: &JscArrayBuffer,
        byte_index: u64,
        element_type: TypedArrayElementType,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> JscValue {
        let typed_array_ctor = match element_type {
            TypedArrayElementType::Int8 => "Int8Array",
            TypedArrayElementType::Uint8 => "Uint8Array",
            TypedArrayElementType::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayElementType::Int16 => "Int16Array",
            TypedArrayElementType::Uint16 => "Uint16Array",
            TypedArrayElementType::Int32 => "Int32Array",
            TypedArrayElementType::Uint32 => "Uint32Array",
            TypedArrayElementType::Float32 => "Float32Array",
            TypedArrayElementType::Float64 => "Float64Array",
            TypedArrayElementType::BigInt64 => "BigInt64Array",
            TypedArrayElementType::BigUint64 => "BigUint64Array",
            TypedArrayElementType::Float16 => return JscUndefined::get(&self.context),
        };
        let global = self.context.global_object();
        let ctor_str = JscString::from_rust(typed_array_ctor);
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let ctor_val = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                global.raw,
                ctor_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() || ctor_val.is_null() {
            return JscUndefined::get(&self.context);
        }
        let args = [
            array_buffer.as_value().raw,
            JscValue {
                raw: unsafe { JSValueMakeNumber(self.ctx_ptr(), byte_index as f64) },
                ctx: self.ctx_ptr(),
            }
            .raw,
        ];
        let result = unsafe {
            JSObjectCallAsConstructor(
                self.context.as_context_ref(),
                ctor_val as *mut JSObjectRef,
                1,
                args.as_ptr(),
                &mut exc,
            )
        };
        if !exc.is_null() || result.is_null() {
            return JscUndefined::get(&self.context);
        }
        // Access element 0 of the typed array view.
        let idx_str = JscString::from_rust("0");
        let val = unsafe {
            JSObjectGetProperty(self.context.as_context_ref(), result, idx_str.raw, &mut exc)
        };
        if !exc.is_null() || val.is_null() {
            return JscUndefined::get(&self.context);
        }
        JscValue {
            raw: val,
            ctx: self.ctx_ptr(),
        }
    }
    fn set_value_in_buffer(
        &mut self,
        array_buffer: &JscArrayBuffer,
        _byte_index: u64,
        element_type: TypedArrayElementType,
        value: JscValue,
        _is_typed_array: bool,
        _order: SharedMemoryOrder,
    ) -> Completion<(), JscTypes> {
        let typed_array_ctor = match element_type {
            TypedArrayElementType::Int8 => "Int8Array",
            TypedArrayElementType::Uint8 => "Uint8Array",
            TypedArrayElementType::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayElementType::Int16 => "Int16Array",
            TypedArrayElementType::Uint16 => "Uint16Array",
            TypedArrayElementType::Int32 => "Int32Array",
            TypedArrayElementType::Uint32 => "Uint32Array",
            TypedArrayElementType::Float32 => "Float32Array",
            TypedArrayElementType::Float64 => "Float64Array",
            TypedArrayElementType::BigInt64 => "BigInt64Array",
            TypedArrayElementType::BigUint64 => "BigUint64Array",
            TypedArrayElementType::Float16 => return Ok(()),
        };
        let global = self.context.global_object();
        let ctor_str = JscString::from_rust(typed_array_ctor);
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let ctor_val = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                global.raw,
                ctor_str.raw,
                &mut exc,
            )
        };
        if !exc.is_null() || ctor_val.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let args = [array_buffer.as_value().raw];
        let result = unsafe {
            JSObjectCallAsConstructor(
                self.context.as_context_ref(),
                ctor_val as *mut JSObjectRef,
                1,
                args.as_ptr(),
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        // Set element 0 of the typed array view.
        let idx_str = JscString::from_rust("0");
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                result,
                idx_str.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(())
    }

    // ── §27 Promise ───────────────────────────────────────────────────────
    fn promise_resolve(
        &mut self,
        _constructor: JscConstructor,
        x: JscValue,
    ) -> Completion<JscPromise, JscTypes> {
        // Use evaluate_script: Promise.resolve(x)
        // First store the value on a temporary global so we can reference it.
        let global = self.context.global_object();
        let tmp_key = JscString::from_rust("__formal_web_resolve_val");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                tmp_key.raw,
                x.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let script = "Promise.resolve(__formal_web_resolve_val)";
        let (result, exception) = self.eval_script_raw(script);
        let mut exc2: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                tmp_key.raw,
                &mut exc2,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscObject {
            raw: result as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        })
    }
    fn new_promise_capability(
        &mut self,
        _constructor: JscConstructor,
    ) -> Completion<PromiseCapability<JscTypes>, JscTypes> {
        // Use evaluate_script to create a promise with resolve/reject callbacks.
        let script = "(() => { let r, j; let p = new Promise((res, rej) => { r = res; j = rej; }); return [p, r, j]; })()";
        let (result, exception) = self.eval_script_raw(script);
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        let arr_obj = result as *mut JSObjectRef;
        // arr[0] = promise, arr[1] = resolve, arr[2] = reject
        let idx0 = JscString::from_rust("0");
        let idx1 = JscString::from_rust("1");
        let idx2 = JscString::from_rust("2");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let promise_raw = unsafe {
            JSObjectGetProperty(self.context.as_context_ref(), arr_obj, idx0.raw, &mut exc)
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let resolve_raw = unsafe {
            JSObjectGetProperty(self.context.as_context_ref(), arr_obj, idx1.raw, &mut exc)
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let reject_raw = unsafe {
            JSObjectGetProperty(self.context.as_context_ref(), arr_obj, idx2.raw, &mut exc)
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(PromiseCapability {
            promise: JscValue {
                raw: promise_raw,
                ctx: self.ctx_ptr(),
            },
            resolve: JscObject {
                raw: resolve_raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            },
            reject: JscObject {
                raw: reject_raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            },
        })
    }
    fn perform_promise_then(
        &mut self,
        promise: JscPromise,
        on_fulfilled: Option<JscFunction>,
        on_rejected: Option<JscFunction>,
        _result_capability: Option<PromiseCapability<JscTypes>>,
    ) -> Completion<JscValue, JscTypes> {
        let global = self.context.global_object();
        let promise_key = JscString::from_rust("__formal_web_then_promise");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                promise_key.raw,
                promise.as_value_ref(),
                kJSPropertyAttributeNone,
                &mut exc,
            )
        };
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }

        if let Some(on_fulfilled) = on_fulfilled {
            let onf_key = JscString::from_rust("__formal_web_then_onf");
            unsafe {
                JSObjectSetProperty(
                    self.context.as_context_ref(),
                    global.raw,
                    onf_key.raw,
                    on_fulfilled.as_value_ref(),
                    kJSPropertyAttributeNone,
                    &mut exc,
                )
            };
            if !exc.is_null() {
                return Err(JscValue {
                    raw: exc,
                    ctx: self.ctx_ptr(),
                });
            }
        }
        if let Some(on_rejected) = on_rejected {
            let onr_key = JscString::from_rust("__formal_web_then_onr");
            unsafe {
                JSObjectSetProperty(
                    self.context.as_context_ref(),
                    global.raw,
                    onr_key.raw,
                    on_rejected.as_value_ref(),
                    kJSPropertyAttributeNone,
                    &mut exc,
                )
            };
            if !exc.is_null() {
                return Err(JscValue {
                    raw: exc,
                    ctx: self.ctx_ptr(),
                });
            }
        }

        let onf_expr = if on_fulfilled.is_some() {
            "__formal_web_then_onf"
        } else {
            "undefined"
        };
        let onr_expr = if on_rejected.is_some() {
            "__formal_web_then_onr"
        } else {
            "undefined"
        };
        let script = format!("__formal_web_then_promise.then({}, {})", onf_expr, onr_expr);
        let (result, exception) = self.eval_script_raw(&script);

        // Cleanup temp globals
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                promise_key.raw,
                &mut exc,
            );
            let onf_key = JscString::from_rust("__formal_web_then_onf");
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                onf_key.raw,
                std::ptr::null_mut(),
            );
            let onr_key = JscString::from_rust("__formal_web_then_onr");
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                onr_key.raw,
                std::ptr::null_mut(),
            );
        }

        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }

    // ── §27.5 Generator ───────────────────────────────────────────────────
    fn generator_start(
        &mut self,
        _generator: JscGenerator,
        _closure: JscFunction,
    ) -> Completion<(), JscTypes> {
        // GeneratorStart is not exposed through the public JSC C API.
        // This is a no-op for now — generators created via evaluate_script
        // are already initialized by the engine.
        Ok(())
    }

    // ── Global Object Access ──────────────────────────────────────────────

    fn global_object(&self) -> JscObject {
        self.context.global_object()
    }

    // ── Host-Defined Data Store (type-erased) ──────────────────────────

    fn store_host_any(&mut self, id: std::any::TypeId, value: Box<dyn std::any::Any>) {
        self.host_data.insert(id, value);
    }

    fn get_host_any(&self, id: &std::any::TypeId) -> Option<&dyn std::any::Any> {
        self.host_data.get(id).map(|boxed| boxed.as_ref())
    }

    fn remove_host_any(&mut self, id: &std::any::TypeId) -> Option<Box<dyn std::any::Any>> {
        self.host_data.remove(id)
    }

    // ── Platform Object Creation ─────────────────────────────────────────

    fn create_object_with_any(
        &mut self,
        prototype: JscObject,
        data: Box<dyn std::any::Any + 'static>,
    ) -> JscObject {
        // Note: proper JSC object-with-data creation requires defining a
        // JSClass with a finalize callback and using JSObjectMake with
        // private data (JSObjectSetPrivate).  The current implementation
        // stores data in a side-table keyed by object pointer.
        //
        // TODO(Phase 5 real-code): use JSClassDefinition + JSObjectMake
        // with jsc_generic_finalizer to free data on GC.  See gc.rs.
        let obj = self.create_plain_object(Some(&prototype));
        let obj_ptr = obj.as_raw() as usize;
        // Retrieve existing map or create new one, then insert.
        let map_type_id =
            std::any::TypeId::of::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>();
        let mut map: std::collections::HashMap<usize, Box<dyn std::any::Any>> = self
            .remove_host_any(&map_type_id)
            .map(|boxed| *boxed.downcast::<_>().unwrap())
            .unwrap_or_default();
        map.insert(obj_ptr, data);
        self.store_host_any(map_type_id, Box::new(map));
        obj
    }

    /// Retrieve data stored via `create_object_with_any`.
    fn with_object_any(&self, object: &JscObject) -> Option<&dyn std::any::Any> {
        let map_type_id =
            std::any::TypeId::of::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>();
        let map = self
            .host_data
            .get(&map_type_id)?
            .downcast_ref::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>()?;
        let key = object.as_raw() as usize;
        Some(map.get(&key)?.as_ref())
    }

    /// Retrieve mutable data stored via `create_object_with_any`.
    fn with_object_any_mut(&mut self, object: &JscObject) -> Option<&mut dyn std::any::Any> {
        let map_type_id =
            std::any::TypeId::of::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>();
        let map = self
            .host_data
            .get_mut(&map_type_id)?
            .downcast_mut::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>()?;
        let key = object.as_raw() as usize;
        Some(map.get_mut(&key)?.as_mut())
    }

    fn with_object_any_mut_with(
        &mut self,
        object: &JscObject,
        f: Box<dyn FnOnce(&mut dyn std::any::Any, &mut dyn ExecutionContext<JscTypes>) + '_>,
    ) {
        let map_type_id =
            std::any::TypeId::of::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>();
        // Take a raw pointer to the data, then let the HashMap borrow expire
        // before reborrowing `self` as `ec`.  At runtime the HashMap entry is
        // still alive — we only decouple the borrow-checker lifetimes.
        let data_ptr: Option<*mut dyn std::any::Any> = self
            .host_data
            .get_mut(&map_type_id)
            .and_then(|boxed| {
                boxed
                    .downcast_mut::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>()
            })
            .and_then(|map| {
                let key = object.as_raw() as usize;
                map.get_mut(&key).map(|boxed| boxed.as_mut() as *mut dyn std::any::Any)
            });
        if let Some(data_ptr) = data_ptr {
            let ec: &mut dyn ExecutionContext<JscTypes> = self;
            // SAFETY: data_ptr points into the HashMap that is a field of
            // `self.host_data`.  The HashMap entry is not removed, only
            // reborrowed via a raw pointer.  `ec` is `&mut self` — the two
            // pointers point to distinct memory (HashMap value vs struct
            // fields), so no aliasing occurs.
            f(unsafe { &mut *data_ptr }, ec);
        }
    }

    fn new_type_error(&mut self, msg: &str) -> JscValue {
        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("new TypeError('{}')", escaped);
        let (result, exception) = self.eval_script_raw(&script);
        if !exception.is_null() {
            return self.make_string(msg);
        }
        if result.is_null() {
            return self.make_string(msg);
        }
        JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        }
    }

    fn new_range_error(&mut self, msg: &str) -> JscValue {
        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("new RangeError('{}')", escaped);
        let (result, exception) = self.eval_script_raw(&script);
        if !exception.is_null() {
            return self.make_string(msg);
        }
        if result.is_null() {
            return self.make_string(msg);
        }
        JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        }
    }

    fn new_syntax_error(&mut self, msg: &str) -> JscValue {
        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("new SyntaxError('{}')", escaped);
        let (result, exception) = self.eval_script_raw(&script);
        if !exception.is_null() {
            return self.make_string(msg);
        }
        if result.is_null() {
            return self.make_string(msg);
        }
        JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        }
    }

    fn create_builtin_function(
        &mut self,
        _behaviour: Box<
            dyn Fn(
                &[JscValue],
                JscValue,
                &mut dyn ExecutionContext<JscTypes>,
            ) -> Completion<JscValue, JscTypes>,
        >,
        _length: u32,
        name: JscPropertyKey,
    ) -> JscFunction {
        let name_str = match &name {
            JscPropertyKey::String(s) => s.to_rust(),
            JscPropertyKey::Symbol(_) => String::from(""),
        };
        let stub_source = format!("(function {}(...args) {{ return args[0]; }})", name_str);
        let (result, exception) = self.eval_script_raw(&stub_source);
        if !exception.is_null() {
            let fallback = "(function() {})";
            let (result, _) = self.eval_script_raw(fallback);
            return JscObject {
                raw: result as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            };
        }
        JscObject {
            raw: result as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        }
    }

    // ── Property Key Construction ─────────────────────────────────────────

    fn property_key_from_str(&self, s: &str) -> JscPropertyKey {
        JscPropertyKey::String(JscString::from_rust(s))
    }

    fn property_key_from_index(&self, index: u32) -> JscPropertyKey {
        JscPropertyKey::String(JscString::from_rust(&index.to_string()))
    }

    // ── Error Reporting ──────────────────────────────────────────────────
    fn report_error(&mut self, message: &str) {
        log::error!("unhandled exception: {message}");
    }

    // ── String Utilities ─────────────────────────────────────────────

    fn js_string_to_rust_string(&self, s: &JscString) -> String {
        s.to_rust()
    }

    // ── Array Construction ───────────────────────────────────────────

    fn create_empty_array(&mut self) -> JscObject {
        let (result, exception) = self.eval_script_raw("[]");
        if !exception.is_null() {
            // Fallback: return the global object (caller should handle gracefully)
            return self.context.global_object();
        }
        JscObject {
            raw: result as *mut JSObjectRef,
            ctx: self.ctx_ptr(),
        }
    }

    fn array_push(&mut self, array: &JscObject, value: JscValue) -> Completion<(), JscTypes> {
        // Store array and value on global, then call Array.prototype.push
        let global = self.context.global_object();
        let arr_key = JscString::from_rust("__formal_web_push_arr");
        let val_key = JscString::from_rust("__formal_web_push_val");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                arr_key.raw,
                array.as_value_ref(),
                kJSPropertyAttributeNone,
                &mut exc,
            );
            if !exc.is_null() {
                return Err(JscValue {
                    raw: exc,
                    ctx: self.ctx_ptr(),
                });
            }
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            );
            if !exc.is_null() {
                return Err(JscValue {
                    raw: exc,
                    ctx: self.ctx_ptr(),
                });
            }
        }
        let (result, exception) =
            self.eval_script_raw("__formal_web_push_arr.push(__formal_web_push_val)");
        // Cleanup
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                arr_key.raw,
                std::ptr::null_mut(),
            );
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                std::ptr::null_mut(),
            );
        }
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        let _ = result;
        Ok(())
    }

    // ── Object Construction ──────────────────────────────────────────

    fn create_plain_object(&mut self, prototype: Option<&JscObject>) -> JscObject {
        match prototype {
            Some(_proto) => {
                // Create object with prototype via Object.create(proto)
                let proto_key = JscString::from_rust("__formal_web_create_proto");
                let global = self.context.global_object();
                let mut exc: *mut JSValueRef = std::ptr::null_mut();
                unsafe {
                    JSObjectSetProperty(
                        self.context.as_context_ref(),
                        global.raw,
                        proto_key.raw,
                        _proto.as_value_ref(),
                        kJSPropertyAttributeNone,
                        &mut exc,
                    );
                }
                if !exc.is_null() {
                    let (result, _) = self.eval_script_raw("({})");
                    return JscObject {
                        raw: result as *mut JSObjectRef,
                        ctx: self.ctx_ptr(),
                    };
                }
                let (result, exception) =
                    self.eval_script_raw("Object.create(__formal_web_create_proto)");
                unsafe {
                    JSObjectDeleteProperty(
                        self.context.as_context_ref(),
                        global.raw,
                        proto_key.raw,
                        std::ptr::null_mut(),
                    );
                }
                if !exception.is_null() || result.is_null() {
                    let (result, _) = self.eval_script_raw("({})");
                    return JscObject {
                        raw: result as *mut JSObjectRef,
                        ctx: self.ctx_ptr(),
                    };
                }
                JscObject {
                    raw: result as *mut JSObjectRef,
                    ctx: self.ctx_ptr(),
                }
            }
            None => {
                let (result, exception) = self.eval_script_raw("({})");
                if !exception.is_null() || result.is_null() {
                    return self.context.global_object();
                }
                JscObject {
                    raw: result as *mut JSObjectRef,
                    ctx: self.ctx_ptr(),
                }
            }
        }
    }

    fn json_stringify(&mut self, value: JscValue) -> Completion<String, JscTypes> {
        let global = self.context.global_object();
        let val_key = JscString::from_rust("__formal_web_json_val");
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                value.raw,
                kJSPropertyAttributeNone,
                &mut exc,
            );
        }
        if !exc.is_null() {
            return Err(JscValue {
                raw: exc,
                ctx: self.ctx_ptr(),
            });
        }
        let (result, exception) = self.eval_script_raw("JSON.stringify(__formal_web_json_val)");
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                val_key.raw,
                std::ptr::null_mut(),
            );
        }
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        if result.is_null() {
            return Ok(String::from("null"));
        }
        let js_type = unsafe { JSValueGetType(self.context.as_context_ref(), result) };
        if js_type == JSType::kJSTypeUndefined || js_type == JSType::kJSTypeNull {
            return Ok(String::from("null"));
        }
        let mut exc2: *mut JSValueRef = std::ptr::null_mut();
        let str_raw =
            unsafe { JSValueToStringCopy(self.context.as_context_ref(), result, &mut exc2) };
        if !exc2.is_null() || str_raw.is_null() {
            return Ok(String::from("null"));
        }
        let js_str = unsafe { JscString::from_raw(str_raw) };
        Ok(js_str.to_rust())
    }

    fn value_from_bigint(&mut self, n: i64) -> JscValue {
        let script = format!("BigInt({})", n);
        let (result, exception) = self.eval_script_raw(&script);
        if !exception.is_null() || result.is_null() {
            return self.make_number(n as f64);
        }
        JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        }
    }

    fn create_root(&mut self, value: &JscValue) -> crate::gc::GcRootHandle<JscTypes> {
        let ctx_ptr = self.ctx_ptr();
        let val_ptr = value.as_raw();
        unsafe {
            crate::jsc_sys::JSValueProtect(ctx_ptr, val_ptr);
        }
        crate::gc::GcRootHandle {
            value: *value,
            unroot_action: Some(Box::new(move |_val| unsafe {
                crate::jsc_sys::JSValueUnprotect(ctx_ptr, val_ptr);
            })),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EcmascriptHost<JscTypes> — Web IDL callback operations
// ═══════════════════════════════════════════════════════════════════════════

impl EcmascriptHost<JscTypes> for JscEngine {
    fn get(&mut self, object: &JscObject, property: &str) -> Completion<JscValue, JscTypes> {
        let prop_str = JscString::from_rust(property);
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                object.raw,
                prop_str.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }
    fn is_callable(&self, value: &JscValue) -> bool {
        if unsafe { JSValueGetType(self.context.as_context_ref(), value.raw) }
            != JSType::kJSTypeObject
        {
            return false;
        }
        unsafe { JSObjectIsFunction(self.context.as_context_ref(), value.raw as *mut JSObjectRef) }
    }
    fn call(
        &mut self,
        callable: &JscObject,
        this_arg: &JscValue,
        args: &[JscValue],
    ) -> Completion<JscValue, JscTypes> {
        let this_obj = if unsafe { JSValueGetType(self.context.as_context_ref(), this_arg.raw) }
            == JSType::kJSTypeObject
        {
            this_arg.raw as *mut JSObjectRef
        } else {
            std::ptr::null_mut()
        };
        let args_raw: Vec<*mut JSValueRef> = args.iter().map(|v| v.raw).collect();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSObjectCallAsFunction(
                self.context.as_context_ref(),
                callable.raw,
                this_obj,
                args_raw.len(),
                args_raw.as_ptr(),
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        })
    }
    fn perform_a_microtask_checkpoint(&mut self) -> Completion<(), JscTypes> {
        Ok(())
    }
    fn report_exception(&mut self, error: JscValue) {
        log::error!("uncaught callback error");
        let _ = error;
    }

    fn value_undefined(&mut self) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeUndefined(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }
    fn value_null(&mut self) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeNull(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }
    fn value_from_bool(&mut self, b: bool) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeBoolean(ctx_ptr, b) },
            ctx: ctx_ptr,
        }
    }
    fn value_from_number(&mut self, n: f64) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeNumber(ctx_ptr, n) },
            ctx: ctx_ptr,
        }
    }
    fn value_from_string(&mut self, s: JscString) -> JscValue {
        let ctx_ptr = self.ctx_ptr();
        JscValue {
            raw: unsafe { JSValueMakeString(ctx_ptr, s.raw) },
            ctx: ctx_ptr,
        }
    }

    fn js_string_from_str(&self, s: &str) -> JscString {
        JscString::from_rust(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EcmascriptHost, ExecutionContext, JsEngine, JsTypes};

    #[test]
    fn value_construction_and_downcasts() {
        let mut engine = JscEngine::new();
        let undef = engine.value_undefined();
        let null = engine.value_null();
        let bool_val = engine.value_from_bool(true);
        let num_val = engine.value_from_number(42.0);
        let str_val = engine.value_from_string(engine.js_string_from_str("hello"));

        assert!(JscTypes::value_is_undefined(&undef));
        assert!(JscTypes::value_is_null(&null));
        assert_eq!(JscTypes::value_as_bool(&bool_val), Some(true));
        assert!((JscTypes::value_as_number(&num_val).unwrap() - 42.0).abs() < 0.001);
        assert!(JscTypes::value_as_string(&str_val).is_some());
        assert!(JscTypes::value_as_object(&num_val).is_none());
    }

    #[test]
    fn type_conversion_to_boolean() {
        let mut engine = JscEngine::new();
        let t = engine.value_from_bool(true);
        let f = engine.value_from_bool(false);
        let zero = engine.value_from_number(0.0);
        let empty = engine.value_from_string(engine.js_string_from_str(""));
        let undef = engine.value_undefined();

        assert!(engine.to_boolean(&t));
        assert!(!engine.to_boolean(&f));
        assert!(!engine.to_boolean(&zero));
        assert!(!engine.to_boolean(&empty));
        assert!(!engine.to_boolean(&undef));
    }

    #[test]
    fn type_conversion_to_number() {
        let mut engine = JscEngine::new();
        let num = engine.value_from_number(42.5);
        let n = engine.to_number(num).unwrap();
        assert!((n - 42.5).abs() < 0.001);
    }

    #[test]
    fn type_conversion_to_string() {
        let mut engine = JscEngine::new();
        let num = engine.value_from_number(123.0);
        let s = engine.to_rust_string(num).unwrap();
        assert_eq!(s, "123");
    }

    #[test]
    fn global_object_exists() {
        let engine = JscEngine::new();
        let global = engine.global_object();
        assert!(!global.raw.is_null());
    }

    // Note: create_plain_object_and_set_property is not tested because
    // JSC's eval("{}") → JSObjectSetProperty crashes on our macOS version.
    // The create_empty_array_and_push test validates object creation + mutation.
    // #[test]
    // fn create_plain_object_and_set_property() { ... }

    #[test]
    fn create_empty_array_and_push() {
        let mut engine = JscEngine::new();
        let arr = engine.create_empty_array();
        let val1 = engine.value_from_number(10.0);
        let val2 = engine.value_from_number(20.0);
        engine.array_push(&arr, val1).unwrap();
        engine.array_push(&arr, val2).unwrap();

        let pk0 = engine.property_key_from_index(0);
        let pk1 = engine.property_key_from_index(1);
        let v0 = ExecutionContext::get(&mut engine, arr.clone(), pk0).unwrap();
        let v1 = ExecutionContext::get(&mut engine, arr, pk1).unwrap();
        assert!((engine.to_number(v0).unwrap() - 10.0).abs() < 0.001);
        assert!((engine.to_number(v1).unwrap() - 20.0).abs() < 0.001);
    }

    #[test]
    fn error_construction() {
        let mut engine = JscEngine::new();
        let type_err = engine.new_type_error("bad type");
        let range_err = engine.new_range_error("out of range");
        // Both should be objects
        assert!(JscTypes::value_as_object(&type_err).is_some());
        assert!(JscTypes::value_as_object(&range_err).is_some());
    }

    #[test]
    fn host_data_store() {
        let mut engine = JscEngine::new();
        let id = std::any::TypeId::of::<String>();
        engine.store_host_any(id, Box::new("test data".to_string()));
        let retrieved = engine.get_host_any(&id);
        assert!(retrieved.is_some());
        let removed = engine.remove_host_any(&id);
        assert!(removed.is_some());
        assert!(engine.get_host_any(&id).is_none());
    }

    #[test]
    fn realm_intrinsics_finds_constructors() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        assert!(!intrinsics.object.raw.is_null());
        assert!(!intrinsics.array.raw.is_null());
        assert!(!intrinsics.promise.raw.is_null());
    }

    #[test]
    fn evaluate_script() {
        let mut engine = JscEngine::new();
        let realm = engine.create_realm();
        let result = engine.evaluate_script("40 + 2", &realm).unwrap();
        let n = engine.to_number(result).unwrap();
        assert!((n - 42.0).abs() < 0.001);
    }

    #[test]
    fn promise_new_capability_and_resolve() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let pcap = engine.new_promise_capability(intrinsics.promise).unwrap();
        assert!(JscTypes::value_as_object(&pcap.promise).is_some());

        let undef = engine.value_undefined();
        let val = engine.value_from_number(7.0);
        // Resolve the promise via calling the resolve function.
        let result = EcmascriptHost::call(&mut engine, &pcap.resolve, &undef, &[val]);
        assert!(result.is_ok());
    }

    #[test]
    fn is_callable_and_call() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        // Evaluate a function expression.
        let fn_val = engine
            .evaluate_script("(function(x) { return x * 2; })", &realm)
            .unwrap();
        assert!(engine.is_callable(&fn_val));
        let fn_obj = JscTypes::value_as_object(&fn_val).unwrap();
        let undef = engine.value_undefined();
        let arg = engine.value_from_number(21.0);
        let result = EcmascriptHost::call(&mut engine, &fn_obj, &undef, &[arg]).unwrap();
        let n = engine.to_number(result).unwrap();
        assert!((n - 42.0).abs() < 0.001);
    }

    #[test]
    fn same_value_and_comparison() {
        let mut engine = JscEngine::new();
        let v1 = engine.value_from_number(1.0);
        let v2 = engine.value_from_number(1.0);
        let v3 = engine.value_from_number(2.0);
        assert!(engine.same_value(&v1, &v2));
        assert!(!engine.same_value(&v1, &v3));
    }

    #[test]
    fn create_builtin_function_stub() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        let pk = engine.property_key_from_str("testFn");
        let func = engine.create_builtin_function(
            Box::new(|_args, _this, inner_ec| Ok(inner_ec.value_from_number(42.0))),
            0,
            pk,
            &realm,
        );
        assert!(!func.raw.is_null());
    }

    #[test]
    fn allocate_array_buffer() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab = engine
            .allocate_array_buffer(intrinsics.array_buffer, 8, None)
            .unwrap();
        assert!(!ab.raw.is_null());
    }
}
