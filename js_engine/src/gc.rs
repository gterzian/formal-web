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

use crate::{ExecutionContext, JsTypes, JsTypesWithRealm};

pub type UnrootAction<T> = Box<dyn FnOnce(&<T as JsTypes>::JsValue)>;

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
pub trait JsTypesGcExt: JsTypes + JsTypesWithRealm + Sized + 'static {
    /// The cycle-safe structural twin link.
    type Reflector: Clone + 'static;
    type Context: ExecutionContext<Self>;

    fn create_reflector(context: &mut Self::Context, obj: &Self::JsObject) -> Self::Reflector;
    fn upgrade_reflector(
        context: &mut Self::Context,
        reflector: &Self::Reflector,
    ) -> Option<Self::JsObject>;
}

/// Internal guard that executes the unroot action when dropped.
/// Shared across all clones of a GcRootHandle via Rc.
pub(crate) struct SharedUnroot<T: JsTypes> {
    value: T::JsValue,
    action: Option<UnrootAction<T>>,
}

impl<T: JsTypes> Drop for SharedUnroot<T> {
    fn drop(&mut self) {
        if let Some(action) = self.action.take() {
            action(&self.value);
        }
    }
}

/// An RAII guard that unroots a protected JS value when the last clone is dropped.
pub struct GcRootHandle<T: JsTypes> {
    /// The rooted JS value. Callers can read this to pass the value
    /// to trait methods like `EcmascriptHost::call`.
    pub value: T::JsValue,
    /// Shared reference to the unrooting logic.
    /// On Boa this is always None. On JSC it holds the unprotect action.
    guard: Option<std::rc::Rc<SharedUnroot<T>>>,
}

impl<T: JsTypes> GcRootHandle<T> {
    /// Creates a new root handle.
    pub fn new(value: T::JsValue, unroot_action: Option<UnrootAction<T>>) -> Self {
        let guard = unroot_action.map(|action| {
            std::rc::Rc::new(SharedUnroot {
                value: value.clone(),
                action: Some(action),
            })
        });
        Self { value, guard }
    }
}

impl<T: JsTypes> Clone for GcRootHandle<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            // Bumping the Rc count safely shares the unroot action across clones.
            guard: self.guard.clone(),
        }
    }
}

// No custom Drop needed — standard drop glue drops the Option<Rc>,
// which decrements the count and triggers SharedUnroot::drop at zero.

// ============================================================================
// SECTION III: BACKEND-ABSTRACTED GC CELL
// ============================================================================

// ── ProtectedCell: auto-protect/unprotect JsValue/JsObject on set ─────────
//
// On Boa: GcCell<JsValue> already traces through `#[derive(Trace)]` — no
// explicit protection needed.  JsValueCell is just GcCell<JsValue>.
//
// On JSC: JsValue/JsObject references stored behind GcCell (Rc<RefCell>)
// are invisible to JSC's GC.  JsValueCell wraps the inner value with
// JSValueProtect on set and JSValueUnprotect on replacement.
//
// Content code uses these as drop-in replacements for GcCell<JsValue> and
// GcCell<Option<JsObject>>, calling set() instead of *borrow_mut() =.

// Boa: type aliases are sufficient — the Boa GC traces through GcCell.
#[cfg(feature = "boa")]
pub use boa_cells::*;

#[cfg(feature = "boa")]
mod boa_cells {
    /// Auto-protecting cell for a single JsValue.
    /// On Boa, set() delegates to GcCell mutation (GC traces automatically).
    #[derive(boa_gc::Trace, boa_gc::Finalize)]
    pub struct JsValueCell(boa_gc::Gc<boa_gc::GcRefCell<boa_engine::JsValue>>);

    /// Auto-protecting cell for an optional JsObject.
    #[derive(boa_gc::Trace, boa_gc::Finalize)]
    pub struct JsObjectCell(boa_gc::Gc<boa_gc::GcRefCell<Option<boa_engine::JsObject>>>);

    impl JsValueCell {
        pub fn new(val: boa_engine::JsValue) -> Self {
            JsValueCell(boa_gc::Gc::new(boa_gc::GcRefCell::new(val)))
        }

