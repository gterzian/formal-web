//! Raw FFI bindings to Apple's JavaScriptCore framework.
//!
//! These are direct `unsafe extern "C"` function declarations for the C API defined in
//! `<JavaScriptCore/JS*.h>`.  All names match the framework headers exactly.
//!
//! # Safety
//!
//! All functions are `unsafe`.  Callers must pass valid pointers and respect
//! the Create Rule (functions containing "Create" or "Copy" return owned
//! references that must be released).

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};

// ── Opaque pointer types ──────────────────────────────────────────────────

pub enum JSContextGroupRef {}
pub enum JSGlobalContextRef {}
pub enum JSContextRef {}
pub enum JSValueRef {}
pub enum JSObjectRef {}
pub enum JSStringRef {}
pub enum JSClassRef {}

// ── Enums ─────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JSType {
    kJSTypeUndefined = 0,
    kJSTypeNull = 1,
    kJSTypeBoolean = 2,
    kJSTypeNumber = 3,
    kJSTypeString = 4,
    kJSTypeObject = 5,
    kJSTypeSymbol = 6,
    kJSTypeBigInt = 7,
}

pub type JSPropertyAttributes = c_uint;
pub const kJSPropertyAttributeNone: JSPropertyAttributes = 0;
pub const kJSPropertyAttributeReadOnly: JSPropertyAttributes = 1 << 1;
pub const kJSPropertyAttributeDontEnum: JSPropertyAttributes = 1 << 2;
pub const kJSPropertyAttributeDontDelete: JSPropertyAttributes = 1 << 3;

// ── Context functions ─────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSGlobalContextCreate(globalObjectClass: *mut JSClassRef) -> *mut JSGlobalContextRef;
    pub fn JSGlobalContextRetain(ctx: *mut JSGlobalContextRef) -> *mut JSGlobalContextRef;
    pub fn JSGlobalContextRelease(ctx: *mut JSGlobalContextRef);
    pub fn JSContextGetGlobalObject(ctx: *mut JSContextRef) -> *mut JSObjectRef;
}

