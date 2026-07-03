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
        let c_str = CString::new(s).expect("JSString contains null byte");
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