        pub fn set(&self, val: boa_engine::JsValue) {
            *self.0.borrow_mut() = val;
        }

        pub fn borrow(&self) -> boa_gc::GcRef<'_, boa_engine::JsValue> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> boa_gc::GcRefMut<'_, boa_engine::JsValue> {
            self.0.borrow_mut()
        }
    }

    impl Clone for JsValueCell {
        fn clone(&self) -> Self {
            JsValueCell(self.0.clone())
        }
    }

    impl JsObjectCell {
        pub fn new(val: Option<boa_engine::JsObject>) -> Self {
            JsObjectCell(boa_gc::Gc::new(boa_gc::GcRefCell::new(val)))
        }

        pub fn set(&self, val: Option<boa_engine::JsObject>) {
            *self.0.borrow_mut() = val;
        }

        pub fn borrow(&self) -> boa_gc::GcRef<'_, Option<boa_engine::JsObject>> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> boa_gc::GcRefMut<'_, Option<boa_engine::JsObject>> {
            self.0.borrow_mut()
        }
    }

    impl Clone for JsObjectCell {
        fn clone(&self) -> Self {
            JsObjectCell(self.0.clone())
        }
    }
}

// JSC: actual struct with auto-protect/unprotect
#[cfg(feature = "jsc")]
pub use jsc_cells::*;

#[cfg(feature = "jsc")]
mod jsc_cells {
    use crate::jsc::{JscObject, JscValue};
    use crate::jsc_sys;

    /// Auto-protecting cell for a single JsValue.
    /// Use `set(val)` to assign (handles protect/unprotect).
    /// Use `borrow()` / `borrow_mut()` for read access or in-place mutation.
    pub struct JsValueCell(std::rc::Rc<std::cell::RefCell<JscValue>>);

    /// Auto-protecting cell for an optional JsObject.
    /// Use `set(val)` to assign (handles protect/unprotect).
    pub struct JsObjectCell(std::rc::Rc<std::cell::RefCell<Option<JscObject>>>);

    unsafe fn protect(val: &JscValue) {
        let js_type = if val.ctx().is_null() {
            return;
        } else {
            // SAFETY: `val.ctx()` is non-null, checked above.
            unsafe { jsc_sys::JSValueGetType(val.ctx(), val.raw) }
        };
        // JSValueProtect works on any GC-managed heap value: objects,
        // symbols (kJSTypeSymbol), and bigints (kJSTypeBigInt).  Only
        // primitive values (undefined, null, boolean, number, string)
        // are stack-allocated and need no protection.
        match js_type {
            crate::jsc_sys::JSType::kJSTypeObject
            | crate::jsc_sys::JSType::kJSTypeSymbol
            | crate::jsc_sys::JSType::kJSTypeBigInt => unsafe {
                jsc_sys::JSValueProtect(val.ctx(), val.raw);
            },
            _ => {}
        }
    }

    unsafe fn unprotect(val: &JscValue) {
        let js_type = if val.ctx().is_null() {
            return;
        } else {
            // SAFETY: `val.ctx()` is non-null, checked above.
            unsafe { jsc_sys::JSValueGetType(val.ctx(), val.raw) }
        };
        match js_type {
            crate::jsc_sys::JSType::kJSTypeObject
            | crate::jsc_sys::JSType::kJSTypeSymbol
            | crate::jsc_sys::JSType::kJSTypeBigInt => unsafe {
                jsc_sys::JSValueUnprotect(val.ctx(), val.raw);
            },
            _ => {}
        }
    }

    impl JsValueCell {
        pub fn new(val: JscValue) -> Self {
            unsafe {
                protect(&val);
            }
            JsValueCell(std::rc::Rc::new(std::cell::RefCell::new(val)))
        }

        pub fn set(&self, val: JscValue) {
            let mut slot = self.0.borrow_mut();
            unsafe {
                unprotect(&slot);
            }
            unsafe {
                protect(&val);
            }
            *slot = val;
        }

        pub fn borrow(&self) -> std::cell::Ref<'_, JscValue> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> std::cell::RefMut<'_, JscValue> {
            self.0.borrow_mut()
        }
    }

