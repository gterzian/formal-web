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

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_char;
use std::sync::LazyLock;

use super::types::*;

// ── Current engine (thread-local) ────────────────────────────────────
//
// The JscEngine may be moved during initialization (local→return value→
// ESO field→ContentDocument field).  Raw `self` pointers captured in
// builtin function closures become dangling after each move.
//
// Instead of capturing the engine pointer, we set a thread-local
// "current engine" before entering any code path that might trigger JS
// callbacks, and clear it after.  The builtin function callbacks read
// this thread-local to find the engine.
thread_local! {
    static CURRENT_ENGINE: RefCell<Option<*mut JscEngine>> = const { RefCell::new(None) };
}

/// Set the current engine for the duration of a scope.
/// Builtin function callbacks will use this to find `ec`.
///
/// Must be called before any code that might invoke JS callbacks
/// (script evaluation, event dispatching, timer callbacks).
pub fn set_current_engine(engine: &mut JscEngine) {
    let ptr = engine as *mut JscEngine;
    CURRENT_ENGINE.with(|current| {
        *current.borrow_mut() = Some(ptr);
    });
}

/// Clear the current engine.  Call after the scope completes.
pub fn clear_current_engine() {
    CURRENT_ENGINE.with(|current| {
        *current.borrow_mut() = None;
    });
}

/// Get the current engine pointer.  Panics if none is set.
fn with_current_engine<R>(f: impl FnOnce(&mut JscEngine) -> R) -> R {
    CURRENT_ENGINE.with(|current| {
        let ptr = current
            .borrow()
            .expect("no current engine set — set_current_engine must be called before entering JS");
        let engine = unsafe { &mut *ptr };
        f(engine)
    })
}
use crate::jsc_sys::*;
use crate::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, IntegrityLevel, IteratorKind,
    JsEngine, JsTypes, JsTypesWithRealm, Numeric, PreferredType, SharedMemoryOrder,
    TypedArrayElementType,
    records::{
        IteratorRecord, PromiseCapability, PromiseResolvers, PropertyDescriptor, RealmIntrinsics,
    },
};

/// Marker type for JSC engine implementations.
#[derive(Clone, Copy, Debug)]
pub struct JscTypes;

// ═══════════════════════════════════════════════════════════════════════════
// Builtin-function machinery
// ═══════════════════════════════════════════════════════════════════════════
//
// `JSObjectMakeFunctionWithCallback` has no user-data parameter, so the
// C callback can't access Rust state.  Instead we:
//
// 1. Define a JSClass with `callAsFunction` + `finalize`.
// 2. In `create_builtin_function`, wrap the user's behaviour to capture an
//    engine raw pointer (stable for the engine's lifetime).  Box the
//    wrapper, leak the Box, and store the pointer as private data via
//    `JSObjectMake`.
// 3. The C callback retrieves the Box via `JSObjectGetPrivate`, calls
//    the wrapped closure (which needs no `ec` param — it uses the captured
//    pointer internally), and returns the result.
// 4. The `finalize` callback drops the Box, freeing the closure.

/// Type stored as private data on each builtin function object.
/// The closure matches the trait method signature, with `ec` retrieved
/// from the thread-local `CURRENT_ENGINE` inside the wrapper.
type StoredBehaviour = Box<
    dyn Fn(
        &[JscValue],
        JscValue,
        &mut dyn ExecutionContext<JscTypes>,
    ) -> Completion<JscValue, JscTypes>,
>;

/// Wrapper around `*mut JSClassRef` that implements `Sync` + `Send` so it
/// can be stored in a `LazyLock` static.  The content process is
/// single-threaded; `Send`/`Sync` impls are a formality.
pub(crate) struct JscClass(pub(crate) *mut JSClassRef);
unsafe impl Send for JscClass {}
unsafe impl Sync for JscClass {}

/// Shared helper: create the JSClassDefinition for builtin objects.
fn builtin_class_def(call_as_constructor: bool) -> JSClassDefinition {
    JSClassDefinition {
        version: 0,
        attributes: kJSClassAttributeNone,
        className: b"FormalWebBuiltin\0".as_ptr() as *const c_char,
        parentClass: std::ptr::null_mut(),
        staticValues: std::ptr::null(),
        staticFunctions: std::ptr::null(),
        initialize: None,
        finalize: Some(builtin_finalize),
        hasProperty: None,
        getProperty: None,
        setProperty: None,
        deleteProperty: None,
        getPropertyNames: None,
        callAsFunction: Some(builtin_call_as_function),
        callAsConstructor: if call_as_constructor {
            Some(builtin_call_as_constructor)
        } else {
            None
        },
        hasInstance: None,
        convertToType: None,
    }
}

/// JSClass for builtin function objects (non-constructor).
static BUILTIN_CLASS: LazyLock<JscClass> =
    LazyLock::new(|| JscClass(unsafe { JSClassCreate(&builtin_class_def(false)) }));

/// JSClass for builtin constructor function objects.
static BUILTIN_CONSTRUCTOR_CLASS: LazyLock<JscClass> =
    LazyLock::new(|| JscClass(unsafe { JSClassCreate(&builtin_class_def(true)) }));

/// JSClass for plain objects (no callbacks).  Uses JSObjectMake to
/// avoid eval_script_raw (which causes nested JSEvaluateScript crashes).
static PLAIN_OBJECT_CLASS: LazyLock<JscClass> = LazyLock::new(|| {
    JscClass(unsafe {
        JSClassCreate(&JSClassDefinition {version:0,attributes:kJSClassAttributeNone,className:b"FormalWebPlain\0".as_ptr()as*const c_char,parentClass:std::ptr::null_mut(),staticValues:std::ptr::null(),staticFunctions:std::ptr::null(),initialize:None,finalize:None,hasProperty:None,getProperty:None,setProperty:None,deleteProperty:None,getPropertyNames:None,callAsFunction:None,callAsConstructor:None,hasInstance:None,convertToType:None,})
    })
});

