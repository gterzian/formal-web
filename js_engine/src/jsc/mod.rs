//! # `jsc` — JSC Engine Backend
//!
//! This module provides the JavaScriptCore implementation of the `js_engine`
//! traits (`JsTypes`, `JsEngine`).  It is gated behind the `jsc` Cargo feature.
//!
//! ## Submodules
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | Safe wrapper types (`JscValue`, `JscObject`, `JscString`, etc.) |
//! | [`engine`] | `JscTypes`, `JscEngine` — the `JsEngine<JscTypes>` implementation |
//!
//! Raw FFI bindings live in [`crate::jsc_sys`].

mod types;
mod engine;

pub use engine::{JscEngine, JscTypes};
pub use types::*;