    impl Drop for JsValueCell {
        fn drop(&mut self) {
            // Unprotect the inner value when the last reference is dropped.
            // Use try_borrow to avoid panicking if the cell is already borrowed
            // (e.g. during cycle teardown or panic recovery).
            if std::rc::Rc::strong_count(&self.0) == 1 {
                if let Ok(val) = self.0.try_borrow() {
                    unsafe {
                        unprotect(&*val);
                    }
                }
            }
        }
    }

    impl Clone for JsValueCell {
        fn clone(&self) -> Self {
            // Share the Rc reference — interior mutability must be preserved.
            Self(self.0.clone())
        }
    }

    impl JsObjectCell {
        pub fn new(val: Option<JscObject>) -> Self {
            if let Some(ref obj) = val {
                let v = JscValue::from(obj.clone());
                unsafe {
                    protect(&v);
                }
            }
            JsObjectCell(std::rc::Rc::new(std::cell::RefCell::new(val)))
        }

        pub fn set(&self, val: Option<JscObject>) {
            let mut slot = self.0.borrow_mut();
            if let Some(ref old) = *slot {
                let ov = JscValue::from(old.clone());
                unsafe {
                    unprotect(&ov);
                }
            }
            if let Some(ref new) = val {
                let nv = JscValue::from(new.clone());
                unsafe {
                    protect(&nv);
                }
            }
            *slot = val;
        }

        pub fn borrow(&self) -> std::cell::Ref<'_, Option<JscObject>> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> std::cell::RefMut<'_, Option<JscObject>> {
            self.0.borrow_mut()
        }
    }

    impl Drop for JsObjectCell {
        fn drop(&mut self) {
            // Unprotect the inner value when the last reference is dropped.
            if std::rc::Rc::strong_count(&self.0) == 1 {
                if let Ok(val) = self.0.try_borrow() {
                    if let Some(obj) = &*val {
                        let v = JscValue::from(obj.clone());
                        unsafe {
                            unprotect(&v);
                        }
                    }
                }
            }
        }
    }

    impl Clone for JsObjectCell {
        fn clone(&self) -> Self {
            // Share the Rc reference — interior mutability must be preserved.
            Self(self.0.clone())
        }
    }
}

#[cfg(feature = "v8")]
pub use v8_cells::*;

#[cfg(feature = "v8")]
mod v8_cells {
    use std::cell::{Ref, RefCell, RefMut};
    use std::rc::Rc;

    use crate::v8::{V8Object, V8Value};

    pub struct JsValueCell(Rc<RefCell<V8Value>>);

    pub struct JsObjectCell(Rc<RefCell<Option<V8Object>>>);

    impl JsValueCell {
        pub fn new(value: V8Value) -> Self {
            Self(Rc::new(RefCell::new(value)))
        }

        pub fn set(&self, value: V8Value) {
            *self.0.borrow_mut() = value;
        }

        pub fn borrow(&self) -> Ref<'_, V8Value> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> RefMut<'_, V8Value> {
            self.0.borrow_mut()
        }
    }

    impl Clone for JsValueCell {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl JsObjectCell {
        pub fn new(value: Option<V8Object>) -> Self {
            Self(Rc::new(RefCell::new(value)))
        }

        pub fn set(&self, value: Option<V8Object>) {
            *self.0.borrow_mut() = value;
        }

        pub fn borrow(&self) -> Ref<'_, Option<V8Object>> {
            self.0.borrow()
        }

        pub fn borrow_mut(&self) -> RefMut<'_, Option<V8Object>> {
            self.0.borrow_mut()
        }
    }

    impl Clone for JsObjectCell {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
}

/// A backend-abstracted GC-managed cell providing interior mutability.
///
/// On Boa this is a type alias for `boa_gc::Gc<boa_gc::GcRefCell<T>>` so
/// the GC traces through the reference. On JSC and V8 it is `Rc<RefCell<T>>`.
///
/// Use `gc_cell_new(val)` to construct, `.borrow()` / `.borrow_mut()` to
/// access the inner value.
#[cfg(feature = "boa")]
pub type GcCell<T> = boa_gc::Gc<boa_gc::GcRefCell<T>>;

