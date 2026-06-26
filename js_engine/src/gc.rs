//! # Engine-Agnostic Garbage Collection & Lifecycle Management
//!
//! ## Architecture
//!
//! Bridges two incompatible GC paradigms without `#[cfg]` leaking into domain code:
//!
//! - **Boa (tracing GC):** `boa_gc::Gc<GcRefCell<V>>` — the value is managed
//!   by Boa's mark-sweep collector.  Domain types `#[derive(boa_gc::Trace)]`
//!   for actual GC marking.
//! - **JSC (explicit boundary):** `Arc<RefCell<V>>` — the value is
//!   ref-counted.  No tracing needed; lifecycle is managed by
//!   `JSObjectSetPrivate`/finalize callback.
//!
//! ## Primitives
//!
//! | Type | Role |
//! |---|---|
//! | [`Trace`] | Marker trait: spec-annotation for GC-reachable fields |
//! | [`Finalize`] | Lifecycle hook when GC reclaims backing memory |
//! | [`JsTypesGcExt`] | Extends [`JsTypes`] with cycle-safe `Reflector` |
//! | [`JsEngineGcExt`] | Extends [`JsEngine`] with `create_root` |
//! | [`GcRootHandle`] | RAII guard for `JSValueProtect`/`JSValueUnprotect` |
//!
//! ## Spec annotation vs actual tracing
//!
//! [`Trace`] is a **spec-annotation only** — it marks which types participate
//! in the reachability graph.  For the Boa backend, actual GC tracing is
//! provided by `boa_gc::Trace` (domain types `#[derive(boa_gc::Trace)]`).
//! For JSC, no tracing is needed — `Arc` manages the Rust memory, and
//! `JSObjectSetPrivate`/finalize callback bridges to JSC's lifecycle.
//!
//! ## Managed allocation
//!
//! Domain types are allocated through engine-specific `allocate_managed`
//! methods that are not abstracted in this module (the Boa `Gc<T: Trace>`
//! bound creates type-system constraints that prevent a generic GAT).
//! See the concrete engine implementations for allocation.
//!
//! - **Boa:** `engine.allocate_managed::<V>(value)` where
//!   `V: boa_gc::Trace + 'static`, returns `Gc<GcRefCell<V>>`.
//! - **JSC:** `engine.allocate_managed::<V>(value)` where
//!   `V: 'static`, returns `Arc<RefCell<V>>`.

use crate::JsTypes;

// ============================================================================
// SECTION I: SPEC-ANNOTATION TRAITS
// ============================================================================

/// Marker trait: declares that a Rust structure participates in the GC
/// reachability graph.
///
/// This is a **spec-annotation only** — it documents which domain types hold
/// JavaScript references for spec compliance review.  Actual GC tracing is
/// handled by engine-specific traits (`boa_gc::Trace` in Boa, no-op in JSC).
///
/// # Safety
///
/// Implementations of this trait promise that every field capable of holding
/// a JavaScript value is also made known to the engine's GC mechanism
/// (e.g., via `#[derive(boa_gc::Trace)]` for the Boa backend).
///
/// # Spec
///
/// <https://tc39.es/ecma262/#sec-gc-reachability>
pub unsafe trait Trace {}

/// Lifecycle hook executed when the host engine reclaims the object's backing
/// memory.
///
/// # Spec
///
/// <https://html.spec.whatwg.org/#host-object-finalization>
pub trait Finalize {
    fn finalize(&self) {}
}

// ============================================================================
// SECTION II: REFLECTOR & ROOTING
// ============================================================================

/// Extends [`JsTypes`] with the cycle-safe reflector link.
///
/// # Spec
///
/// The `Reflector` is a structural twin link that lets a Rust domain object
/// reference its associated JS wrapper object without creating fatal cycles.
///
/// - **Boa:** Strong `JsObject` reference — cycles are safe in tracing GC.
/// - **JSC:** Raw unprotected pointer — `JSValueProtect` here would create
///   an unbreakable cycle between `JSObjectSetPrivate` (native data) and
///   the protected JS wrapper reference.
pub trait JsTypesGcExt: JsTypes + Sized + 'static {
    /// The cycle-safe structural twin link.
    type Reflector: Clone + 'static;

    fn create_reflector(obj: &Self::JsObject) -> Self::Reflector;
    fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject>;
}

/// Extends [`JsEngine`] with rooting operations.
///
/// # Spec
///
/// Provides `create_root` to anchor a JS value across asynchronous execution
/// bounds, preventing premature collection.
pub trait JsEngineGcExt<T: JsTypesGcExt> {
    /// Explicitly anchors a JS value to prevent collection across async
    /// execution bounds.
    ///
    /// Returns a [`GcRootHandle`] that unroots on drop.
    fn create_root(&mut self, value: &T::JsValue) -> GcRootHandle<T>;
}

