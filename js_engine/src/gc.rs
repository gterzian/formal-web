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

/// A backend-abstracted GC-managed cell providing interior mutability.
///
/// On Boa this is a type alias for `boa_gc::Gc<boa_gc::GcRefCell<T>>` so
/// the GC traces through the reference.  On JSC it is a struct: an
/// `Rc<RefCell<T>>` slot plus managed-reference edges (JSManagedValue +
/// `addManagedReference:withOwner:`) that keep the JS values directly
/// inside `T` alive; the edges' owner is the per-context anchor by
/// default, and is re-pointed to a per-object owner when the containing
/// platform object is created (see [`GcOwner`]).  On V8 it is
/// `Rc<RefCell<T>>`.
///
/// Use `gc_cell_new(val)` to construct, `.borrow()` / `.borrow_mut()` to
/// access the inner value, and [`GcCellSet::set`] to replace the value —
/// on JSC replacing a value re-registers the managed edges for the new
/// value.  In-place mutation of a `GcCell<gc_struct>`'s fields via
/// `borrow_mut` does *not* re-register edges (see the JSC README, "GC
/// integration design").
#[cfg(feature = "boa")]
pub type GcCell<T> = boa_gc::Gc<boa_gc::GcRefCell<T>>;

#[cfg(feature = "jsc")]
pub use jsc_gc_cell::GcCell;

// TODO(v8): Move platform-object ownership to a per-isolate `cppgc::Heap` and
// replace off-heap roots with traced `Member`/`WeakMember` edges. This requires
// changing context-free cell construction and borrowing before this alias can
// use rusty_v8's cppgc types safely.
#[cfg(feature = "v8")]
pub type GcCell<T> = std::rc::Rc<std::cell::RefCell<T>>;

/// Replacing the value inside a [`GcCell`].  On JSC the managed-reference
/// edges are re-registered for the new value (and the old value's edges
/// removed); on Boa and V8 this is a plain `*borrow_mut() = value`.
pub trait GcCellSet<T> {
    fn set(&self, value: T);

    /// Re-register the managed-reference edges after in-place mutation of
    /// the cell's contents via `borrow_mut` (e.g. pushing onto a
    /// `GcCell<Vec<T>>` or assigning a field of a `GcCell<gc_struct>`).
    /// On JSC the edges are re-extracted from the current contents; on
    /// Boa and V8 this is a no-op.
    fn sync(&self) {}
}

#[cfg(feature = "boa")]
impl<T: boa_engine::Trace> GcCellSet<T> for boa_gc::Gc<boa_gc::GcRefCell<T>> {
    fn set(&self, value: T) {
        *self.borrow_mut() = value;
    }
}

#[cfg(feature = "v8")]
impl<T> GcCellSet<T> for std::rc::Rc<std::cell::RefCell<T>> {
    fn set(&self, value: T) {
        *self.borrow_mut() = value;
    }
}

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "boa")]
pub fn gc_cell_new<T: boa_gc::Trace>(val: T) -> GcCell<T> {
    boa_gc::Gc::new(boa_gc::GcRefCell::new(val))
}

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "jsc")]
pub fn gc_cell_new<T: GcTraceable>(val: T) -> GcCell<T> {
    GcCell::new(val)
}

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "v8")]
pub fn gc_cell_new<T>(val: T) -> GcCell<T> {
    std::rc::Rc::new(std::cell::RefCell::new(val))
}

/// Compare two [`GcCell`] references for pointer equality.
///
/// Returns `true` if both references point to the same GC-managed allocation.
/// On Boa this uses `Gc::ptr_eq`; on JSC and V8 it uses `Rc::ptr_eq` on the
/// underlying slot.
#[cfg(feature = "boa")]
pub fn gc_cell_ptr_eq<T: boa_gc::Trace + ?Sized>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    boa_gc::Gc::ptr_eq(a, b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(feature = "jsc")]
pub fn gc_cell_ptr_eq<T: GcTraceable>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    GcCell::ptr_eq(a, b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(feature = "v8")]
pub fn gc_cell_ptr_eq<T>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    std::rc::Rc::ptr_eq(a, b)
}

// ── GC owners and platform-object adoption (non-Boa) ─────────────────────

/// Handle to a GC owner for managed-reference edges.  On JSC it wraps the
/// per-object `NSObject` exported on a platform object's reflector (see
/// `JscGcOwner`); on V8 it is unused (no managed edges exist).
#[cfg(not(feature = "boa"))]
#[derive(Clone)]
pub struct GcOwnerRef {
    #[cfg(feature = "jsc")]
    pub(crate) jsc: std::rc::Rc<crate::jsc::JscGcOwner>,
}

#[cfg(not(feature = "boa"))]
impl GcOwnerRef {
    #[cfg(feature = "jsc")]
    pub(crate) fn jsc(owner: crate::jsc::JscGcOwner) -> Self {
        Self {
            jsc: std::rc::Rc::new(owner),
        }
    }

    #[cfg(feature = "jsc")]
    pub(crate) fn as_jsc(&self) -> &crate::jsc::JscGcOwner {
        &self.jsc
    }
}

