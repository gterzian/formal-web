//! # `js_engine` — the generic JS engine trait
//!
//! Two categories of abstraction (see `js_engine/README.md` for the full
//! philosophy):
//!
//! 1. **Standard** — `JsEngine<T>` mirrors ECMA-262 abstract operations.
//! 2. **Weird** — `gc.rs` abstracts engine-specific GC (no spec equivalent).
//!
//! ## Modules
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | `JsTypes`, `JsTypesWithRealm` |
//! | [`engine`] | `JsEngine`, `Completion`, `EcmascriptHost`, `HostHooks` |
//! | [`enums`] | `Numeric`, `PreferredType`, `IntegrityLevel`, etc. |
//! | [`records`] | `IteratorRecord`, `PromiseCapability`, `PromiseResolvers`, `PropertyDescriptor` |
//! | [`gc`] | `Trace`, `Finalize`, `GcRootHandle` (engine-specific) |
//! | [`boa`] | Boa backend (feature = "boa") |
//! | [`jsc`] | JSC backend (feature = "jsc") |
//!
//! ## Feature flags
//!
//! | Feature | Engine | Default |
//! |---|---|---|
//! | `boa` | Boa (git dep) | **default** |
//! | `jsc` | JavaScriptCore (macOS) | opt-in |
//!
//! At most one engine feature can be active.

pub mod engine;
pub mod enums;
pub mod gc;
pub mod records;
pub mod types;

#[cfg(feature = "boa")]
pub mod boa;

#[cfg(feature = "jsc")]
pub mod jsc_sys;

#[cfg(feature = "jsc")]
pub mod jsc;

pub use engine::{Completion, EcmascriptHost, ExecutionContext, HostHooks, JsEngine};
pub use enums::{
    IntegrityLevel, IteratorKind, Numeric, PreferredType, PromiseRejectionOperation,
    SharedMemoryOrder, TypedArrayElementType,
};
pub use gc::{Finalize, GcRootHandle, JsTypesGcExt, Trace};
pub use records::{
    IteratorRecord, ModuleRequest, PromiseCapability, PromiseResolvers, PropertyDescriptor,
    RealmIntrinsics, RootedPromiseCapability,
};
pub use types::{JsTypes, JsTypesWithRealm};