#[cfg(any(feature = "jsc", feature = "v8"))]
pub type GcCell<T> = std::rc::Rc<std::cell::RefCell<T>>;

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "boa")]
pub fn gc_cell_new<T: boa_gc::Trace>(val: T) -> GcCell<T> {
    boa_gc::Gc::new(boa_gc::GcRefCell::new(val))
}

/// Construct a [`GcCell`] with the given value.
#[cfg(any(feature = "jsc", feature = "v8"))]
pub fn gc_cell_new<T>(val: T) -> GcCell<T> {
    std::rc::Rc::new(std::cell::RefCell::new(val))
}

/// Compare two [`GcCell`] references for pointer equality.
///
/// Returns `true` if both references point to the same GC-managed allocation.
/// On Boa this uses `Gc::ptr_eq`; on JSC and V8 it uses `Rc::ptr_eq`.
#[cfg(feature = "boa")]
pub fn gc_cell_ptr_eq<T: boa_gc::Trace + ?Sized>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    boa_gc::Gc::ptr_eq(a, b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(any(feature = "jsc", feature = "v8"))]
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
        type Context = crate::boa::BoaContext;

        fn create_reflector(_context: &mut Self::Context, obj: &Self::JsObject) -> Self::Reflector {
            obj.clone()
        }
        fn upgrade_reflector(
            _context: &mut Self::Context,
            reflector: &Self::Reflector,
        ) -> Option<Self::JsObject> {
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
#[cfg(feature = "jsc")]
mod jsc_gc_impl {
    use super::*;
    use crate::jsc::JscTypes;

    impl JsTypesGcExt for JscTypes {
        /// A (raw_object_ptr, context) pair so that `upgrade_reflector` can
        /// reconstruct a fully-valid `JscObject` with a non-null context.
        type Reflector = (*mut std::ffi::c_void, *mut crate::jsc_sys::JSContextRef);
        type Context = crate::jsc::JscEngine;

        fn create_reflector(_context: &mut Self::Context, obj: &Self::JsObject) -> Self::Reflector {
            (obj.as_raw() as *mut std::ffi::c_void, obj.ctx())
        }

        fn upgrade_reflector(
            _context: &mut Self::Context,
            reflector: &Self::Reflector,
        ) -> Option<Self::JsObject> {
            let (raw_ptr, ctx) = *reflector;
            if raw_ptr.is_null() || ctx.is_null() {
                None
            } else {
                Some(unsafe {
                    crate::jsc::JscObject::from_raw(
                        raw_ptr as *mut crate::jsc_sys::JSObjectRef,
                        ctx,
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

#[cfg(any(feature = "jsc", feature = "v8"))]
mod persistent_handle_trace_impls {
    use super::Trace;

    // Blanket Trace impls for common types used as captures with
    // `create_builtin_function`.
    unsafe impl Trace for () {}
    unsafe impl Trace for bool {}
    unsafe impl Trace for u64 {}
    unsafe impl Trace for i64 {}
    unsafe impl Trace for u32 {}
    unsafe impl Trace for i32 {}
    unsafe impl Trace for usize {}
    unsafe impl Trace for String {}
    // Bound on T ensures that only types whose inner value is itself GC-safe
    // can be wrapped in Rc<RefCell<T>>/Rc<Cell<T>> and held as a traced field.
    // This prevents raw JscValue/JscObject from being stored behind these
    // wrappers (they must use JsValueCell/JsObjectCell instead).
    unsafe impl<T: Trace> Trace for std::rc::Rc<std::cell::RefCell<T>> {}
    unsafe impl<T: Trace> Trace for std::rc::Rc<std::cell::Cell<T>> {}
    unsafe impl<A: Trace, B: Trace> Trace for (A, B) {}
    unsafe impl<A: Trace, B: Trace, C: Trace> Trace for (A, B, C) {}
    unsafe impl<A: Trace, B: Trace, C: Trace, D: Trace> Trace for (A, B, C, D) {}
    unsafe impl<A: Trace, B: Trace, C: Trace, D: Trace, E: Trace> Trace for (A, B, C, D, E) {}
}