/// Adopt a platform object's cells onto its per-object GC owner.
///
/// Implemented by [`GcCell`] (re-points the cell's own managed edges) and
/// generated by `#[gc_struct]` for composite types (delegates to the
/// `GcCell`-typed fields, skipping `#[ignore_trace]` fields).  Called by
/// `create_interface_instance` once the reflector exists, so a struct's
/// JS-value fields stay alive exactly while the struct's JS object is
/// reachable from JS.  No-op on V8 (no managed edges exist).
#[cfg(not(feature = "boa"))]
pub trait GcOwner {
    fn adopt_gc_owner(&mut self, _owner: &GcOwnerRef) {}
}

#[cfg(not(feature = "boa"))]
impl<T: GcOwner> GcOwner for Option<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        if let Some(inner) = self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(not(feature = "boa"))]
impl<T: GcOwner> GcOwner for Vec<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        for inner in self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(not(feature = "boa"))]
impl<T: GcOwner> GcOwner for std::collections::VecDeque<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        for inner in self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(not(feature = "boa"))]
impl<T: GcOwner> GcOwner for Box<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        self.as_mut().adopt_gc_owner(owner);
    }
}

// Plain `Rc<RefCell<T>>` is not an adoption target on JSC (only `GcCell`
// structs hold managed edges); on V8 `GcCell<T>` *is* `Rc<RefCell<T>>`
// and has no edges to re-point.  No-op in both cases.
#[cfg(any(feature = "jsc", feature = "v8"))]
impl<T> GcOwner for std::rc::Rc<std::cell::RefCell<T>> {}

// ── JSC cell: Rc<RefCell<T>> slot plus managed-reference edges ──────────

#[cfg(feature = "jsc")]
pub use jsc_gc_cell::GcTraceable;

#[cfg(feature = "jsc")]
mod jsc_gc_cell {
    use std::cell::{Ref, RefCell, RefMut};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use crate::jsc::{
        JscBigInt, JscGcOwner, JscManagedValue, JscObject, JscString, JscSymbol, JscValue,
    };