/// An RAII guard that safely unroots a protected JS value when dropped.
///
/// # Spec
///
/// - **Boa:** No-op — dropping the handle inherently releases the reference.
/// - **JSC:** Calls `JSValueUnprotect` in the destructor.
#[allow(clippy::type_complexity)]
pub struct GcRootHandle<T: JsTypesGcExt> {
    value: T::JsValue,
    unroot_action: Option<Box<dyn FnOnce(&T::JsValue)>>,
}

impl<T: JsTypesGcExt> Drop for GcRootHandle<T> {
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
    use crate::boa::{BoaEngine, BoaTypes};
    use super::*;

    impl<T: boa_gc::Finalize + ?Sized> Finalize for T {
        #[inline]
        fn finalize(&self) {
            boa_gc::Finalize::finalize(self);
        }
    }

    impl JsTypesGcExt for BoaTypes {
        /// Boa handles cyclic tracing natively.  Strong object handles are
        /// safe — no cycle hazard.
        type Reflector = boa_engine::object::JsObject;

        fn create_reflector(obj: &Self::JsObject) -> Self::Reflector { obj.clone() }
        fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject> {
            Some(reflector.clone())
        }
    }

    impl JsEngineGcExt<BoaTypes> for BoaEngine {
        fn create_root(
            &mut self,
            value: &boa_engine::JsValue,
        ) -> GcRootHandle<BoaTypes> {
            GcRootHandle {
                value: value.clone(),
                // No-op: dropping the handle inherently unroots in Boa's GC.
                unroot_action: None,
            }
        }
    }
}

// ── JSC backend ───────────────────────────────────────────────────────────
#[cfg(not(feature = "boa"))]
mod jsc_gc_impl {
    use super::*;
    use crate::jsc::{JscEngine, JscTypes};

    impl JsTypesGcExt for JscTypes {
        /// SAFETY: Must be an explicitly unprotected raw pointer.  Using
        /// `JSValueProtect` here creates a fatal cycle between
        /// `JSObjectSetPrivate` and the protected reference.
        type Reflector = *mut std::ffi::c_void;

        fn create_reflector(obj: &Self::JsObject) -> Self::Reflector {
            obj.as_raw() as *mut std::ffi::c_void
        }

        fn upgrade_reflector(reflector: &Self::Reflector) -> Option<Self::JsObject> {
            if reflector.is_null() {
                None
            } else {
                // SAFETY: The reflector is a raw pointer to a live JSObjectRef
                // that outlives the access.  `JSObjectRef` is a ZST so
                // transmute is required (`as` cast is structurally invalid).
                // SAFETY: The reflector is a raw pointer to a live JSObjectRef
                // that outlives the access.  `sys::JSObjectRef` is a ZST enum,
                // so transmute is required (`as` cast is structurally invalid).
                Some(unsafe {
                    crate::jsc::JscObject::from_raw(
                        std::mem::transmute::<
                            *mut std::ffi::c_void,
                            *mut crate::jsc::sys::JSObjectRef,
                        >(*reflector),
                    )
                })
            }
        }
    }

    unsafe extern "C" {
        fn JSValueProtect(ctx: *mut std::ffi::c_void, value: *mut std::ffi::c_void);
        fn JSValueUnprotect(ctx: *mut std::ffi::c_void, value: *mut std::ffi::c_void);
        fn JSObjectGetPrivate(object: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    impl JsEngineGcExt<JscTypes> for JscEngine {
        fn create_root(
            &mut self,
            value: &crate::jsc::JscValue,
        ) -> GcRootHandle<JscTypes> {
            let ctx_ptr = self.context().as_context_ref() as *mut std::ffi::c_void;
            let val_ptr = value.as_raw() as *mut std::ffi::c_void;

            unsafe {
                JSValueProtect(ctx_ptr, val_ptr);
            }

            GcRootHandle {
                value: *value,
                unroot_action: Some(Box::new(move |_val| {
                    unsafe {
                        JSValueUnprotect(ctx_ptr, val_ptr);
                    }
                })),
            }
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // JSC SPECIFIC: THE FINALIZER LIFECYCLE HOOK
    // ───────────────────────────────────────────────────────────────────────
    //
    /// When registering a `JSClassDefinition` for a host object in JSC, this
    /// callback is registered as the `finalize` hook.  When the JS wrapper
    /// becomes unreachable, JSC sweeps it and fires this function.
    ///
    /// # Safety
    ///
    /// `object` must be a valid `JSObjectRef` with private data set via
    /// `JSObjectSetPrivate` pointing to an `Arc<RefCell<V>>`.
    pub extern "C" fn jsc_generic_finalizer<V>(object: *mut std::ffi::c_void) {
        unsafe {
            let private_data = JSObjectGetPrivate(object);
            if !private_data.is_null() {
                // Re-materialize the Arc and drop it, decrementing the
                // reference count and freeing the Rust memory when it
                // reaches zero.
                drop(std::sync::Arc::from_raw(
                    private_data as *const std::cell::RefCell<V>,
                ));
            }
        }
    }
}
