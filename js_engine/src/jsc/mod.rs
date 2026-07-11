//! # `jsc` — JSC Engine Backend
//!
//! This module provides the JavaScriptCore implementation of the `js_engine`
//! traits (`JsTypes`, `JsEngine`, `ExecutionContext`, `EcmascriptHost`).
//! It is gated behind the `jsc` Cargo feature.
//!
//! ## Implementation strategy
//!
//! Many ECMA-262 operations (promises, BigInt, JSON) that are not in the
//! public JSC C API are implemented via `JSEvaluateScript` with temporary
//! global properties for argument passing.  This works for the POC but
//! production code should use native API calls where available.
//!
//! `create_builtin_function` uses a custom JSClass with `callAsFunction` and
//! `finalize` callbacks.  The behaviour closure is wrapped to capture a raw
//! engine pointer (stable for the engine's lifetime), boxed, and stored as
//! private data on the JSObject via `JSObjectMake`.  The C callback retrieves
//! the closure via `JSObjectGetPrivate` and calls it; `finalize` drops the
//! Box to free the closure on GC.
//!
//! ## Known issues
//!
//! - **`create_plain_object` → `JSObjectSetProperty` crash**: setting a
//!   property on an object returned by `eval("{}")` causes SIGSEGV on
//!   current macOS.  `create_empty_array` + `array_push` works fine.
//! - **Iterator operations crash**: `get_iterator` / `get_iterator_step_value`
//!   not yet stable.
//! - **Value type queries need context**: `JscValue` carries a `ctx`
//!   pointer so `value_as_*` trait methods can call `JSValueGetType`.
//!
//! ## Submodules
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | Safe wrapper types (`JscValue`, `JscObject`, `JscString`, etc.) |
//! | [`engine`] | `JscTypes`, `JscEngine` — the `JsEngine<JscTypes>` implementation |
//!
//! Raw FFI bindings live in [`crate::jsc_sys`].

mod engine;
mod types;

pub use engine::{
    JscEngine, JscTypes, clear_current_engine, create_builtin_fn_with_captures, set_current_engine,
};
pub use types::*;
