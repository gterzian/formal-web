//! Safe Rust wrappers around JavaScriptCore C API types.
//!
//! These wrapper types encapsulate the raw FFI pointers from the `sys`
//! submodule and provide a safe, Rustic API for JS value manipulation.
//!
//! # Safety model
//!
//! - `JscEngine` owns a `JSGlobalContextRef` and all values derived from it.
//! - Values (`JscValue`, `JscObject`, `JscString`) hold raw pointers; the
//!   caller must ensure the engine outlives them.
//! - JSC uses garbage collection; values are retained by the engine's roots.
//!   Strings (`JSStringRef`) follow the Create Rule and are wrapped in RAII.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::gc::Trace;
use crate::jsc_sys::*;

// ── JscContext (owned) ────────────────────────────────────────────────────

/// RAII wrapper for a `JSGlobalContextRef`.
pub struct JscContext {
    pub(crate) raw: *mut JSGlobalContextRef,
}

unsafe impl Send for JscContext {}
unsafe impl Sync for JscContext {}

impl JscContext {
    pub fn new() -> Self {
        let raw = unsafe { JSGlobalContextCreate(super::engine::GLOBAL_CONTEXT_CLASS.0) };
        assert!(!raw.is_null(), "JSGlobalContextCreate returned null");
        Self { raw }
    }

    pub fn as_context_ref(&self) -> *mut JSContextRef {
        self.raw as *mut JSContextRef
    }

    pub fn global_object(&self) -> JscObject {
        let ctx_ptr = self.as_context_ref();
        let raw = unsafe { JSContextGetGlobalObject(ctx_ptr) };
        assert!(!raw.is_null());
        JscObject { raw, ctx: ctx_ptr }
    }
}

impl Clone for JscContext {
    fn clone(&self) -> Self {
        let raw = unsafe { JSGlobalContextRetain(self.raw) };
        Self { raw }
    }
}

impl Default for JscContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for JscContext {
    fn drop(&mut self) {
        unsafe { JSGlobalContextRelease(self.raw) }
    }
}

// ── JscString (owned) ─────────────────────────────────────────────────────

/// RAII wrapper for a `JSStringRef`.
pub struct JscString {
    pub(crate) raw: *mut JSStringRef,
}

impl JscString {
    pub fn from_rust(s: &str) -> Self {
        // JSC's C API expects null-terminated strings without embedded null
        // bytes. If the Rust string contains null bytes (e.g. malformed
        // data from structured clone or error messages), replace them with
        // the Unicode replacement character instead of panicking.
        let c_str = match CString::new(s) {
            Ok(c_str) => c_str,
            Err(error) => {
                log::warn!(
                    "JscString::from_rust got string with null byte at position {}; replacing",
                    error.nul_position(),
                );
                let sanitized: String = s
                    .chars()
                    .map(|c| if c == '\0' { '\u{FFFD}' } else { c })
                    .collect();
                CString::new(sanitized).expect("sanitized string should not contain null bytes")
            }
        };
        let raw = unsafe { JSStringCreateWithUTF8CString(c_str.as_ptr()) };
        assert!(!raw.is_null());
        Self { raw }
    }

    /// # Safety
    /// `raw` must be a valid `JSStringRef` obtained from a JSC API function
    /// that returns ownership to the caller.
    pub unsafe fn from_raw(raw: *mut JSStringRef) -> Self {
        Self { raw }
    }

    pub fn as_raw(&self) -> *mut JSStringRef {
        self.raw
    }

    pub fn to_rust(&self) -> String {
        let max_size = unsafe { JSStringGetMaximumUTF8CStringSize(self.raw) };
        let mut buffer = vec![0u8; max_size];
        let count = unsafe {
            JSStringGetUTF8CString(self.raw, buffer.as_mut_ptr() as *mut c_char, max_size)
        };
        buffer.truncate(count.saturating_sub(1));
        String::from_utf8_lossy(&buffer).to_string()
    }