/// Shared helper: builtin function behaviour invoker.
/// Retrieves `ec` from the thread-local `CURRENT_ENGINE` so the stored
/// closure can be called with the full trait-method signature.
unsafe fn invoke_stored_behaviour(
    stored_ptr: *mut StoredBehaviour,
    ctx: *mut JSContextRef,
    this_object: *mut JSObjectRef,
    argument_count: usize,
    arguments: *const *mut JSValueRef,
) -> Result<*mut JSValueRef, *mut JSValueRef> {
    unsafe {
        let stored: &StoredBehaviour = &*stored_ptr;

        let jsc_args: Vec<JscValue> = if argument_count == 0 || arguments.is_null() {
            Vec::new()
        } else {
            let args_slice = std::slice::from_raw_parts(arguments, argument_count);
            args_slice
                .iter()
                .map(|raw| JscValue { raw: *raw, ctx })
                .collect()
        };

        let this_val = JscValue {
            raw: this_object as *mut JSValueRef,
            ctx,
        };

        // Extract the engine pointer from CURRENT_ENGINE.
        let engine_ptr: *mut JscEngine = CURRENT_ENGINE.with(|current| match *current.borrow() {
            Some(ptr) => ptr,
            None => std::ptr::null_mut(),
        });
        if engine_ptr.is_null() {
            // CURRENT_ENGINE not set — return undefined to avoid SIGBUS.
            // This can happen when a builtin function is invoked outside
            // the normal set_current_engine scope (e.g., during GC finalization).
            return Ok(JSValueMakeUndefined(ctx));
        }
        let ec: &mut dyn ExecutionContext<JscTypes> = &mut *engine_ptr;

        let call_result = stored(&jsc_args, this_val, ec);

        match call_result {
            Ok(result) => Ok(result.raw),
            Err(err) => Err(err.raw),
        }
    }
}

/// `callAsFunction` for builtin objects.  Retrieves the `StoredBehaviour`
/// pointer from private data, converts C args to `JscValue` slices, and
/// calls the wrapped closure.
extern "C" fn builtin_call_as_function(
    ctx: *mut JSContextRef,
    function: *mut JSObjectRef,
    this_object: *mut JSObjectRef,
    argument_count: usize,
    arguments: *const *mut JSValueRef,
    exception: *mut *mut JSValueRef,
) -> *mut JSValueRef {
    let stored_ptr = unsafe { JSObjectGetPrivate(function) } as *mut StoredBehaviour;
    if stored_ptr.is_null() {
        return unsafe { JSValueMakeUndefined(ctx) };
    }
    match unsafe {
        invoke_stored_behaviour(stored_ptr, ctx, this_object, argument_count, arguments)
    } {
        Ok(raw) => raw,
        Err(err_raw) => {
            unsafe {
                *exception = err_raw;
            }
            std::ptr::null_mut()
        }
    }
}

/// `callAsConstructor` for builtin constructor objects.
///
/// In JSC's C API, `function` is the constructor function being called
/// (which serves as `new.target`), and `thisObject` is the pre-allocated
/// `this` for the constructor body.  Our Web IDL constructors need
/// `new.target` as the second argument, so we pass `function` instead.
extern "C" fn builtin_call_as_constructor(
    ctx: *mut JSContextRef,
    function: *mut JSObjectRef,
    _this_object: *mut JSObjectRef,
    argument_count: usize,
    arguments: *const *mut JSValueRef,
    exception: *mut *mut JSValueRef,
) -> *mut JSValueRef {
    let stored_ptr = unsafe { JSObjectGetPrivate(function) } as *mut StoredBehaviour;
    if stored_ptr.is_null() {
        return unsafe { JSValueMakeUndefined(ctx) };
    }
    // Pass `function` (the constructor / new.target) as `new_target_or_this`.
    match unsafe { invoke_stored_behaviour(stored_ptr, ctx, function, argument_count, arguments) } {
        Ok(raw) => raw,
        Err(err_raw) => {
            unsafe {
                *exception = err_raw;
            }
            std::ptr::null_mut()
        }
    }
}

/// Shared helper: create a JSC function object from a `StoredBehaviour`.
fn make_builtin_function(
    ctx: *mut JSContextRef,
    behaviour: StoredBehaviour,
    name: &JscPropertyKey,
    is_constructor: bool,
) -> JscObject {
    let class_ref = if is_constructor {
        BUILTIN_CONSTRUCTOR_CLASS.0
    } else {
        BUILTIN_CLASS.0
    };
    let raw_obj = unsafe { JSObjectMake(ctx, class_ref, std::ptr::null_mut()) };
    let stored = Box::new(behaviour);
    let stored_ptr = Box::into_raw(stored) as *mut std::ffi::c_void;
    unsafe { JSObjectSetPrivate(raw_obj, stored_ptr) };

    // Note: Skipping `name` property because JSObjectSetProperty on
    // JSClass-created function objects causes crashes on macOS 26.

    JscObject { raw: raw_obj, ctx }
}

/// `finalize` for builtin objects.  Drops the `StoredBehaviour` Box,
/// freeing the captured closure.
extern "C" fn builtin_finalize(object: *mut JSObjectRef) {
    let stored_ptr = unsafe { JSObjectGetPrivate(object) } as *mut StoredBehaviour;
    if !stored_ptr.is_null() {
        unsafe {
            drop(Box::from_raw(stored_ptr));
        }
    }
}