    /// Enumerates the JS values *directly* held by a type, for
    /// managed-reference edge registration.
    ///
    /// Values inside nested [`GcCell`]s are NOT enumerated: each cell
    /// registers (and owns) the edges for its own values, and the value
    /// graph can be cyclic *through* cells (e.g. a stream's controller
    /// cell holds a stream that holds the controller), so recursing into
    /// cells would loop forever.
    pub trait GcTraceable {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue));
    }

    impl GcTraceable for JscValue {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            visit(self);
        }
    }

    impl GcTraceable for JscObject {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            visit(&self.as_value());
        }
    }

    impl GcTraceable for JscSymbol {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            visit(self.as_value());
        }
    }

    impl GcTraceable for JscBigInt {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            visit(self.as_value());
        }
    }

    impl<T: GcTraceable> GcTraceable for Option<T> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            if let Some(inner) = self {
                inner.visit_js_values(visit);
            }
        }
    }

    impl<T: GcTraceable> GcTraceable for Vec<T> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            for inner in self {
                inner.visit_js_values(visit);
            }
        }
    }

    impl<T: GcTraceable> GcTraceable for VecDeque<T> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            for inner in self {
                inner.visit_js_values(visit);
            }
        }
    }

    impl<T: GcTraceable> GcTraceable for Box<T> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.as_ref().visit_js_values(visit);
        }
    }

    impl<A: GcTraceable, B: GcTraceable> GcTraceable for (A, B) {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.0.visit_js_values(visit);
            self.1.visit_js_values(visit);
        }
    }

    impl<A: GcTraceable, B: GcTraceable, C: GcTraceable> GcTraceable for (A, B, C) {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.0.visit_js_values(visit);
            self.1.visit_js_values(visit);
            self.2.visit_js_values(visit);
        }
    }

    impl<A: GcTraceable, B: GcTraceable, C: GcTraceable, D: GcTraceable> GcTraceable for (A, B, C, D) {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.0.visit_js_values(visit);
            self.1.visit_js_values(visit);
            self.2.visit_js_values(visit);
            self.3.visit_js_values(visit);
        }
    }

    impl<A: GcTraceable, B: GcTraceable, C: GcTraceable, D: GcTraceable, E: GcTraceable> GcTraceable
        for (A, B, C, D, E)
    {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.0.visit_js_values(visit);
            self.1.visit_js_values(visit);
            self.2.visit_js_values(visit);
            self.3.visit_js_values(visit);
            self.4.visit_js_values(visit);
        }
    }

    // No-op for non-JS-value types.
    macro_rules! impl_no_values {
        ($($t:ty),* $(,)?) => {
            $(impl GcTraceable for $t {
                fn visit_js_values(&self, _visit: &mut dyn FnMut(&JscValue)) {}
            })*
        };
    }
    impl_no_values!(
        (),
        bool,
        u8,
        u16,
        u32,
        u64,
        usize,
        i8,
        i16,
        i32,
        i64,
        isize,
        f32,
        f64,
        char,
        String,
        JscString,
    );

    /// Whether managed-reference edges are enabled (default: disabled).
    /// The system JavaScriptCore's GC crashes when many managed values
    /// are registered during heavy streams tests (see the JSC README),
    /// so the edges are opt-in via `FORMAL_WEB_GC_EDGES=1`.
    fn gc_edges_enabled() -> bool {
        use std::sync::OnceLock;
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("FORMAL_WEB_GC_EDGES").is_some())
    }

    /// The JSC unified cell: an `Rc<RefCell<T>>` slot plus managed edges.
    ///
    /// The edges are registered for the JS values directly inside `T` at
    /// construction (against the per-context anchor), replaced on
    /// [`super::GcCellSet::set`], and re-pointed to a per-object owner by
    /// [`super::GcOwner::adopt_gc_owner`].
    pub struct GcCell<T: GcTraceable> {
        slot: Rc<RefCell<T>>,
        edges: Rc<RefCell<Vec<JscManagedValue>>>,
        owner: Rc<RefCell<Option<JscGcOwner>>>,
    }

    impl<T: GcTraceable> GcCell<T> {
        /// Create a cell; if managed edges are enabled, the JS values
        /// directly inside `val` are kept alive via managed references
        /// against the per-context anchor (until the cell is adopted onto
        /// a per-object owner).  Edges are disabled by default — they are
        /// unstable on the system JavaScriptCore (see the JSC README).
        pub(crate) fn new(val: T) -> Self {
            let ctx = if gc_edges_enabled() {
                crate::jsc::current_engine_context()
            } else {
                std::ptr::null_mut()
            };
            let owner = if ctx.is_null() {
                None
            } else {
                Some(JscGcOwner::anchor(ctx))
            };
            let cell = Self {
                slot: Rc::new(RefCell::new(val)),
                edges: Rc::new(RefCell::new(Vec::new())),
                owner: Rc::new(RefCell::new(owner)),
            };
            cell.rebuild_edges();
            cell
        }

        /// Compare two cells for slot identity.
        pub(crate) fn ptr_eq(a: &Self, b: &Self) -> bool {
            Rc::ptr_eq(&a.slot, &b.slot)
        }

        pub fn borrow(&self) -> Ref<'_, T> {
            self.slot.borrow()
        }

        pub fn borrow_mut(&self) -> RefMut<'_, T> {
            self.slot.borrow_mut()
        }

        /// Re-register the managed edges from the values currently in the
        /// slot (dropping the old edges first) under the current owner.
        ///
        /// Managed edges are unstable on the system JavaScriptCore (they
        /// crash its GC during heavy streams tests — see the JSC README),
        /// so they are disabled by default; `FORMAL_WEB_GC_EDGES=1`
        /// enables them for experimentation.
        fn rebuild_edges(&self) {
            let mut edges = self.edges.borrow_mut();
            // Dropping the old JscManagedValues removes their edges.
            edges.clear();
            if !gc_edges_enabled() {
                return;
            }
            let owner_binding = self.owner.borrow();
            let Some(owner) = owner_binding.as_ref() else {
                return;
            };
            let slot = self.slot.borrow();
            slot.visit_js_values(&mut |value| {
                if let Some(managed) = JscManagedValue::new(value.ctx(), value.as_raw(), owner) {
                    edges.push(managed);
                }
            });
        }

        /// Re-point the cell's edges to `owner` and re-register them.
        fn adopt(&self, owner: &JscGcOwner) {
            *self.owner.borrow_mut() = Some(owner.clone());
            self.rebuild_edges();
        }
    }

    impl<T: GcTraceable> Clone for GcCell<T> {
        fn clone(&self) -> Self {
            Self {
                slot: self.slot.clone(),
                edges: self.edges.clone(),
                owner: self.owner.clone(),
            }
        }
    }

    impl<T: GcTraceable + Default> Default for GcCell<T> {
        fn default() -> Self {
            Self::new(T::default())
        }
    }

    // A GcCell is opaque to outer cells: it manages its own edges.
    impl<T: GcTraceable> GcTraceable for GcCell<T> {
        fn visit_js_values(&self, _visit: &mut dyn FnMut(&JscValue)) {}
    }

    impl<T: GcTraceable> super::GcCellSet<T> for GcCell<T> {
        fn set(&self, value: T) {
            *self.slot.borrow_mut() = value;
            self.rebuild_edges();
        }

        fn sync(&self) {
            self.rebuild_edges();
        }
    }

    impl<T: GcTraceable> super::GcOwner for GcCell<T> {
        fn adopt_gc_owner(&mut self, owner: &super::GcOwnerRef) {
            self.adopt(owner.as_jsc());
        }
    }

    // GcRootHandle and PromiseResolvers hold raw JS values; enumerate
    // them so cells containing these types register edges for them.
    impl GcTraceable for crate::gc::GcRootHandle<crate::jsc::JscTypes> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.value.visit_js_values(visit);
        }
    }

    impl GcTraceable for crate::records::PromiseResolvers<crate::jsc::JscTypes> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.resolve.visit_js_values(visit);
            self.reject.visit_js_values(visit);
        }
    }
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
