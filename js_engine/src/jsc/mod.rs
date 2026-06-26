//! # `jsc` — JSC Engine Backend
//!
//! This module provides the JavaScriptCore implementation of the `js_engine`
//! traits (`JsTypes`, `JsEngine`).  It is gated behind the `jsc` Cargo feature.
//!
//! ## Submodules
//!
//! | Module | Contents |
//! |---|---|
//! | [`sys`] | Raw FFI bindings to JavaScriptCore framework (34 extern functions) |
//! | [`types`] | Safe wrapper types (`JscValue`, `JscObject`, `JscString`, etc.) |
//! | [`engine`] | `JscTypes`, `JscEngine` — the `JsEngine<JscTypes>` implementation |

pub(crate) mod sys;
mod types;
mod engine;

pub use engine::{JscEngine, JscTypes};
pub use types::*;
