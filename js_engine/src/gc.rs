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
#[cfg(not(feature = "boa"))]
pub unsafe trait Trace {}

#[cfg(feature = "boa")]
pub unsafe trait Trace: boa_gc::Trace {}

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
    /// The rooted JS value.  Callers can read this to pass the value
    /// to trait methods like `EcmascriptHost::call`.
    pub value: T::JsValue,
    pub(crate) unroot_action: Option<Box<dyn FnOnce(&T::JsValue)>>,
}

impl<T: JsTypes> Drop for GcRootHandle<T> {
    fn drop(&mut self) {
        if let Some(action) = self.unroot_action.take() {
            action(&self.value);
        }
    }
}

impl<T: JsTypes> Clone for GcRootHandle<T> {
    fn clone(&self) -> Self {
        // Cloning a GcRootHandle creates a new root for the value.
        // On Boa this is a no-op (GC traces through Trace); on JSC
        // this adds a new global-object property via the unroot action.
        // Since unroot_action is a FnOnce (not Fn), we can't clone it —
        // we store None and rely on the engine's GC to keep the value
        // alive until the clone is dropped.
        #[cfg(feature = "boa")]
        {
            Self {
                value: self.value.clone(),
                unroot_action: None,
            }
        }
        #[cfg(not(feature = "boa"))]
        {
            // JSC: we can't clone the unroot action.  The clone will
            // be kept alive by the original handle's root as long as
            // it outlives the clone.  This matches the common pattern
            // where a clone is stored temporarily for a callback.
            Self {
                value: self.value.clone(),
                unroot_action: None,
            }
        }
    }
}

// ============================================================================
// SECTION III: BACKEND-ABSTRACTED GC CELL
// ============================================================================

/// A backend-abstracted GC-managed cell providing interior mutability.
///
/// On Boa this is a type alias for `boa_gc::Gc<boa_gc::GcRefCell<T>>` so
/// the GC traces through the reference.  On JSC it is `Rc<RefCell<T>>`.
///
/// Use `gc_cell_new(val)` to construct, `.borrow()` / `.borrow_mut()` to
/// access the inner value.
#[cfg(feature = "boa")]
pub type GcCell<T> = boa_gc::Gc<boa_gc::GcRefCell<T>>;

#[cfg(not(feature = "boa"))]
pub type GcCell<T> = std::rc::Rc<std::cell::RefCell<T>>;

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "boa")]
pub fn gc_cell_new<T: boa_gc::Trace>(val: T) -> GcCell<T> {
    boa_gc::Gc::new(boa_gc::GcRefCell::new(val))
}

/// Construct a [`GcCell`] with the given value.
#[cfg(not(feature = "boa"))]
pub fn gc_cell_new<T>(val: T) -> GcCell<T> {
    std::rc::Rc::new(std::cell::RefCell::new(val))
}

/// Compare two [`GcCell`] references for pointer equality.
///
/// Returns `true` if both references point to the same GC-managed allocation.
/// On Boa this uses `Gc::ptr_eq`; on JSC it uses `Rc::ptr_eq`.
#[cfg(feature = "boa")]
pub fn gc_cell_ptr_eq<T: boa_gc::Trace + ?Sized>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    boa_gc::Gc::ptr_eq(a, b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(not(feature = "boa"))]
pub fn gc_cell_ptr_eq<T>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    std::rc::Rc::ptr_eq(a, b)
}

// ============================================================================
// SECTION IV: GC-TRAIT MACRO
// ============================================================================

/// Declarative macro that derives the correct GC traits for a type
/// regardless of the active JS engine backend.
///
/// For structs: attaches `#[derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)]`
/// on Boa (or no-op Trace/Finalize impls on JSC).
///
/// For enums: attaches `#[derive(boa_gc::Finalize, boa_gc::Trace)]` without `JsData`,
/// since enums are not stored as platform objects.
///
/// Usage:
/// ```ignore
/// js_engine::impl_gc_traits! {
///     /// Optional doc comment.
///     pub(crate) struct MyWidget {
///         field: String,
///         callback: Option<GcRootHandle<TestTypes>>,
///     }
/// }
///
/// js_engine::impl_gc_traits! {
///     pub(crate) enum MyState {
///         Idle,
///         Active { count: u32 },
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_gc_traits {
    // Struct variant — includes JsData for platform-object storage.
    ($(#[$attr:meta])* $vis:vis struct $name:ident $(<$($generic:tt),+>)? { $($fields:tt)* }) => {
        $(#[$attr])*
        #[cfg_attr(
            feature = "boa",
            derive(boa_gc::Finalize, boa_gc::Trace, boa_engine::JsData)
        )]
        $vis struct $name $(<$($generic),+>)? {
            $($fields)*
        }

        #[cfg(not(feature = "boa"))]
        unsafe impl $(<$($generic),+>)? $crate::gc::Trace for $name $(<$($generic),+>)? {}

        #[cfg(not(feature = "boa"))]
        impl $(<$($generic),+>)? $crate::gc::Finalize for $name $(<$($generic),+>)? {}
    };

    // Enum variant — no JsData (enums aren't platform objects).
    ($(#[$attr:meta])* $vis:vis enum $name:ident $(<$($generic:tt),+>)? { $($variants:tt)* }) => {
        $(#[$attr])*
        #[cfg_attr(
            feature = "boa",
            derive(boa_gc::Finalize, boa_gc::Trace)
        )]
        $vis enum $name $(<$($generic),+>)? {
            $($variants)*
        }

        #[cfg(not(feature = "boa"))]
        unsafe impl $(<$($generic),+>)? $crate::gc::Trace for $name $(<$($generic),+>)? {}

        #[cfg(not(feature = "boa"))]
        impl $(<$($generic),+>)? $crate::gc::Finalize for $name $(<$($generic),+>)? {}
    };
}

// ============================================================================
// SECTION V: ENGINE-SPECIFIC IMPLEMENTATIONS
// ============================================================================

// ── Boa backend ───────────────────────────────────────────────────────────
#[cfg(feature = "boa")]
mod boa_gc_impl {
    use super::*;
    use crate::boa::BoaTypes;

    // SAFETY: `boa_gc::Trace` satisfies all the requirements of
    // `js_engine::gc::Trace` — both guarantee that every GC-reachable
    // field is visited during trace.
    unsafe impl<T: boa_gc::Trace> Trace for T {}

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

    // SAFETY: GcRootHandle wraps a JsValue which implements boa_gc::Trace.
    // We delegate tracing to the inner value so that structs containing
    // GcRootHandle fields (e.g. on_change callbacks) are properly traced.
    unsafe impl boa_gc::Trace for super::GcRootHandle<BoaTypes> {
        unsafe fn trace(&self, tracer: &mut boa_gc::Tracer) {
            unsafe {
                boa_gc::Trace::trace(&self.value, tracer);
            }
        }
        unsafe fn trace_non_roots(&self) {
            unsafe {
                boa_gc::Trace::trace_non_roots(&self.value);
            }
        }
        fn run_finalizer(&self) {
            boa_gc::Trace::run_finalizer(&self.value);
        }
    }

    impl boa_gc::Finalize for super::GcRootHandle<BoaTypes> {}
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

    #[allow(dead_code)]
    pub extern "C" fn jsc_generic_finalizer<V>(object: *mut std::ffi::c_void) {
        unsafe {
            let private_data =
                crate::jsc_sys::JSObjectGetPrivate(object as *mut crate::jsc_sys::JSObjectRef);
            if !private_data.is_null() {
                drop(std::sync::Arc::from_raw(
                    private_data as *const std::cell::RefCell<V>,
                ));
            }
        }
    }
}