    pub fn len(&self) -> usize {
        unsafe { JSStringGetLength(self.raw) }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Clone for JscString {
    fn clone(&self) -> Self {
        Self {
            raw: unsafe { JSStringRetain(self.raw) },
        }
    }
}

impl Drop for JscString {
    fn drop(&mut self) {
        unsafe { JSStringRelease(self.raw) }
    }
}

impl std::fmt::Debug for JscString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JscString({:?})", self.to_rust())
    }
}

impl PartialEq for JscString {
    fn eq(&self, other: &Self) -> bool {
        unsafe { JSStringIsEqual(self.raw, other.raw) }
    }
}
impl Eq for JscString {}

impl PartialEq<str> for JscString {
    fn eq(&self, other: &str) -> bool {
        self.to_rust() == other
    }
}

impl PartialEq<&str> for JscString {
    fn eq(&self, other: &&str) -> bool {
        self.to_rust() == *other
    }
}

impl std::hash::Hash for JscString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.to_rust().hash(state);
    }
}

// ── JscValue (borrowed from the engine) ───────────────────────────────────

/// A JavaScript value — wraps a `JSValueRef`.
///
/// Valid only while the creating `JscEngine` exists.
/// Contains a context pointer for type queries (JSC requires context for
/// `JSValueGetType`, `JSValueIsString`, etc.).
#[derive(Clone, Copy)]
pub struct JscValue {
    pub(crate) raw: *mut JSValueRef,
    pub(crate) ctx: *mut JSContextRef,
}

impl std::fmt::Debug for JscValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JscValue").field("raw", &self.raw).finish()
    }
}

impl PartialEq for JscValue {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl Eq for JscValue {}

// SAFETY: JSC garbage collection handles JS values natively — no Rust
// tracing is needed.  The marker impl is required for trait bounds on
// capture types passed to `builtin_with_captures`.
#[cfg(not(feature = "boa"))]
unsafe impl Trace for JscValue {}

impl Default for JscValue {
    fn default() -> Self {
        JscValue {
            raw: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
        }
    }
}

impl From<bool> for JscValue {
    fn from(b: bool) -> Self {
        // Look up the current engine's context from the thread-local
        // (set by EngineGuard at every ExecutionContext entry point).
        let ctx = super::engine::current_engine_context();
        JscValue {
            raw: unsafe { JSValueMakeBoolean(ctx, b) },
            ctx,
        }
    }
}

impl From<f64> for JscValue {
    fn from(n: f64) -> Self {
        let ctx = super::engine::current_engine_context();
        JscValue {
            raw: unsafe { JSValueMakeNumber(ctx, n) },
            ctx,
        }
    }
}

impl From<JscObject> for JscValue {
    fn from(obj: JscObject) -> Self {
        Self {
            raw: obj.raw as *mut JSValueRef,
            ctx: obj.ctx,
        }
    }
}

/// Converting a `JscPropertyKey` to a `JscValue` may require a context
/// (for creating a JSString from the key).  This is a best-effort impl;
/// prefer keeping the key type separate.
impl From<JscPropertyKey> for JscValue {
    fn from(_key: JscPropertyKey) -> Self {
        panic!(
            "Cannot create JscValue from JscPropertyKey without a context; use ec.property_key_to_value()"
        )
    }
}

impl JscValue {
    /// # Safety
    /// `raw` must be a valid `JSValueRef` from `ctx`.
    pub unsafe fn from_raw(raw: *mut JSValueRef, ctx: *mut JSContextRef) -> Self {
        Self { raw, ctx }
    }
    pub fn as_raw(&self) -> *mut JSValueRef {
        self.raw
    }
    pub fn ctx(&self) -> *mut JSContextRef {
        self.ctx
    }

    pub fn get_type(&self) -> JSType {
        if self.ctx.is_null() {
            return JSType::kJSTypeUndefined;
        }
        unsafe { JSValueGetType(self.ctx, self.raw) }
    }

    pub fn to_bool(&self) -> bool {
        if self.ctx.is_null() {
            return false;
        }
        unsafe { JSValueToBoolean(self.ctx, self.raw) }
    }

