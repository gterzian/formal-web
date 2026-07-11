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

use crate::ExecutionContext;
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
    pub uint8_array: T::Constructor,
    pub boolean: T::Constructor,
    pub number: T::Constructor,
    pub string: T::Constructor,
    pub bigint: T::Constructor,
    pub date: T::Constructor,
    pub regexp: T::Constructor,
    pub map: T::Constructor,
    pub set: T::Constructor,
    pub boolean_prototype: T::JsObject,
    pub number_prototype: T::JsObject,
    pub string_prototype: T::JsObject,
    pub bigint_prototype: T::JsObject,
    pub date_prototype: T::JsObject,
    pub regexp_prototype: T::JsObject,
    pub map_prototype: T::JsObject,
    pub set_prototype: T::JsObject,
    pub error_prototype: T::JsObject,
    pub type_error_prototype: T::JsObject,
    pub range_error_prototype: T::JsObject,
    pub syntax_error_prototype: T::JsObject,
    pub reference_error_prototype: T::JsObject,
    pub uri_error_prototype: T::JsObject,
    pub eval_error_prototype: T::JsObject,
    pub object_prototype: T::JsObject,
    pub function_prototype: T::JsObject,
    pub async_iterator_prototype: T::JsObject,
}

/// <https://html.spec.whatwg.org/#hostloadimportedmodule>
#[derive(Debug, Clone)]
pub struct ModuleRequest<T: JsTypes> {
    pub specifier: T::JsString,
    pub attributes: Vec<(T::JsString, T::JsValue)>,
}

/// GC-safe pair of promise resolve/reject callables.
///
/// Created by [`ExecutionContext::new_promise_pending`] as a replacement
/// for engine-specific resolver types (e.g. Boa's `ResolvingFunctions`).
/// Stored in GC-traced domain structs to hold pending promise resolvers.
#[derive(Clone)]
#[cfg_attr(
    feature = "boa",
    derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)
)]
pub struct PromiseResolvers<T: JsTypes> {
    pub resolve: T::JsObject,
    pub reject: T::JsObject,
    // On JSC, protect both resolve and reject JS function objects from GC
    // while Rust code holds references.  Rc refcounting keeps protections
    // alive across Clone/Drop cycles.
    #[cfg(not(feature = "boa"))]
    #[allow(dead_code)]
    root: Option<(
        std::rc::Rc<crate::gc::GcRootHandle<T>>,
        std::rc::Rc<crate::gc::GcRootHandle<T>>,
    )>,
}

impl<T: JsTypes> std::fmt::Debug for PromiseResolvers<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PromiseResolvers")
            .field("resolve", &std::format!("{:p}", &self.resolve as *const _))
            .field("reject", &std::format!("{:p}", &self.reject as *const _))
            .finish()
    }
}

#[cfg(not(feature = "boa"))]
unsafe impl<T: JsTypes> crate::gc::Trace for PromiseResolvers<T> {}

#[cfg(not(feature = "boa"))]
impl<T: JsTypes> crate::gc::Finalize for PromiseResolvers<T> {}

impl<T: JsTypes> PromiseResolvers<T> {
    /// Create promise resolvers with GC protection.
    /// On JSC, protects both resolve and reject via JSValueProtect.
    /// On Boa, this is a no-op (GC traces via Trace derive).
    #[cfg_attr(feature = "boa", allow(unused_variables))]
    pub fn new(resolve: T::JsObject, reject: T::JsObject, ec: &mut dyn ExecutionContext<T>) -> Self
    where
        T: crate::JsTypesWithRealm,
    {
        #[cfg(not(feature = "boa"))]
        let resolve_value = T::value_from_object(resolve.clone());
        #[cfg(not(feature = "boa"))]
        let reject_value = T::value_from_object(reject.clone());
        #[cfg(not(feature = "boa"))]
        let root = Some((
            std::rc::Rc::new(ec.create_root(&resolve_value)),
            std::rc::Rc::new(ec.create_root(&reject_value)),
        ));
        Self {
            resolve,
            reject,
            #[cfg(not(feature = "boa"))]
            root,
        }
    }

    /// Resolves the associated promise with the given value.
    pub fn resolve(
        &self,
        value: T::JsValue,
        ec: &mut dyn ExecutionContext<T>,
    ) -> crate::Completion<T::JsValue, T> {
        let undefined = ec.value_undefined();
        ec.call(&self.resolve, &undefined, &[value])
    }

    /// Rejects the associated promise with the given reason.
    pub fn reject(
        &self,
        reason: T::JsValue,
        ec: &mut dyn ExecutionContext<T>,
    ) -> crate::Completion<T::JsValue, T> {
        let undefined = ec.value_undefined();
        ec.call(&self.reject, &undefined, &[reason])
    }
}
