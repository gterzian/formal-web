//! # GC & Lifecycle — the only engine-specific abstraction
//!
//! Everything else in this crate mirrors standard ECMA-262 abstract operations.
//! GC has no ECMA-262 equivalent — each JS engine has its own internal GC API.
//! This module abstracts over those differences (see `js_engine/README.md`).
//!
//! ## Primitives
//!
//! | Type | Role |
//! |---|---|
//! | [`Trace`] | Marker trait for GC-reachable fields |
//! | [`Finalize`] | Lifecycle hook when GC reclaims backing memory |
//! | [`JsTypesGcExt`] | Extends [`JsTypes`] with cycle-safe `Reflector` |
//! | [`JsEngineGcExt`] | Extends [`JsEngine`] with `create_root` |
//! | [`GcRootHandle`] | RAII guard for rooting a JS value |
//!
//! Each backend provides its own implementations inside `#[cfg]`-gated
//! sub-modules below.

use crate::JsTypes;

// ============================================================================
// SECTION I: SPEC-ANNOTATION TRAITS
// ============================================================================

/// Marker trait: declares that a Rust structure participates in the GC
/// reachability graph.
///
/// This documents which domain types hold JavaScript references for spec
/// compliance review.  Actual GC tracing semantics are engine-specific.
///
/// # Safety
///
/// Implementations must ensure that every field capable of holding a JavaScript
/// value is also made known to the engine's GC mechanism.
pub unsafe trait Trace {}

/// Lifecycle hook executed when the host engine reclaims the object's backing
/// memory.
pub trait Finalize {
    fn finalize(&self) {}
}

// ============================================================================
// SECTION II: REFLECTOR & ROOTING
// ============================================================================

/// Extends [`JsTypes`] with the cycle-safe reflector link.
///
/// The `Reflector` is a structural twin link that lets a Rust domain object
/// reference its associated JS wrapper object without creating fatal cycles.
/// The concrete representation is engine-specific.
pub trait JsTypesGcExt: JsTypes + Sized + 'static {
    /// The cycle-safe structural twin link.
    type Reflector: Clone + 'static;

    fn create_reflector(obj: &Self::JsObject) -> Self::Reflector;
    fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject>;
}

/// An RAII guard that unroots a protected JS value when dropped.
pub struct GcRootHandle<T: JsTypes> {
    pub(crate) value: T::JsValue,
    pub(crate) unroot_action: Option<Box<dyn FnOnce(&T::JsValue)>>,
}

impl<T: JsTypes> Drop for GcRootHandle<T> {
    fn drop(&mut self) {
        if let Some(action) = self.unroot_action.take() {
            action(&self.value);
        }
    }
}

// ============================================================================
// SECTION III: ENGINE-SPECIFIC IMPLEMENTATIONS
// ============================================================================

// ── Boa backend ───────────────────────────────────────────────────────────
#[cfg(feature = "boa")]
mod boa_gc_impl {
    use super::*;
    use crate::boa::BoaTypes;

    // SAFETY: `boa_gc::Trace` has the same safety contract as
    // `js_engine::gc::Trace` — the implementor guarantees that every GC
    // reachable field is visited during trace.
    unsafe impl<T: boa_gc::Trace + ?Sized> Trace for T {}

    impl<T: boa_gc::Finalize + ?Sized> Finalize for T {
        #[inline]
        fn finalize(&self) {
            boa_gc::Finalize::finalize(self);
        }
    }

    impl JsTypesGcExt for BoaTypes {
        type Reflector = boa_engine::object::JsObject;

        fn create_reflector(obj: &Self::JsObject) -> Self::Reflector {
            obj.clone()
        }
        fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject> {
            Some(reflector.clone())
        }
    }
}

// ── JSC backend ───────────────────────────────────────────────────────────
#[cfg(not(feature = "boa"))]
mod jsc_gc_impl {
    use super::*;
    use crate::jsc::JscTypes;

    impl JsTypesGcExt for JscTypes {
        type Reflector = *mut std::ffi::c_void;

        fn create_reflector(obj: &Self::JsObject) -> Self::Reflector {
            obj.as_raw() as *mut std::ffi::c_void
        }

        fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject> {
            if reflector.is_null() {
                None
            } else {
                Some(unsafe {
                    crate::jsc::JscObject::from_raw(
                        std::mem::transmute::<
                            *mut std::ffi::c_void,
                            *mut crate::jsc_sys::JSObjectRef,
                        >(*reflector),
                        std::ptr::null_mut(),
                    )
                })
            }
        }
    }

    unsafe extern "C" {
        fn JSObjectGetPrivate(object: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    #[allow(dead_code)]
    pub extern "C" fn jsc_generic_finalizer<V>(object: *mut std::ffi::c_void) {
        unsafe {
            let private_data = JSObjectGetPrivate(object);
            if !private_data.is_null() {
                drop(std::sync::Arc::from_raw(
                    private_data as *const std::cell::RefCell<V>,
                ));
            }
        }
    }
}
