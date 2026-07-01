//! Record types used by `JsEngine<T>` method signatures:
//!
//! | Record | ECMA-262 ref | Fields |
//! |---|---|---|
//! | `IteratorRecord<T>` | §7.4.1 | `iterator`, `next_method`, `done` |
//! | `PromiseCapability<T>` | §27.2.1 | `promise`, `resolve`, `reject` |
//! | `PropertyDescriptor<T>` | §6.2.5 | `value`, `writable`, `get`, `set`, `enumerable`, `configurable` |
//! | `RealmIntrinsics<T>` | §9.1 (table 7) | 15 constructors/prototypes |
//! | `ModuleRequest<T>` | HTML host hooks | `specifier`, `attributes` |
//!
//! `PropertyDescriptor<T>` is a concrete struct (NOT an associated type on
//! `JsTypes`) because the spec's Property Descriptor is a plain record type
//! with no engine-specific representation.

use crate::JsTypes;
use crate::gc::GcRootHandle;

/// <https://tc39.es/ecma262/#sec-iterator-record>
#[derive(Debug, Clone)]
pub struct IteratorRecord<T: JsTypes> {
    pub iterator: T::JsObject,
    pub next_method: T::Function,
    pub done: bool,
}

/// <https://tc39.es/ecma262/#sec-promisecapability-records>
#[derive(Debug, Clone)]
pub struct PromiseCapability<T: JsTypes> {
    pub promise: T::JsValue,
    pub resolve: T::Function,
    pub reject: T::Function,
}

/// A GC-safe, long-lived form of a promise capability for host-side state.
pub struct RootedPromiseCapability<T: JsTypes> {
    pub promise: GcRootHandle<T>,
    pub resolve: GcRootHandle<T>,
    pub reject: GcRootHandle<T>,
}

/// <https://tc39.es/ecma262/#sec-property-descriptor-specification-type>
#[derive(Debug, Clone)]
pub struct PropertyDescriptor<T: JsTypes> {
    pub value: Option<T::JsValue>,
    pub writable: Option<bool>,
    pub get: Option<T::Function>,
    pub set: Option<T::Function>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

/// <https://tc39.es/ecma262/#table-basic-intrinsics>
#[derive(Debug, Clone)]
pub struct RealmIntrinsics<T: JsTypes> {
    pub array_buffer: T::Constructor,
    pub shared_array_buffer: T::Constructor,
    pub promise: T::Constructor,
    pub object: T::Constructor,
    pub function: T::Constructor,
    pub error: T::Constructor,
    pub type_error: T::Constructor,
    pub range_error: T::Constructor,
    pub syntax_error: T::Constructor,
    pub reference_error: T::Constructor,
    pub uri_error: T::Constructor,
    pub eval_error: T::Constructor,
    pub array: T::Constructor,
    pub object_prototype: T::JsObject,
    pub function_prototype: T::JsObject,
}

/// <https://html.spec.whatwg.org/#hostloadimportedmodule>
#[derive(Debug, Clone)]
pub struct ModuleRequest<T: JsTypes> {
    pub specifier: T::JsString,
    pub attributes: Vec<(T::JsString, T::JsValue)>,
}
