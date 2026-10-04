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
//! | [`v8`] | V8 backend (feature = "v8") |
//!
//! ## Feature flags
//!
//! | Feature | Engine | Default |
//! |---|---|---|
//! | `boa` | Boa (git dep) | **default** |
//! | `v8` | V8 (macOS arm64) | opt-in |
//!
//! At most one engine feature can be active.

pub mod engine;
pub mod enums;
pub mod gc;
pub mod records;
pub mod types;

#[cfg(feature = "boa")]
pub mod boa;

#[cfg(feature = "v8")]
pub mod v8;

/// Log an invariant violation and abort the process.
///
/// Used where a panic would unwind across an FFI boundary (V8's GC trace
/// callback and native callbacks): a violated GC/handle invariant is
/// unrecoverable, and continuing can corrupt the heap, so the process aborts
/// deterministically with a message rather than unwinding into C++ or
/// dereferencing freed memory.
#[macro_export]
macro_rules! fatal_invariant {
    ($($arg:tt)*) => {{
        ::log::error!($($arg)*);
        ::std::process::abort();
    }};
}

/// Re-exported cppgc surface for the V8 backend, referenced by
/// `#[gc_struct]`-generated tracing impls. Content code must not depend on
/// `rusty_v8` directly, so the cppgc traits and the visitor type are
/// re-exported here under a stable path.
#[cfg(feature = "v8")]
pub mod v8_gc {
    pub use rusty_v8::cppgc::{GarbageCollected, Traced, Visitor};
}

pub use engine::{
    Completion, EcmascriptHost, ExecutionContext, HostHooks, JsEngine, create_engine,
};
pub use enums::{
    IntegrityLevel, IteratorKind, Numeric, PreferredType, PromiseRejectionOperation, PromiseState,
    SharedMemoryOrder, TypedArrayElementType,
};
#[cfg(feature = "v8")]
pub use gc::associate_existing_object;
pub use gc::{
    Finalize, GcCell, GcRootHandle, JsTypesGcExt, Trace, create_platform_object, gc_cell_new,
};
#[cfg(feature = "boa")]
pub use js_engine_macros::gc_struct_boa as gc_struct;

#[cfg(feature = "v8")]
pub use js_engine_macros::gc_struct_v8 as gc_struct;

pub use js_engine_macros::ignore_trace;
pub use records::{
    IteratorRecord, ModuleRequest, PromiseCapability, PromiseResolvers, PropertyDescriptor,
    RealmIntrinsics, RootedPromiseCapability,
};
pub use types::{JsTypes, JsTypesWithRealm};