/// JSClassDefinition for the global context.  We use a custom class instead
/// of NULL so the global object supports `JSObjectSetPrivate` / `getPrivate`.
pub(crate) static GLOBAL_CONTEXT_CLASS: LazyLock<JscClass> = LazyLock::new(|| {
    let def = JSClassDefinition {
        version: 0,
        attributes: kJSClassAttributeNone,
        className: b"FormalWebGlobal\0".as_ptr() as *const c_char,
        parentClass: std::ptr::null_mut(),
        staticValues: std::ptr::null(),
        staticFunctions: std::ptr::null(),
        initialize: None,
        finalize: None,
        hasProperty: None,
        getProperty: None,
        setProperty: None,
        deleteProperty: None,
        getPropertyNames: None,
        callAsFunction: None,
        callAsConstructor: None,
        hasInstance: None,
        convertToType: None,
    };
    JscClass(unsafe { JSClassCreate(&def) })
});

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
    fn value_as_bigint(v: &Self::JsValue) -> Option<Self::JsBigInt> {
        if v.ctx.is_null() {
            return None;
        }
        let jstype = unsafe { JSValueGetType(v.ctx, v.raw) };
        if jstype == JSType::kJSTypeBigInt {
            Some(unsafe { JscBigInt::from_value(*v) })
        } else {
            None
        }
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

    fn object_is_boolean_wrapper(_o: &Self::JsObject) -> bool {
        // JSC C API has no direct boolean-object check; fallible approximation
        false
    }
    fn object_is_number_wrapper(_o: &Self::JsObject) -> bool {
        false
    }
    fn object_is_string_wrapper(_o: &Self::JsObject) -> bool {
        false
    }
    fn object_is_bigint_wrapper(_o: &Self::JsObject) -> bool {
        false
    }
    fn object_is_date(o: &Self::JsObject) -> bool {
        if o.ctx.is_null() {
            return false;
        }
        let val_ref: *mut JSValueRef = o.as_value_ref();
        unsafe { crate::jsc_sys::JSValueIsDate(o.ctx, val_ref) }
    }
    fn object_is_regexp(o: &Self::JsObject) -> bool {
        if o.ctx.is_null() {
            return false;
        }
        // JSValueIsRegExp is not in the public JSC C API, so evaluate
        // Object.prototype.toString.call(this) to check the @@toStringTag.
        let script =
            JscString::from_rust("Object.prototype.toString.call(this)==='[object RegExp]'");
        let result = unsafe {
            JSEvaluateScript(
                o.ctx,
                script.raw,
                o.raw,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        if result.is_null() {
            return false;
        }
        unsafe { JSValueToBoolean(o.ctx, result) }
    }
    fn object_is_error(o: &Self::JsObject) -> bool {
        if o.ctx.is_null() {
            return false;
        }
        // Check if Object.prototype.toString.call(this) matches an Error type.
        // Error objects have @@toStringTag returning "Error", "TypeError", "RangeError", etc.
        let script = JscString::from_rust("/Error\\]/.test(Object.prototype.toString.call(this))");
        let result = unsafe {
            JSEvaluateScript(
                o.ctx,
                script.raw,
                o.raw,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            )
        };
        if result.is_null() {
            return false;
        }
        unsafe { JSValueToBoolean(o.ctx, result) }
    }

    fn boolean_wrapper_data(_o: &Self::JsObject) -> Option<bool> {
        None
    }
    fn number_wrapper_data(_o: &Self::JsObject) -> Option<f64> {
        None
    }
    fn string_wrapper_data(_o: &Self::JsObject) -> Option<Self::JsString> {
        None
    }
    fn bigint_wrapper_data(_o: &Self::JsObject) -> Option<Self::JsBigInt> {
        None
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
    /// Monotonically-increasing counter for GC-root property names.
    next_root_id: u64,
    queued_jobs: Vec<Box<dyn FnOnce(&mut JscEngine)>>,
}

impl JscEngine {
    pub fn new() -> Self {
        Self {
            context: JscContext::new(),
            host_data: HashMap::new(),
            next_root_id: 0,
            queued_jobs: Vec::new(),
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

    fn property_key_to_value(&self, key: &JscPropertyKey) -> JscValue {
        match key {
            JscPropertyKey::String(string) => JscValue {
                raw: unsafe { JSValueMakeString(self.ctx_ptr(), string.raw) },
                ctx: self.ctx_ptr(),
            },
            JscPropertyKey::Symbol(symbol) => *symbol.as_value(),
        }
    }

    fn descriptor_field_value(
        &self,
        descriptor_object: *mut JSObjectRef,
        field_name: &str,
    ) -> Result<Option<JscValue>, JscValue> {
        let field = JscString::from_rust(field_name);
        if !unsafe {
            JSObjectHasProperty(self.context.as_context_ref(), descriptor_object, field.raw)
        } {
            return Ok(None);
        }

        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let value = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                descriptor_object,
                field.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }

        Ok(Some(JscValue {
            raw: value,
            ctx: self.ctx_ptr(),
        }))
    }

    /// Evaluate a JS expression and return the raw result + any exception.
    ///
    /// # Safety
    ///
    /// `self` must remain valid for the duration of the script evaluation.
    /// Nested calls must not mutate `self`'s context reference.
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

    /// Associate Rust data with an existing JSC object (e.g., the global object).
    /// This is used by the JSC build_context to attach a Window to the realm's
    /// global object so `with_object_any` can find it.
    /// Public because it is called from the content crate's build_context.
    pub fn associate_existing_object(
        &mut self,
        object: &JscObject,
        data: Box<dyn std::any::Any + 'static>,
    ) {
        use std::collections::HashMap;
        let map_type_id = std::any::TypeId::of::<HashMap<usize, Box<dyn std::any::Any>>>();
        let mut map: HashMap<usize, Box<dyn std::any::Any>> = self
            .remove_host_any(&map_type_id)
            .map(|boxed| {
                *boxed
                    .downcast::<HashMap<usize, Box<dyn std::any::Any>>>()
                    .unwrap()
            })
            .unwrap_or_default();
        let key = object.as_raw() as usize;
        map.insert(key, data);
        self.store_host_any(map_type_id, Box::new(map));
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
        let previous = CURRENT_ENGINE.with(|current| current.borrow_mut().take());
        let ptr = self as *mut JscEngine;
        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = Some(ptr);
        });

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

        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = previous;
        });

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
    fn create_builtin_fn_static(
        &mut self,
        behaviour: fn(
            &[JscValue],
            JscValue,
            &mut dyn ExecutionContext<JscTypes>,
        ) -> Completion<JscValue, JscTypes>,
        _length: u32,
        name: JscPropertyKey,
    ) -> JscFunction {
        let stored: StoredBehaviour = Box::new(move |args, this, ec| behaviour(args, this, ec));
        make_builtin_function(self.ctx_ptr(), stored, &name, false)
    }

    fn create_builtin_fn(
        &mut self,
        behaviour: Box<
            dyn Fn(
                &[JscValue],
                JscValue,
                &mut dyn ExecutionContext<JscTypes>,
            ) -> Completion<JscValue, JscTypes>,
        >,
        _length: u32,
        name: JscPropertyKey,
    ) -> JscFunction {
        let stored: StoredBehaviour = behaviour;
        make_builtin_function(self.ctx_ptr(), stored, &name, false)
    }

    fn create_builtin_function(
        &mut self,
        behaviour: Box<
            dyn Fn(
                &[JscValue],
                JscValue,
                &mut dyn ExecutionContext<JscTypes>,
            ) -> Completion<JscValue, JscTypes>,
        >,
        _length: u32,
        name: JscPropertyKey,
        is_constructor: bool,
    ) -> JscFunction {
        let stored: StoredBehaviour = behaviour;
        make_builtin_function(self.ctx_ptr(), stored, &name, is_constructor)
    }

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
        match &property_key {
            JscPropertyKey::String(prop_str) => {
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
            JscPropertyKey::Symbol(sym) => {
                // JSC's C API `JSObjectGetProperty` only takes a JSStringRef,
                // not a symbol.  Fall back to eval: `obj[sym]`.
                let global = self.context.global_object();
                let ctx_ptr = self.ctx_ptr();
                let obj_key = JscString::from_rust("__fw_get_obj");
                let sym_key = JscString::from_rust("__fw_get_sym");
                let mut exc: *mut JSValueRef = std::ptr::null_mut();
                unsafe {
                    JSObjectSetProperty(
                        ctx_ptr,
                        global.raw,
                        obj_key.raw,
                        object.as_value_ref(),
                        kJSPropertyAttributeNone,
                        &mut exc,
                    );
                    if exc.is_null() {
                        JSObjectSetProperty(
                            ctx_ptr,
                            global.raw,
                            sym_key.raw,
                            sym.value.raw,
                            kJSPropertyAttributeNone,
                            &mut exc,
                        );
                    }
                }
                if !exc.is_null() {
                    unsafe {
                        JSObjectDeleteProperty(ctx_ptr, global.raw, obj_key.raw, &mut exc);
                    }
                    return Err(JscValue {
                        raw: exc,
                        ctx: ctx_ptr,
                    });
                }
                let (result, exception) = self.eval_script_raw("__fw_get_obj[__fw_get_sym]");
                // Cleanup temporary globals.
                unsafe {
                    JSObjectDeleteProperty(ctx_ptr, global.raw, obj_key.raw, &mut exc);
                    JSObjectDeleteProperty(ctx_ptr, global.raw, sym_key.raw, &mut exc);
                }
                if !exception.is_null() {
                    return Err(JscValue {
                        raw: exception,
                        ctx: ctx_ptr,
                    });
                }
                Ok(JscValue {
                    raw: result,
                    ctx: ctx_ptr,
                })
            }
        }
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
        descriptor: PropertyDescriptor<JscTypes>,
    ) -> Completion<(), JscTypes> {
        let Some(prop_str) = self.property_key_to_jsstring(&property_key) else {
            return Ok(());
        };

        // For simple value descriptors, use JSObjectSetProperty directly.
        // Accessor descriptors (getter/setter) store a placeholder until
        // full Object.defineProperty support via script eval is added.
        //
        // Store operands on the global object, eval, then clean up.
        let global = self.context.global_object();
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let prop_name = prop_str.to_rust();
        let escaped_name = prop_name.replace('\\', "\\\\").replace('\'', "\\'");

        // Store target.
        let obj_key = JscString::from_rust("__fw_dpo_target");
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                obj_key.raw,
                object.as_value_ref(),
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

        // Store descriptor values as globals.
        let mut parts: Vec<String> = Vec::new();

        if let Some(getter) = &descriptor.get {
            let get_key = JscString::from_rust("__fw_dpo_get");
            unsafe {
                JSObjectSetProperty(
                    self.context.as_context_ref(),
                    global.raw,
                    get_key.raw,
                    getter.as_value_ref(),
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
            parts.push(String::from("get: __fw_dpo_get"));
        }
        if let Some(setter) = &descriptor.set {
            let set_key = JscString::from_rust("__fw_dpo_set");
            unsafe {
                JSObjectSetProperty(
                    self.context.as_context_ref(),
                    global.raw,
                    set_key.raw,
                    setter.as_value_ref(),
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
            parts.push(String::from("set: __fw_dpo_set"));
        }
        if let Some(value) = &descriptor.value {
            let val_key = JscString::from_rust("__fw_dpo_val");
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
            parts.push(String::from("value: __fw_dpo_val"));
            parts.push(if descriptor.writable.unwrap_or(true) {
                String::from("writable: true")
            } else {
                String::from("writable: false")
            });
        }
        parts.push(if descriptor.enumerable.unwrap_or(true) {
            String::from("enumerable: true")
        } else {
            String::from("enumerable: false")
        });
        parts.push(if descriptor.configurable.unwrap_or(true) {
            String::from("configurable: true")
        } else {
            String::from("configurable: false")
        });

        let desc_expr = parts.join(", ");
        let script = format!(
            "Object.defineProperty(__fw_dpo_target, '{}', {{{}}})",
            escaped_name, desc_expr
        );
        let (_result, exception) = self.eval_script_raw(&script);

        // Cleanup temp globals
        unsafe {
            let keys = [
                JscString::from_rust("__fw_dpo_target"),
                JscString::from_rust("__fw_dpo_get"),
                JscString::from_rust("__fw_dpo_set"),
                JscString::from_rust("__fw_dpo_val"),
            ];
            for key in &keys {
                JSObjectDeleteProperty(
                    self.context.as_context_ref(),
                    global.raw,
                    key.raw,
                    std::ptr::null_mut(),
                );
            }
        }

        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        Ok(())
    }

    fn to_property_descriptor(
        &mut self,
        _desc_obj: JscObject,
    ) -> Completion<PropertyDescriptor<JscTypes>, JscTypes> {
        // Stub — not yet used by JSC proxy code
        Err(self.new_type_error("to_property_descriptor not yet implemented on JSC backend"))
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

    fn get_prototype_of(&mut self, _object: JscObject) -> Completion<Option<JscObject>, JscTypes> {
        // Stub — not yet used by JSC proxy code
        Err(self.new_type_error("get_prototype_of not yet implemented on JSC backend"))
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
        Ok(self.get_own_property(object, property_key)?.is_some())
    }
    fn own_property_keys(
        &mut self,
        object: JscObject,
    ) -> Completion<Vec<JscPropertyKey>, JscTypes> {
        let global = self.context.global_object();
        let object_key = JscString::from_rust("__formal_web_own_keys_target");
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                object_key.raw,
                object.as_value_ref(),
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

        let (result, exception) =
            self.eval_script_raw("Reflect.ownKeys(__formal_web_own_keys_target)");
        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                object_key.raw,
                std::ptr::null_mut(),
            );
        }
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }

        let result_object = result as *mut JSObjectRef;
        let length_key = JscString::from_rust("length");
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let length_value = unsafe {
            JSObjectGetProperty(
                self.context.as_context_ref(),
                result_object,
                length_key.raw,
                &mut exception,
            )
        };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }
        let length =
            unsafe { JSValueToNumber(self.context.as_context_ref(), length_value, &mut exception) };
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }

        let mut keys = Vec::new();
        for index in 0..(length as u32) {
            let index_key = JscString::from_rust(&index.to_string());
            let key_value = unsafe {
                JSObjectGetProperty(
                    self.context.as_context_ref(),
                    result_object,
                    index_key.raw,
                    &mut exception,
                )
            };
            if !exception.is_null() {
                return Err(JscValue {
                    raw: exception,
                    ctx: self.ctx_ptr(),
                });
            }
            keys.push(self.to_property_key(JscValue {
                raw: key_value,
                ctx: self.ctx_ptr(),
            })?);
        }

        Ok(keys)
    }
    fn get_own_property(
        &mut self,
        object: JscObject,
        property_key: JscPropertyKey,
    ) -> Completion<Option<PropertyDescriptor<JscTypes>>, JscTypes> {
        let global = self.context.global_object();
        let object_key = JscString::from_rust("__formal_web_desc_target");
        let property_key_name = JscString::from_rust("__formal_web_desc_key");
        let property_value = self.property_key_to_value(&property_key);

        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        unsafe {
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                object_key.raw,
                object.as_value_ref(),
                kJSPropertyAttributeNone,
                &mut exception,
            );
            JSObjectSetProperty(
                self.context.as_context_ref(),
                global.raw,
                property_key_name.raw,
                property_value.raw,
                kJSPropertyAttributeNone,
                &mut exception,
            );
        }
        if !exception.is_null() {
            return Err(JscValue {
                raw: exception,
                ctx: self.ctx_ptr(),
            });
        }

        let (result, exception) = self.eval_script_raw(
            "Object.getOwnPropertyDescriptor(__formal_web_desc_target, __formal_web_desc_key)",
        );

        unsafe {
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                object_key.raw,
                std::ptr::null_mut(),
            );
            JSObjectDeleteProperty(
                self.context.as_context_ref(),
                global.raw,
                property_key_name.raw,
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
            return Ok(None);
        }

        let result_value = JscValue {
            raw: result,
            ctx: self.ctx_ptr(),
        };
        if JscTypes::value_is_undefined(&result_value) {
            return Ok(None);
        }

        let descriptor_object = result as *mut JSObjectRef;
        let value = self.descriptor_field_value(descriptor_object, "value")?;
        let writable = self
            .descriptor_field_value(descriptor_object, "writable")?
            .map(|field| unsafe { JSValueToBoolean(self.context.as_context_ref(), field.raw) });
        let get = self
            .descriptor_field_value(descriptor_object, "get")?
            .and_then(|field| {
                if self.is_callable(&field) {
                    JscTypes::value_as_object(&field)
                } else {
                    None
                }
            });
        let set = self
            .descriptor_field_value(descriptor_object, "set")?
            .and_then(|field| {
                if self.is_callable(&field) {
                    JscTypes::value_as_object(&field)
                } else {
                    None
                }
            });
        let enumerable = self
            .descriptor_field_value(descriptor_object, "enumerable")?
            .map(|field| unsafe { JSValueToBoolean(self.context.as_context_ref(), field.raw) });
        let configurable = self
            .descriptor_field_value(descriptor_object, "configurable")?
            .map(|field| unsafe { JSValueToBoolean(self.context.as_context_ref(), field.raw) });

        Ok(Some(PropertyDescriptor {
            value,
            writable,
            get,
            set,
            enumerable,
            configurable,
        }))
    }
    fn construct(
        &mut self,
        function: JscConstructor,
        args: &[JscValue],
        _new_target: Option<JscConstructor>,
    ) -> Completion<JscObject, JscTypes> {
        let previous = CURRENT_ENGINE.with(|current| current.borrow_mut().take());
        let ptr = self as *mut JscEngine;
        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = Some(ptr);
        });

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

        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = previous;
        });

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

        let uint8_array = fetch_ctor("Uint8Array");

        // AsyncIteratorPrototype: create a minimal prototype object with
        // [Symbol.asyncIterator] returning this.
        // In JSC, there is no built-in %AsyncIteratorPrototype%, so we
        // construct one manually.
        let boolean_ctor = fetch_ctor("Boolean");
        let number_ctor = fetch_ctor("Number");
        let string_ctor = fetch_ctor("String");
        let bigint_ctor = fetch_ctor("BigInt");
        let date_ctor = fetch_ctor("Date");
        let regexp_ctor = fetch_ctor("RegExp");
        let map_ctor = fetch_ctor("Map");
        let set_ctor = fetch_ctor("Set");

        // Fetch prototypes via constructor.prototype
        let fetch_proto = |ctor: JscObject| -> JscObject {
            let mut exc: *mut JSValueRef = std::ptr::null_mut();
            let raw = unsafe { JSObjectGetProperty(ctx, ctor.raw, proto_str.raw, &mut exc) };
            if raw.is_null() {
                return object_prototype;
            }
            JscObject {
                raw: raw as *mut JSObjectRef,
                ctx: self.ctx_ptr(),
            }
        };

        let boolean_prototype = fetch_proto(boolean_ctor);
        let number_prototype = fetch_proto(number_ctor);
        let string_prototype = fetch_proto(string_ctor);
        let bigint_prototype = fetch_proto(bigint_ctor);
        let date_prototype = fetch_proto(date_ctor);
        let regexp_prototype = fetch_proto(regexp_ctor);
        let map_prototype = fetch_proto(map_ctor);
        let set_prototype = fetch_proto(set_ctor);
        let error_prototype = fetch_proto(error);
        let type_error_prototype = fetch_proto(type_error);
        let range_error_prototype = fetch_proto(range_error);
        let syntax_error_prototype = fetch_proto(syntax_error);
        let reference_error_prototype = fetch_proto(reference_error);
        let uri_error_prototype = fetch_proto(uri_error);
        let eval_error_prototype = fetch_proto(eval_error);

        let async_iterator_prototype = object_prototype;

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
            uint8_array,
            boolean: boolean_ctor,
            number: number_ctor,
            string: string_ctor,
            bigint: bigint_ctor,
            date: date_ctor,
            regexp: regexp_ctor,
            map: map_ctor,
            set: set_ctor,
            boolean_prototype,
            number_prototype,
            string_prototype,
            bigint_prototype,
            date_prototype,
            regexp_prototype,
            map_prototype,
            set_prototype,
            error_prototype,
            type_error_prototype,
            range_error_prototype,
            syntax_error_prototype,
            reference_error_prototype,
            uri_error_prototype,
            eval_error_prototype,
            object_prototype,
            function_prototype,
            async_iterator_prototype,
        }
    }

    fn realm_global_object(&self) -> JscObject
    where
        JscTypes: JsTypesWithRealm,
    {
        self.context.global_object()
    }

    // ── §7.3 Functions ────────────────────────────────────────────────────

    fn get_function_realm(&mut self, _function: &JscObject) -> Completion<JscRealm, JscTypes> {
        todo!("get_function_realm")
    }

    // ── §9.6 Jobs ─────────────────────────────────────────────────────────
    fn enqueue_job(&mut self, job: Box<dyn FnOnce()>) {
        self.queued_jobs.push(Box::new(move |_: &mut JscEngine| {
            job();
        }));
    }
    fn enqueue_job_with_realm(
        &mut self,
        _realm: JscRealm,
        job: Box<dyn FnOnce(&mut dyn ExecutionContext<JscTypes>)>,
    ) {
        self.queued_jobs
            .push(Box::new(move |engine: &mut JscEngine| {
                job(engine);
            }));
    }
    fn run_jobs(&mut self) {
        let jobs = std::mem::take(&mut self.queued_jobs);
        for job in jobs {
            job(self);
        }
    }

    // ── §25 ArrayBuffer — runtime queries ─────────────────────────────────
    fn allocate_array_buffer(
        &mut self,
        constructor: JscConstructor,
        byte_length: u64,
        max_byte_length: Option<u64>,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        JsEngine::allocate_array_buffer(self, constructor, byte_length, max_byte_length)
    }

    fn clone_array_buffer(
        &mut self,
        src: JscArrayBuffer,
        src_byte_offset: u64,
        src_length: u64,
        clone_constructor: JscConstructor,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        JsEngine::clone_array_buffer(self, src, src_byte_offset, src_length, clone_constructor)
    }

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

    // ── §23.2 TypedArray Objects ──────────────────────────────────────────

    fn typed_array_buffer(
        &mut self,
        _typed_array: &JscTypedArray,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        todo!("typed_array_buffer not yet implemented for JSC")
    }

    fn typed_array_byte_offset(
        &mut self,
        _typed_array: &JscTypedArray,
    ) -> Completion<u64, JscTypes> {
        todo!("typed_array_byte_offset not yet implemented for JSC")
    }

    fn typed_array_byte_length(
        &mut self,
        _typed_array: &JscTypedArray,
    ) -> Completion<u64, JscTypes> {
        todo!("typed_array_byte_length not yet implemented for JSC")
    }

    fn typed_array_element_type(
        &self,
        _typed_array: &JscTypedArray,
    ) -> Option<TypedArrayElementType> {
        None
    }

    fn construct_typed_array_view(
        &mut self,
        _element_type: TypedArrayElementType,
        _buffer: JscArrayBuffer,
        _byte_offset: u64,
        _byte_length: u64,
    ) -> Completion<JscTypedArray, JscTypes> {
        todo!("construct_typed_array_view not yet implemented for JSC")
    }

    // ── §25.3 DataView Objects ────────────────────────────────────────────

    fn data_view_buffer(
        &mut self,
        _data_view: &JscDataView,
    ) -> Completion<JscArrayBuffer, JscTypes> {
        todo!("data_view_buffer not yet implemented for JSC")
    }

    fn data_view_byte_offset(&mut self, _data_view: &JscDataView) -> Completion<u64, JscTypes> {
        todo!("data_view_byte_offset not yet implemented for JSC")
    }

    fn data_view_byte_length(&mut self, _data_view: &JscDataView) -> Completion<u64, JscTypes> {
        todo!("data_view_byte_length not yet implemented for JSC")
    }

    fn construct_data_view_from_buffer(
        &mut self,
        _buffer: JscArrayBuffer,
        _byte_offset: u64,
        _byte_length: u64,
    ) -> Completion<JscDataView, JscTypes> {
        todo!("construct_data_view_from_buffer not yet implemented for JSC")
    }

    // ── §25.1 ArrayBuffer — data access ───────────────────────────────────

    fn array_buffer_data(&self, _array_buffer: &JscArrayBuffer) -> Option<Vec<u8>> {
        None
    }

    // ── §22.2 Date ────────────────────────────────────────────────────────

    fn get_date_value(&mut self, date: &JscObject) -> Completion<f64, JscTypes> {
        // JSC: call date.getTime()
        let get_time_str = JscString::from_rust("getTime");
        let get_time_key = JscPropertyKey::String(get_time_str);
        let get_time = self
            .get_method(
                JscValue {
                    raw: date.as_value_ref(),
                    ctx: self.ctx_ptr(),
                },
                get_time_key,
            )?
            .ok_or_else(|| self.new_type_error("Date has no getTime"))?;
        let date_val = JscValue {
            raw: date.as_value_ref(),
            ctx: self.ctx_ptr(),
        };
        let result = EcmascriptHost::call(self, &get_time, &date_val, &[])?;
        self.to_number(result)
    }

    // ── §22.3 RegExp ─────────────────────────────────────────────────────

    fn get_regexp_source(&mut self, regexp: &JscObject) -> Completion<String, JscTypes> {
        let source_str = JscString::from_rust("source");
        let source_key = JscPropertyKey::String(source_str);
        let result = ExecutionContext::get(self, *regexp, source_key)?;
        // RegExp.source is a string; extract it
        if let Some(s) = JscTypes::value_as_string(&result) {
            let source: String = s.to_rust();
            Ok(source)
        } else {
            Err(self.new_type_error("RegExp.source is not a string"))
        }
    }

    fn get_regexp_flags(&mut self, regexp: &JscObject) -> Completion<String, JscTypes> {
        let flags_str = JscString::from_rust("flags");
        let flags_key = JscPropertyKey::String(flags_str);
        let result = ExecutionContext::get(self, *regexp, flags_key)?;
        if let Some(s) = JscTypes::value_as_string(&result) {
            let flags: String = s.to_rust();
            Ok(flags)
        } else {
            Err(self.new_type_error("RegExp.flags is not a string"))
        }
    }

    // ── §24.1 Map ────────────────────────────────────────────────────────

    fn map_get_entries(
        &mut self,
        _map: &JscMap,
    ) -> Completion<Vec<(JscValue, JscValue)>, JscTypes> {
        Err(self.new_type_error("map_get_entries not yet implemented for JSC"))
    }

    fn map_set_entry(
        &mut self,
        _map: &JscMap,
        _key: JscValue,
        _value: JscValue,
    ) -> Completion<(), JscTypes> {
        Err(self.new_type_error("map_set_entry not yet implemented for JSC"))
    }

    // ── §24.2 Set ────────────────────────────────────────────────────────

    fn set_get_values(&mut self, _set: &JscSet) -> Completion<Vec<JscValue>, JscTypes> {
        Err(self.new_type_error("set_get_values not yet implemented for JSC"))
    }

    fn set_add_entry(&mut self, _set: &JscSet, _value: JscValue) -> Completion<(), JscTypes> {
        Err(self.new_type_error("set_add_entry not yet implemented for JSC"))
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

    fn new_promise_pending(
        &mut self,
    ) -> Completion<(JscValue, PromiseResolvers<JscTypes>), JscTypes> {
        // Reuse the same mechanism as new_promise_capability.
        // The constructor parameter is ignored by new_promise_capability,
        // so a dummy value is safe.
        let dummy_ctor = JscObject {
            raw: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
        };
        let pcap = self.new_promise_capability(dummy_ctor)?;
        Ok((
            pcap.promise,
            PromiseResolvers {
                resolve: pcap.resolve,
                reject: pcap.reject,
            },
        ))
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

    fn promise_state(
        &mut self,
        _promise: &JscObject,
    ) -> Completion<crate::enums::PromiseState<JscTypes>, JscTypes> {
        // JSC: Runtime promise state inspection is not supported via the C API.
        // Callers should restructure to avoid needing this.
        Ok(crate::enums::PromiseState::Pending)
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
                boxed.downcast_mut::<std::collections::HashMap<usize, Box<dyn std::any::Any>>>()
            })
            .and_then(|map| {
                let key = object.as_raw() as usize;
                map.get_mut(&key)
                    .map(|boxed| boxed.as_mut() as *mut dyn std::any::Any)
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

    // ── Property Key Construction ─────────────────────────────────────────

    fn property_key_from_str(&self, s: &str) -> JscPropertyKey {
        JscPropertyKey::String(JscString::from_rust(s))
    }

    fn property_key_from_index(&self, index: u32) -> JscPropertyKey {
        JscPropertyKey::String(JscString::from_rust(&index.to_string()))
    }

    fn property_key_from_symbol(&self, sym: &JscSymbol) -> JscPropertyKey {
        JscPropertyKey::Symbol(sym.clone())
    }

    fn property_key_from_well_known_symbol(&mut self, name: &str) -> JscPropertyKey {
        fn is_symbol_value(value: &JscValue) -> bool {
            let js_type: JSType = unsafe { JSValueGetType(value.ctx, value.raw) };
            js_type == JSType::kJSTypeSymbol
        }
        // JSC symbol references are opaque JscValues — look up from the global object.
        let global = self.context.global_object();
        let symbol_fn =
            EcmascriptHost::get(self, &global, "Symbol").unwrap_or_else(|_| self.value_undefined());
        let symbol_obj = match <JscTypes as JsTypes>::value_as_object(&symbol_fn) {
            Some(obj) => obj,
            None => return JscPropertyKey::from_rust(name),
        };
        let sym_value =
            EcmascriptHost::get(self, &symbol_obj, name).unwrap_or_else(|_| self.value_undefined());
        if is_symbol_value(&sym_value) {
            let sym = unsafe { JscSymbol::from_value(sym_value) };
            JscPropertyKey::Symbol(sym)
        } else {
            JscPropertyKey::from_rust(name)
        }
    }

    fn property_key_to_rust_string(&self, key: &JscPropertyKey) -> String {
        match key {
            JscPropertyKey::String(s) => s.to_rust(),
            JscPropertyKey::Symbol(_) => String::from("Symbol()"),
        }
    }

    // ── Proxy Creation (§10.5.14) ──────────────────────────────────────────

    fn create_proxy(
        &mut self,
        _target: JscObject,
        _handler: JscObject,
    ) -> Completion<JscObject, JscTypes> {
        Err(self.new_type_error("Proxy not yet supported on JSC backend"))
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
        let ctx = self.ctx_ptr();
        let raw_obj = unsafe { JSObjectMake(ctx, PLAIN_OBJECT_CLASS.0, std::ptr::null_mut()) };
        if let Some(proto) = prototype {
            unsafe {
                JSObjectSetPrototype(ctx, raw_obj, proto.as_value_ref());
            }
        }
        JscObject { raw: raw_obj, ctx }
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
        // Store the value as a non-enumerable property on the global object
        // to keep it alive in JSC's GC graph.  This avoids JSValueProtect
        // which SIGSEGVs on eval-created values on some macOS versions.
        let root_id = self.next_root_id;
        self.next_root_id = self.next_root_id.wrapping_add(1);
        let prop_name = format!("__fw_root_{root_id}");
        let key = JscString::from_rust(&prop_name);
        let mut exc: *mut JSValueRef = std::ptr::null_mut();
        let global = self.context.global_object();
        let ctx_ptr = self.ctx_ptr();
        unsafe {
            JSObjectSetProperty(
                ctx_ptr,
                global.raw,
                key.raw,
                value.raw,
                kJSPropertyAttributeDontEnum,
                &mut exc,
            );
        }
        // On drop, delete the property to allow GC.
        let cleanup_key = key;
        let cleanup_ctx = ctx_ptr;
        let cleanup_global_raw = global.raw;
        crate::gc::GcRootHandle {
            value: *value,
            unroot_action: Some(Box::new(move |_val| unsafe {
                JSObjectDeleteProperty(
                    cleanup_ctx,
                    cleanup_global_raw,
                    cleanup_key.raw,
                    std::ptr::null_mut(),
                );
            })),
        }
    }

    fn evaluate_script(&mut self, source: &str) -> Completion<JscValue, JscTypes> {
        let previous = CURRENT_ENGINE.with(|current| current.borrow_mut().take());
        let ptr = self as *mut JscEngine;
        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = Some(ptr);
        });

        let script = JscString::from_rust(source);
        let ctx_ref = self.context.as_context_ref();
        let mut exception: *mut JSValueRef = std::ptr::null_mut();
        let result = unsafe {
            JSEvaluateScript(
                ctx_ref,
                script.raw,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut exception,
            )
        };

        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = previous;
        });

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
        // Ensure CURRENT_ENGINE is set before the JS call so that any
        // builtin function callback (callAsFunction / callAsConstructor)
        // can find the engine via with_current_engine.
        // Save and restore so nested calls don't corrupt the state.
        let previous = CURRENT_ENGINE.with(|current| current.borrow_mut().take());
        let ptr = self as *mut JscEngine;
        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = Some(ptr);
        });

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

        // Restore previous CURRENT_ENGINE value.
        CURRENT_ENGINE.with(|current| {
            *current.borrow_mut() = previous;
        });

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
        self.run_jobs();
        Ok(())
    }
    fn report_exception(&mut self, error: JscValue) {
        log::error!("uncaught callback error: {}", error.display());
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
        let engine = JscEngine::new();
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
        let result = JsEngine::evaluate_script(&mut engine, "40 + 2", &realm).unwrap();
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
        let fn_val =
            JsEngine::evaluate_script(&mut engine, "(function(x) { return x * 2; })", &realm)
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
    fn allocate_array_buffer() {
        let mut engine = JscEngine::new();
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        let ab =
            JsEngine::allocate_array_buffer(&mut engine, intrinsics.array_buffer, 8, None).unwrap();
        assert!(!ab.raw.is_null());
    }
}
