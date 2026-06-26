//! # `js_engine` — ECMA-262 Abstract Operation Trait
//!
//! <https://tc39.es/ecma262/>
//!
//! | Feature | Module | Engine | Status |
//! |---|---|---|---|
//! | `boa` | [`boa`] | Boa (git dep) | Most operations implemented |
//! | `jsc` | [`jsc`] | JavaScriptCore (macOS) | Basic operations implemented |
//!
//! # Modules
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | `JsTypes`, `JsTypesWithRealm` |
//! | [`engine`] | `JsEngine`, `Completion`, `HostHooks` |
//! | [`enums`] | `Numeric`, `PreferredType`, `IntegrityLevel`, `IteratorKind`, etc. |
//! | [`records`] | `IteratorRecord`, `PromiseCapability`, `PropertyDescriptor`, `RealmIntrinsics`, `ModuleRequest` |

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

pub use engine::{Completion, EcmascriptHost, HostHooks, JsEngine};
pub use enums::{
    IntegrityLevel, IteratorKind, Numeric, PreferredType, PromiseRejectionOperation,
    SharedMemoryOrder, TypedArrayElementType,
};
pub use gc::{Finalize, GcRootHandle, JsEngineGcExt, JsTypesGcExt, Trace};
pub use records::{
    IteratorRecord, ModuleRequest, PromiseCapability, PropertyDescriptor, RealmIntrinsics,
};
pub use types::{JsTypes, JsTypesWithRealm};

#[cfg(feature = "boa")]
pub use boa::{BoaEngine, BoaTypes};