// ── Value functions ───────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSValueGetType(ctx: *mut JSContextRef, value: *mut JSValueRef) -> JSType;
    pub fn JSValueIsUndefined(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsNull(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsBoolean(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsNumber(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsString(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsObject(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueIsObjectOfClass(
        ctx: *mut JSContextRef,
        value: *mut JSValueRef,
        jsClass: *mut JSClassRef,
    ) -> bool;
    pub fn JSValueIsStrictEqual(
        ctx: *mut JSContextRef,
        a: *mut JSValueRef,
        b: *mut JSValueRef,
    ) -> bool;
    pub fn JSValueIsEqual(
        ctx: *mut JSContextRef,
        a: *mut JSValueRef,
        b: *mut JSValueRef,
        exception: *mut *mut JSValueRef,
    ) -> bool;
    pub fn JSValueToBoolean(ctx: *mut JSContextRef, value: *mut JSValueRef) -> bool;
    pub fn JSValueToNumber(
        ctx: *mut JSContextRef,
        value: *mut JSValueRef,
        exception: *mut *mut JSValueRef,
    ) -> c_double;
    pub fn JSValueToStringCopy(
        ctx: *mut JSContextRef,
        value: *mut JSValueRef,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSStringRef;
    pub fn JSValueMakeUndefined(ctx: *mut JSContextRef) -> *mut JSValueRef;
    pub fn JSValueMakeNull(ctx: *mut JSContextRef) -> *mut JSValueRef;
    pub fn JSValueMakeBoolean(ctx: *mut JSContextRef, value: bool) -> *mut JSValueRef;
    pub fn JSValueMakeNumber(ctx: *mut JSContextRef, value: c_double) -> *mut JSValueRef;
    pub fn JSValueMakeString(ctx: *mut JSContextRef, string: *mut JSStringRef) -> *mut JSValueRef;
    pub fn JSValueMakeSymbol(
        ctx: *mut JSContextRef,
        description: *mut JSStringRef,
    ) -> *mut JSValueRef;
}

// ── Object functions ──────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSObjectIsFunction(ctx: *mut JSContextRef, object: *mut JSObjectRef) -> bool;
    pub fn JSObjectIsConstructor(ctx: *mut JSContextRef, object: *mut JSObjectRef) -> bool;
    pub fn JSObjectCallAsFunction(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        thisObject: *mut JSObjectRef,
        argumentCount: usize,
        arguments: *const *mut JSValueRef,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSValueRef;
    pub fn JSObjectCallAsConstructor(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        argumentCount: usize,
        arguments: *const *mut JSValueRef,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSObjectRef;
    pub fn JSObjectGetProperty(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        propertyName: *mut JSStringRef,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSValueRef;
    pub fn JSObjectSetProperty(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        propertyName: *mut JSStringRef,
        value: *mut JSValueRef,
        attributes: JSPropertyAttributes,
        exception: *mut *mut JSValueRef,
    );
    pub fn JSObjectHasProperty(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        propertyName: *mut JSStringRef,
    ) -> bool;
    pub fn JSObjectDeleteProperty(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        propertyName: *mut JSStringRef,
        exception: *mut *mut JSValueRef,
    ) -> bool;
    pub fn JSObjectGetPrototype(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
    ) -> *mut JSValueRef;
    pub fn JSObjectSetPrototype(
        ctx: *mut JSContextRef,
        object: *mut JSObjectRef,
        value: *mut JSValueRef,
    );
}

// ── String functions ──────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSStringCreateWithUTF8CString(string: *const c_char) -> *mut JSStringRef;
    pub fn JSStringGetLength(string: *mut JSStringRef) -> usize;
    pub fn JSStringGetMaximumUTF8CStringSize(string: *mut JSStringRef) -> usize;
    pub fn JSStringGetUTF8CString(
        string: *mut JSStringRef,
        buffer: *mut c_char,
        bufferSize: usize,
    ) -> usize;
    pub fn JSStringRetain(string: *mut JSStringRef) -> *mut JSStringRef;
    pub fn JSStringRelease(string: *mut JSStringRef);
    pub fn JSStringIsEqual(a: *mut JSStringRef, b: *mut JSStringRef) -> bool;
}

// ── Evaluation ────────────────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSEvaluateScript(
        ctx: *mut JSContextRef,
        script: *mut JSStringRef,
        thisObject: *mut JSObjectRef,
        sourceURL: *mut JSStringRef,
        startingLineNumber: i32,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSValueRef;
}

// ── Typed Array functions ─────────────────────────────────────────────────

unsafe extern "C" {
    pub fn JSObjectMakeArrayBufferWithBytesNoCopy(
        ctx: *mut JSContextRef,
        bytes: *mut c_void,
        byteLength: usize,
        bytesDeallocator: *mut c_void,
        exception: *mut *mut JSValueRef,
    ) -> *mut JSObjectRef;

    // ── GC protection (not in public headers; available on macOS) ────────
    pub fn JSValueProtect(ctx: *mut JSContextRef, value: *mut JSValueRef);
    pub fn JSValueUnprotect(ctx: *mut JSContextRef, value: *mut JSValueRef);
}

// ── Function construction ─────────────────────────────────────────────────

pub type JSObjectCallAsFunctionCallback = unsafe extern "C" fn(
    ctx: *mut JSContextRef,
    function: *mut JSObjectRef,
    thisObject: *mut JSObjectRef,
    argumentCount: usize,
    arguments: *const *mut JSValueRef,
    exception: *mut *mut JSValueRef,
) -> *mut JSValueRef;

pub type JSObjectFinalizeCallback = unsafe extern "C" fn(object: *mut JSObjectRef);

unsafe extern "C" {
    /// Creates a JS function backed by a C callback.  The callback receives
    /// the context, function object, this, arguments, and an exception
    /// out-parameter.  No user-data parameter — use a side registry or
    /// `JSObjectSetPrivate` with a custom class for stateful callbacks.
    pub fn JSObjectMakeFunctionWithCallback(
        ctx: *mut JSContextRef,
        name: *mut JSStringRef,
        callAsFunction: Option<JSObjectCallAsFunctionCallback>,
    ) -> *mut JSObjectRef;
}

// ── Private data (requires object created with a JSClass that supports it) ─

unsafe extern "C" {
    pub fn JSObjectSetPrivate(object: *mut JSObjectRef, data: *mut c_void) -> bool;
    pub fn JSObjectGetPrivate(object: *mut JSObjectRef) -> *mut c_void;
}

// ── JSClass (for custom object types with private data + finalize) ────────

pub type JSClassAttributes = c_uint;
pub const kJSClassAttributeNone: JSClassAttributes = 0;
pub const kJSClassAttributeNoAutomaticPrototype: JSClassAttributes = 1 << 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JSStaticValue {
    pub name: *const c_char,
    pub getProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            exception: *mut *mut JSValueRef,
        ) -> *mut JSValueRef,
    >,
    pub setProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            exception: *mut *mut JSValueRef,
        ) -> bool,
    >,
    pub attributes: JSPropertyAttributes,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JSStaticFunction {
    pub name: *const c_char,
    pub callAsFunction: Option<JSObjectCallAsFunctionCallback>,
    pub attributes: JSPropertyAttributes,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JSClassDefinition {
    pub version: c_int,
    pub attributes: JSClassAttributes,
    pub className: *const c_char,
    pub parentClass: *mut JSClassRef,
    pub staticValues: *const JSStaticValue,
    pub staticFunctions: *const JSStaticFunction,
    pub initialize: Option<
        unsafe extern "C" fn(ctx: *mut JSContextRef, object: *mut JSObjectRef),
    >,
    pub finalize: Option<JSObjectFinalizeCallback>,
    pub hasProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            propertyName: *mut JSStringRef,
        ) -> bool,
    >,
    pub getProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            propertyName: *mut JSStringRef,
            exception: *mut *mut JSValueRef,
        ) -> *mut JSValueRef,
    >,
    pub setProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            propertyName: *mut JSStringRef,
            value: *mut JSValueRef,
            exception: *mut *mut JSValueRef,
        ) -> bool,
    >,
    pub deleteProperty: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            propertyName: *mut JSStringRef,
            exception: *mut *mut JSValueRef,
        ) -> bool,
    >,
    pub getPropertyNames: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            propertyNameArray: *mut JSObjectRef, // JSPropertyNameAccumulatorRef in C API
        ),
    >,
    pub callAsFunction: Option<JSObjectCallAsFunctionCallback>,
    pub callAsConstructor: Option<JSObjectCallAsFunctionCallback>,
    pub hasInstance: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            constructor: *mut JSObjectRef,
            possibleInstance: *mut JSValueRef,
            exception: *mut *mut JSValueRef,
        ) -> bool,
    >,
    pub convertToType: Option<
        unsafe extern "C" fn(
            ctx: *mut JSContextRef,
            object: *mut JSObjectRef,
            ty: JSType,
            exception: *mut *mut JSValueRef,
        ) -> *mut JSValueRef,
    >,
}

unsafe extern "C" {
    pub fn JSClassCreate(definition: *const JSClassDefinition) -> *mut JSClassRef;
    pub fn JSClassRetain(jsClass: *mut JSClassRef);
    pub fn JSClassRelease(jsClass: *mut JSClassRef);
}

unsafe extern "C" {
    /// Creates a JS object of the given class.  The `data` pointer is stored
    /// as private data accessible via `JSObjectGetPrivate`.  It is freed by
    /// the class's `finalize` callback.
    pub fn JSObjectMake(
        ctx: *mut JSContextRef,
        jsClass: *mut JSClassRef,
        data: *mut c_void,
    ) -> *mut JSObjectRef;
}