    /// Returns `true` if the value is `undefined`.
    pub fn is_undefined(&self) -> bool {
        if self.ctx.is_null() {
            return true;
        }
        unsafe { JSValueGetType(self.ctx, self.raw) == JSType::kJSTypeUndefined }
    }

    /// Returns `true` if the value is `null`.
    pub fn is_null(&self) -> bool {
        if self.ctx.is_null() {
            return true;
        }
        unsafe { JSValueGetType(self.ctx, self.raw) == JSType::kJSTypeNull }
    }

    /// If the value is an object, returns the underlying `JscObject`.
    pub fn as_object(&self) -> Option<JscObject> {
        if self.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueGetType(self.ctx, self.raw) == JSType::kJSTypeObject } {
            Some(JscObject {
                raw: self.raw as *mut JSObjectRef,
                ctx: self.ctx,
            })
        } else {
            None
        }
    }

    /// Creates an `undefined` value for the given context.
    pub fn undefined(ctx: &JscContext) -> Self {
        let ctx_ptr = ctx.as_context_ref();
        Self {
            raw: unsafe { JSValueMakeUndefined(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }

    /// Creates a `null` value for the given context.
    pub fn null(ctx: &JscContext) -> Self {
        let ctx_ptr = ctx.as_context_ref();
        Self {
            raw: unsafe { JSValueMakeNull(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }

    /// If the value is a string, returns the underlying `JscString`.
    pub fn as_string(&self) -> Option<JscString> {
        if self.ctx.is_null() {
            return None;
        }
        if unsafe { JSValueIsString(self.ctx, self.raw) } {
            let mut exc: *mut JSValueRef = std::ptr::null_mut();
            let raw = unsafe { JSValueToStringCopy(self.ctx, self.raw, &mut exc) };
            if !exc.is_null() || raw.is_null() {
                return None;
            }
            Some(unsafe { JscString::from_raw(raw) })
        } else {
            None
        }
    }

    /// Display the value as a string (for error messages / debugging).
    pub fn display(&self) -> String {
        if self.ctx.is_null() {
            return String::from("undefined");
        }
        let js_str = unsafe { JSValueToStringCopy(self.ctx, self.raw, std::ptr::null_mut()) };
        if js_str.is_null() {
            return String::from("<error>");
        }
        let len = unsafe { JSStringGetLength(js_str) } as usize;
        let max_len = len * 3 + 1;
        let mut buf = vec![0i8; max_len];
        let written = unsafe { JSStringGetUTF8CString(js_str, buf.as_mut_ptr(), max_len) };
        let result = if written > 0 {
            let slice = &buf[..(written as usize - 1)];
            String::from_utf8_lossy(unsafe { std::mem::transmute::<&[i8], &[u8]>(slice) })
                .into_owned()
        } else {
            String::from("<conversion error>")
        };
        unsafe { JSStringRelease(js_str) };
        result
    }
}

/// The undefined value for a given context.
pub struct JscUndefined;
impl JscUndefined {
    pub fn get(ctx: &JscContext) -> JscValue {
        let ctx_ptr = ctx.as_context_ref();
        JscValue {
            raw: unsafe { JSValueMakeUndefined(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }
}

/// The null value for a given context.
pub struct JscNull;
impl JscNull {
    pub fn get(ctx: &JscContext) -> JscValue {
        let ctx_ptr = ctx.as_context_ref();
        JscValue {
            raw: unsafe { JSValueMakeNull(ctx_ptr) },
            ctx: ctx_ptr,
        }
    }
}

// ── JscObject (borrowed) ──────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct JscObject {
    pub(crate) raw: *mut JSObjectRef,
    pub(crate) ctx: *mut JSContextRef,
}

impl PartialEq for JscObject {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw && self.ctx == other.ctx
    }
}
impl Eq for JscObject {}

impl JscObject {
    /// # Safety
    /// `raw` must be a valid `JSObjectRef` from `ctx`.
    pub unsafe fn from_raw(raw: *mut JSObjectRef, ctx: *mut JSContextRef) -> Self {
        Self { raw, ctx }
    }
    pub fn as_raw(&self) -> *mut JSObjectRef {
        self.raw
    }
    pub fn ctx(&self) -> *mut JSContextRef {
        self.ctx
    }
    pub fn as_value_ref(&self) -> *mut JSValueRef {
        self.raw as *mut JSValueRef
    }
    pub fn as_value(&self) -> JscValue {
        JscValue {
            raw: self.as_value_ref(),
            ctx: self.ctx,
        }
    }

    pub fn is_callable(&self) -> bool {
        if self.ctx.is_null() {
            return false;
        }
        unsafe { JSObjectIsFunction(self.ctx, self.raw) }
    }

    pub fn is_constructor(&self) -> bool {
        if self.ctx.is_null() {
            return false;
        }
        unsafe { JSObjectIsConstructor(self.ctx, self.raw) }
    }

    // ── Generic downcast helpers (compatible with Boa's JsObject methods) ──

    /// Downcast the object's native data to a concrete type `T`.
    ///
    /// For JSC, data is stored in a host-side HashMap keyed by object pointer
    /// (via `create_object_with_any` / `with_object_any`).  This method exists
    /// for parity with Boa's `JsObject::downcast_ref`.  Prefer using
    /// `ec.with_object_any(&obj)` directly.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        None
    }

    /// Downcast the object's native data to a mutable concrete type `T`.
    ///
    /// Prefer using `ec.with_object_any_mut(&obj)` directly.
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        None
    }

    /// Get the `ArrayBuffer`'s backing data as a byte slice.
    ///
    /// Falls back to `None` since this is not directly supported via JSC's
    /// public C API.  Use `JSObjectGetArrayBufferBytesPtr` on newer macOS.
    pub fn data(&self) -> Option<&[u8]> {
        None
    }

    /// Get the `ArrayBuffer`'s backing data as a mutable byte slice.
    pub fn data_mut(&self) -> Option<&mut [u8]> {
        None
    }
}

// ── JscSymbol (borrowed) ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JscSymbol {
    pub(crate) value: JscValue,
}

impl JscSymbol {
    /// # Safety
    /// The value must have type `kJSTypeSymbol`.
    pub unsafe fn from_value(value: JscValue) -> Self {
        Self { value }
    }
    pub fn as_value(&self) -> &JscValue {
        &self.value
    }
}

// ── JscBigInt (borrowed) ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JscBigInt {
    pub(crate) value: JscValue,
}

impl JscBigInt {
    /// # Safety
    /// The value must have type `kJSTypeBigInt`.
    pub unsafe fn from_value(value: JscValue) -> Self {
        Self { value }
    }
    pub fn as_value(&self) -> &JscValue {
        &self.value
    }
}

// ── JscPropertyKey ────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum JscPropertyKey {
    String(JscString),
    Symbol(JscSymbol),
}

impl JscPropertyKey {
    pub fn from_rust(s: &str) -> Self {
        JscPropertyKey::String(JscString::from_rust(s))
    }
}

// ── Type aliases (all JscObject in the C API) ─────────────────────────────

pub type JscArrayBuffer = JscObject;
pub type JscSharedArrayBuffer = JscObject;
pub type JscTypedArray = JscObject;
pub type JscDataView = JscObject;
pub type JscPromise = JscObject;
pub type JscMap = JscObject;
pub type JscSet = JscObject;
pub type JscWeakMap = JscObject;
pub type JscWeakSet = JscObject;
pub type JscWeakRef = JscObject;
pub type JscGenerator = JscObject;
pub type JscAsyncGenerator = JscObject;
pub type JscFunction = JscObject;
pub type JscConstructor = JscObject;

/// JSC's global context serves as the realm.
#[derive(Clone, Copy)]
pub struct JscRealm {
    pub(crate) raw: *mut JSGlobalContextRef,
}

impl JscRealm {
    /// # Safety
    /// `raw` must be a valid `JSGlobalContextRef`.
    pub unsafe fn from_raw(raw: *mut JSGlobalContextRef) -> Self {
        Self { raw }
    }
    pub fn as_raw(&self) -> *mut JSGlobalContextRef {
        self.raw
    }
}
