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
//! | [`GcCell`] | Unified GC-managed cell with interior mutability |
//!
//! Each backend provides its own implementations inside `#[cfg]`-gated
//! sub-modules below.

use crate::{ExecutionContext, JsTypes, JsTypesWithRealm};

#[cfg(feature = "boa")]
use crate::boa::BoaTypes;
#[cfg(feature = "jsc")]
use crate::jsc::JscTypes;
#[cfg(feature = "v8")]
use crate::v8::V8Types;

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
#[cfg(feature = "v8")]
pub unsafe trait Trace {
    /// Visit every cppgc edge (`TracedReference` to a JS object, nested
    /// `GcCell` `Member`) reachable from this value during marking.
    ///
    /// # Safety
    ///
    /// Implementations must visit every edge exactly once; missing an edge
    /// leaves a dangling pointer in the cppgc heap once the referent is
    /// collected. The visitor is only valid during stop-the-world marking on
    /// the isolate thread.
    unsafe fn trace(&self, visitor: &mut crate::v8_gc::Visitor);

    /// Convert every rooted JS handle inside this value into a cppgc edge.
    ///
    /// Called when the value is stored into traced storage (a `GcCell`, or a
    /// traced platform-object field through the engine's store helpers): a
    /// `v8::Global` root would keep the referent alive unconditionally, while
    /// a `TracedReference` edge keeps it alive only while the owning heap
    /// object is traced — which is what lets cycles spanning the JS heap and
    /// the cppgc heap be collected.
    fn store(&mut self, ec: &mut dyn crate::ExecutionContext<crate::v8::V8Types>);
}

#[cfg(feature = "boa")]
pub unsafe trait Trace: boa_gc::Trace {}

#[cfg(all(not(feature = "boa"), not(feature = "v8")))]
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
// SECTION III: UNIFIED GC CELL
// ============================================================================

// ── Boa backend ────────────────────────────────────────────────────────────
//
// `GcCell<T>` is `Gc<GcRefCell<T>>`: Boa's GC traces through the pointer and
// `GcRefCell` provides the runtime borrow checks. The execution context is
// accepted for API uniformity with engines whose cells need isolate-scoped
// access proof, and is otherwise unused.
#[cfg(feature = "boa")]
pub use boa_cells::*;

#[cfg(feature = "boa")]
mod boa_cells {
    use super::*;
    use crate::boa::BoaTypes;

    /// Unified GC-managed cell providing interior mutability.
    ///
    /// Construction and access take the execution context so the API is
    /// uniform across engines; on Boa the context is unused because
    /// `Gc<GcRefCell<T>>` is traced and borrow-checked by the engine GC.
    #[derive(Clone)]
    pub struct GcCell<T: boa_gc::Trace + 'static>(pub(crate) boa_gc::Gc<boa_gc::GcRefCell<T>>);

    /// Construct a [`GcCell`] with the given value.
    pub fn gc_cell_new<T: boa_gc::Trace + 'static>(
        value: T,
        _ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> GcCell<T> {
        GcCell(boa_gc::Gc::new(boa_gc::GcRefCell::new(value)))
    }

    impl<T: boa_gc::Trace + 'static> GcCell<T> {
        /// Immutably borrow the wrapped value.
        pub fn borrow<'a, 'e>(&'a self, _ec: &'e dyn ExecutionContext<BoaTypes>) -> GcRef<'a, T> {
            self.0.borrow()
        }

        /// Mutably borrow the wrapped value.
        pub fn borrow_mut<'a, 'e>(
            &'a self,
            _ec: &'e mut dyn ExecutionContext<BoaTypes>,
        ) -> GcRefMut<'a, T> {
            self.0.borrow_mut()
        }

        /// Replace the wrapped value.
        pub fn set<'a, 'e>(&'a self, value: T, _ec: &'e mut dyn ExecutionContext<BoaTypes>) {
            *self.0.borrow_mut() = value;
        }

        /// Compare two cells for pointer equality.
        pub fn ptr_eq(&self, other: &Self) -> bool {
            boa_gc::Gc::ptr_eq(&self.0, &other.0)
        }
    }

    // SAFETY: Delegates to the inner `Gc<GcRefCell<T>>`, which visits the
    // wrapped value during marking exactly like any other `Gc` field.
    unsafe impl<T: boa_gc::Trace + 'static> boa_gc::Trace for GcCell<T> {
        unsafe fn trace(&self, tracer: &mut boa_gc::Tracer) {
            unsafe {
                self.0.trace(tracer);
            }
        }

        unsafe fn trace_non_roots(&self) {
            unsafe {
                self.0.trace_non_roots();
            }
        }

        fn run_finalizer(&self) {
            self.0.run_finalizer();
        }
    }

    impl<T: boa_gc::Trace + 'static> boa_gc::Finalize for GcCell<T> {}

    pub type GcRef<'a, T> = boa_gc::GcRef<'a, T>;
    pub type GcRefMut<'a, T> = boa_gc::GcRefMut<'a, T>;
}

// ── JSC backend ────────────────────────────
//
// `GcCell<T>` is an `Rc<RefCell<T>>` slot plus managed-reference edges.  JSC's
// GC does not observe Rust-side references, so every JS value stored in a cell
// is wrapped in a `JSManagedValue` whose owner is the realm anchor by default
// and the platform object's exported holder once the object exists (see
// `GcOwner`).  The holder owns the platform data and therefore the cell, so
// the edges are removed when the holder (its JS wrapper) dies.
#[cfg(feature = "jsc")]
pub use jsc_cells::*;

#[cfg(feature = "jsc")]
mod jsc_cells {
    use std::cell::{Ref, RefCell, RefMut};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;
    use crate::jsc::{
        JscBigInt, JscEngine, JscGcOwnerRef, JscManagedValue, JscObject, JscString, JscSymbol,
        JscTypes, JscValue,
    };

    /// Enumerates the JS values *directly* held by a type, for
    /// managed-reference edge registration.
    ///
    /// Values inside nested [`GcCell`]s are not enumerated: each cell
    /// registers (and owns) the edges for its own values, and the value graph
    /// can be cyclic through cells.
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

    // Types that hold JS values behind their own fields enumerate them so
    // cells containing them register edges for those values.
    impl GcTraceable for super::GcRootHandle<JscTypes> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.value.visit_js_values(visit);
        }
    }

    impl GcTraceable for crate::records::PromiseResolvers<JscTypes> {
        fn visit_js_values(&self, visit: &mut dyn FnMut(&JscValue)) {
            self.resolve.visit_js_values(visit);
            self.reject.visit_js_values(visit);
        }
    }

    /// The unified JSC cell: an `Rc<RefCell<T>>` slot plus managed-reference
    /// edges for the JS values directly inside `T`.
    pub struct GcCell<T: GcTraceable> {
        slot: Rc<RefCell<T>>,
        edges: Rc<RefCell<Vec<JscManagedValue>>>,
        owner: Rc<RefCell<Option<JscGcOwnerRef>>>,
    }

    impl<T: GcTraceable> GcCell<T> {
        fn with_owner(value: T, owner: Option<JscGcOwnerRef>) -> Self {
            let cell = Self {
                slot: Rc::new(RefCell::new(value)),
                edges: Rc::new(RefCell::new(Vec::new())),
                owner: Rc::new(RefCell::new(owner)),
            };
            cell.rebuild_edges();
            cell
        }

        /// Re-register the managed edges for the values currently in the
        /// slot, dropping the old edges first.
        fn rebuild_edges(&self) {
            let mut edges = self.edges.borrow_mut();
            edges.clear();
            let owner = self.owner.borrow();
            let Some(owner) = owner.as_ref() else {
                return;
            };
            let slot = self.slot.borrow();
            slot.visit_js_values(&mut |value| {
                if let Some(managed) = owner.create_managed_value(value) {
                    edges.push(managed);
                }
            });
        }

        /// Re-point the cell's edges to the platform object's holder and
        /// re-register them.
        pub(crate) fn adopt(&self, owner: &JscGcOwnerRef) {
            *self.owner.borrow_mut() = Some(owner.clone());
            self.rebuild_edges();
        }

        /// Immutably borrow the wrapped value.
        pub fn borrow<'a, 'e>(&'a self, _ec: &'e dyn ExecutionContext<JscTypes>) -> GcRef<'a, T> {
            self.slot.borrow()
        }

        /// Mutably borrow the wrapped value.
        pub fn borrow_mut<'a, 'e>(
            &'a self,
            _ec: &'e mut dyn ExecutionContext<JscTypes>,
        ) -> GcRefMut<'a, T> {
            self.slot.borrow_mut()
        }

        /// Replace the wrapped value and re-register the managed edges.
        pub fn set<'a, 'e>(&'a self, value: T, _ec: &'e mut dyn ExecutionContext<JscTypes>) {
            *self.slot.borrow_mut() = value;
            self.rebuild_edges();
        }

        /// Re-register the managed edges after in-place mutation of the
        /// cell's contents through [`borrow_mut`](Self::borrow_mut).
        pub fn sync(&self) {
            self.rebuild_edges();
        }

        /// Compare two cells for pointer equality.
        pub fn ptr_eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.slot, &other.slot)
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
            Self::with_owner(T::default(), None)
        }
    }

    // A GcCell is opaque to outer cells: it manages its own edges.
    impl<T: GcTraceable> GcTraceable for GcCell<T> {
        fn visit_js_values(&self, _visit: &mut dyn FnMut(&JscValue)) {}
    }

    impl<T: GcTraceable> GcOwner for GcCell<T> {
        fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
            self.adopt(owner.as_jsc());
        }
    }

    /// Construct a [`GcCell`], registering managed edges against the realm
    /// anchor until the containing platform object adopts the cell onto its
    /// own holder (see [`GcOwner`]).
    pub fn gc_cell_new<T: GcTraceable>(
        value: T,
        ec: &mut dyn ExecutionContext<JscTypes>,
    ) -> GcCell<T> {
        let owner = ec
            .as_any()
            .downcast_ref::<JscEngine>()
            .and_then(JscEngine::realm_gc_owner_ref);
        GcCell::with_owner(value, owner)
    }

    pub type GcRef<'a, T> = Ref<'a, T>;
    pub type GcRefMut<'a, T> = RefMut<'a, T>;
}

// ── GC owners and platform-object adoption ──────────────────────────────

// On JSC a platform object adopts the cells it owns onto the managed-reference
// owner of its JS wrapper.  On Boa and V8 there are no managed edges, so every
// type satisfies the trait through a blanket implementation.

/// Handle to the managed-reference owner a platform object's cells adopt.
#[cfg(feature = "jsc")]
#[derive(Clone)]
pub struct GcOwnerRef {
    pub(crate) jsc: crate::jsc::JscGcOwnerRef,
}

#[cfg(feature = "jsc")]
impl GcOwnerRef {
    pub(crate) fn jsc(owner: crate::jsc::JscGcOwnerRef) -> Self {
        Self { jsc: owner }
    }

    pub(crate) fn as_jsc(&self) -> &crate::jsc::JscGcOwnerRef {
        &self.jsc
    }
}

/// Adopt a platform object's [`GcCell`] fields onto its per-object GC owner.
///
/// Implemented by [`GcCell`] (re-points the cell's managed edges) and generated
/// by `#[gc_struct]` for composite types (delegates to the `GcCell`-typed
/// fields, skipping `#[ignore_trace]` fields).  Called once the reflector
/// exists, so a struct's JS-value fields stay alive exactly while the struct's
/// JS object is reachable.  No-op on V8 (no managed edges exist).
#[cfg(feature = "jsc")]
pub trait GcOwner {
    fn adopt_gc_owner(&mut self, _owner: &GcOwnerRef) {}
}

#[cfg(feature = "jsc")]
impl<T: GcOwner> GcOwner for Option<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        if let Some(inner) = self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(feature = "jsc")]
impl<T: GcOwner> GcOwner for Vec<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        for inner in self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(feature = "jsc")]
impl<T: GcOwner> GcOwner for std::collections::VecDeque<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        for inner in self {
            inner.adopt_gc_owner(owner);
        }
    }
}

#[cfg(feature = "jsc")]
impl<T: GcOwner> GcOwner for Box<T> {
    fn adopt_gc_owner(&mut self, owner: &GcOwnerRef) {
        self.as_mut().adopt_gc_owner(owner);
    }
}

// Plain `Rc<RefCell<T>>` is not an adoption target (only `GcCell` structs hold
// managed edges).  No-op.
#[cfg(feature = "jsc")]
impl<T> GcOwner for std::rc::Rc<std::cell::RefCell<T>> {}

/// Adopt a platform object's [`GcCell`] fields onto its per-object GC owner.
/// Boa and V8 have no managed edges, so every type can adopt as a no-op.
#[cfg(not(feature = "jsc"))]
pub trait GcOwner {
    fn adopt_gc_owner(&mut self, _owner: &GcOwnerRef) {}
}

#[cfg(not(feature = "jsc"))]
#[derive(Clone)]
pub struct GcOwnerRef;

#[cfg(not(feature = "jsc"))]
impl<T> GcOwner for T {}

// ── V8 backend ─────────────────────────────────────────────────────────────
//
// `GcCell<T>` is a cppgc `Member` edge to a heap cell allocated on the
// isolate's `cppgc::Heap`. Cloning creates a second edge (via `GetRustObj`),
// mirroring Boa's `Gc<GcRefCell<T>>` clone semantics. The value itself lives
// in an `UnsafeCell` guarded by the isolate-scoped access discipline; the
// borrow counter restores the runtime double-borrow checks of `RefCell`.
//
// Borrow discipline: never call engine methods (any `ec` operation) while a
// borrow guard is live — shared or mutable. An engine call may allocate and
// trigger a cppgc trace that reads the cell while the borrow is live; a
// mutable borrow being traced is undefined behavior. Clone the value out
// instead (`borrow(ec).clone()` … `set(value, ec)`), or scope the borrow to
// a non-engine section. `HeapCell::trace` aborts on a live mutable borrow as
// a backstop.
#[cfg(feature = "v8")]
pub use v8_cells::*;

#[cfg(feature = "v8")]
mod v8_cells {
    use super::*;
    use crate::gc::Trace;
    use crate::v8::gc::{V8GcCell, V8GcRef, V8GcRefMut};
    use crate::v8::{V8Engine, V8Types};

    /// Unified GC-managed cell providing interior mutability.
    #[derive(Clone)]
    pub struct GcCell<T: Trace + 'static>(pub(crate) V8GcCell<T>);

    /// Construct a [`GcCell`] with the given value.
    ///
    /// Allocates the cell on the execution context's isolate cppgc heap.
    pub fn gc_cell_new<T: Trace + 'static>(
        mut value: T,
        ec: &mut dyn ExecutionContext<V8Types>,
    ) -> GcCell<T> {
        // Convert any rooted JS handles into cppgc edges before the value
        // enters traced storage.
        value.store(ec);
        let engine = ec
            .as_any()
            .downcast_ref::<V8Engine>()
            .expect("V8 GcCell created with a non-V8 execution context");
        GcCell(V8GcCell::new(value, engine))
    }

    impl<T: Trace + 'static> GcCell<T> {
        /// Immutably borrow the wrapped value.
        pub fn borrow<'a>(&'a self, ec: &dyn ExecutionContext<V8Types>) -> GcRef<'a, T> {
            self.0.borrow(ec)
        }

        /// Mutably borrow the wrapped value.
        pub fn borrow_mut<'a>(&'a self, ec: &mut dyn ExecutionContext<V8Types>) -> GcRefMut<'a, T> {
            self.0.borrow_mut(ec)
        }

        /// Replace the wrapped value.
        pub fn set(&self, mut value: T, ec: &mut dyn ExecutionContext<V8Types>) {
            // Convert any rooted JS handles into cppgc edges before the value
            // enters traced storage.
            value.store(ec);
            self.0.set(value, ec);
        }

        /// Compare two cells for pointer equality.
        pub fn ptr_eq(&self, other: &Self) -> bool {
            self.0.ptr_eq(&other.0)
        }
    }

    pub type GcRef<'a, T> = V8GcRef<'a, T>;
    pub type GcRefMut<'a, T> = V8GcRefMut<'a, T>;
}

/// Construct a [`GcCell`] with the given value.
///
/// The execution context supplies the engine access required for allocation
/// (the cppgc heap on V8). Boa and JSC ignore it but accept it for API
/// uniformity.
#[cfg(feature = "boa")]
pub fn gc_cell_new<T: boa_gc::Trace + 'static>(
    value: T,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> GcCell<T> {
    boa_cells::gc_cell_new(value, ec)
}

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "jsc")]
pub fn gc_cell_new<T: GcTraceable>(value: T, ec: &mut dyn ExecutionContext<JscTypes>) -> GcCell<T> {
    jsc_cells::gc_cell_new(value, ec)
}

/// Construct a [`GcCell`] with the given value.
#[cfg(feature = "v8")]
pub fn gc_cell_new<T: Trace + 'static>(
    value: T,
    ec: &mut dyn ExecutionContext<V8Types>,
) -> GcCell<T> {
    v8_cells::gc_cell_new(value, ec)
}

/// Compare two [`GcCell`] references for pointer equality.
///
/// Returns `true` if both references point to the same allocation.
#[cfg(feature = "boa")]
pub fn gc_cell_ptr_eq<T: boa_gc::Trace + 'static>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    a.ptr_eq(b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(feature = "jsc")]
pub fn gc_cell_ptr_eq<T: GcTraceable>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    a.ptr_eq(b)
}

/// Compare two [`GcCell`] references for pointer equality.
#[cfg(feature = "v8")]
pub fn gc_cell_ptr_eq<T: Trace + 'static>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    a.ptr_eq(b)
}

/// Associate Rust platform data with an existing JS object (e.g. the Window
/// platform object with the realm's global object).
///
/// Each backend stores the data where its `with_object_any` machinery can
/// find it again: JSC keeps a side map keyed by object pointer; V8 keeps a
/// per-realm association list. (The Boa backend builds the global object
/// directly through its host hooks, so it has no need for this.)
///
/// The data must be GC-traceable (`Trace` + `Finalize`): the bound is
/// satisfied by `#[gc_struct]` types, whose cells and JS edges participate in
/// the engine's tracing.
#[cfg(feature = "jsc")]
pub fn associate_existing_object<D>(
    ec: &mut dyn ExecutionContext<JscTypes>,
    object: &<JscTypes as JsTypes>::JsObject,
    data: D,
) where
    D: 'static + Trace + Finalize,
{
    let engine = ec
        .as_any_mut()
        .downcast_mut::<crate::jsc::JscEngine>()
        .expect("associate_existing_object called with a non-JSC execution context");
    engine.associate_existing_object(object, Box::new(data));
}

/// Associate Rust platform data with an existing JS object.
#[cfg(feature = "v8")]
pub fn associate_existing_object<D>(
    ec: &mut dyn ExecutionContext<V8Types>,
    object: &<V8Types as JsTypes>::JsObject,
    data: D,
) where
    D: 'static + Trace + Finalize,
{
    let engine = ec
        .as_any_mut()
        .downcast_mut::<crate::v8::V8Engine>()
        .expect("associate_existing_object called with a non-V8 execution context");
    // The concrete type is known here (`D: Trace`), so the platform is
    // wrapped in `V8PlatformData` with its real trace before the engine
    // stores it on the cppgc heap: the associated platform's cells and JS
    // edges (Window event listeners, timers, ...) must be traced while the
    // realm lives.
    engine.associate_existing_object(object, Box::new(crate::v8::V8PlatformData::new(data)));
}

/// Create a JS object with the given prototype, wrapping GC-traceable
/// platform data in the backend's GC wrapper (V8 `V8PlatformData`, Boa
/// `TraceableBox`) so the engine's GC traces the platform object's cells and
/// JS edges from the JS wrapper. JSC stores the raw data in its per-object
/// side table.
///
/// The concrete data type `D` is known here (`Trace` + `Finalize`), so the
/// wrapper carries the real trace/finalize vtables; `create_object_with_any`
/// alone only receives type-erased `Box<dyn Any>` and falls back to no-op
/// tracing, which is only safe for prototypes and namespace objects that hold
/// no `GcCell` fields. Generic over `Ty` so the Web IDL bindings can call it
/// from generic code.
#[cfg(feature = "jsc")]
pub fn create_platform_object<Ty, D>(
    ec: &mut dyn ExecutionContext<Ty>,
    prototype: &Ty::JsObject,
    data: D,
) -> Ty::JsObject
where
    Ty: JsTypes + JsTypesWithRealm,
    D: 'static + Trace + Finalize,
{
    ec.create_object_with_any(prototype.clone(), Box::new(data))
}

/// Create a JS object with the given prototype, wrapping GC-traceable
/// platform data in the backend's GC wrapper (V8 `V8PlatformData`, Boa
/// `TraceableBox`) so the engine's GC traces the platform object's cells and
/// JS edges from the JS wrapper. JSC stores the raw data in its per-object
/// side table.
///
/// The concrete data type `D` is known here (`Trace` + `Finalize`), so the
/// wrapper carries the real trace/finalize vtables; `create_object_with_any`
/// alone only receives type-erased `Box<dyn Any>` and falls back to no-op
/// tracing, which is only safe for prototypes and namespace objects that hold
/// no `GcCell` fields. Generic over `Ty` so the Web IDL bindings can call it
/// from generic code.
#[cfg(feature = "v8")]
pub fn create_platform_object<Ty, D>(
    ec: &mut dyn ExecutionContext<Ty>,
    prototype: &Ty::JsObject,
    data: D,
) -> Ty::JsObject
where
    Ty: JsTypes + JsTypesWithRealm,
    D: 'static + Trace + Finalize,
{
    // The concrete type is known here (`D: Trace`), so the platform is
    // wrapped in `V8PlatformData` with its real trace before the engine
    // stores it on the cppgc heap: the platform's cells and JS edges must
    // be traced while the wrapper lives.
    ec.create_object_with_any(
        prototype.clone(),
        Box::new(crate::v8::V8PlatformData::new(data)),
    )
}

/// Create a JS object with the given prototype, wrapping GC-traceable
/// platform data in the backend's GC wrapper (V8 `V8PlatformData`, Boa
/// `TraceableBox`) so the engine's GC traces the platform object's cells and
/// JS edges from the JS wrapper. JSC stores the raw data in its per-object
/// side table.
///
/// The concrete data type `D` is known here (`Trace` + `Finalize`), so the
/// wrapper carries the real trace/finalize vtables; `create_object_with_any`
/// alone only receives type-erased `Box<dyn Any>` and falls back to no-op
/// tracing, which is only safe for prototypes and namespace objects that hold
/// no `GcCell` fields. Generic over `Ty` so the Web IDL bindings can call it
/// from generic code.
#[cfg(feature = "boa")]
pub fn create_platform_object<Ty, D>(
    ec: &mut dyn ExecutionContext<Ty>,
    prototype: &Ty::JsObject,
    data: D,
) -> Ty::JsObject
where
    Ty: JsTypes + JsTypesWithRealm,
    D: 'static + Trace + Finalize,
{
    // The concrete type is known here (`D: Trace`), so the data is wrapped
    // in a `TraceableBox` with its real trace/finalize vtables before the
    // engine stores it: the platform's `GcCell` fields and JS references
    // must be visible to the Boa GC while the wrapper lives.
    ec.create_object_with_any(
        prototype.clone(),
        Box::new(crate::boa::TraceableBox::new(data)),
    )
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
    use crate::jsc::{JscEngine, JscManagedValue, JscTypes};

    impl JsTypesGcExt for JscTypes {
        /// A weak `JSManagedValue` reference to the wrapper.  Unlike a raw
        /// pointer, it reports `None` once the wrapper has been collected
        /// instead of dangling.
        type Reflector = JscManagedValue;
        type Context = JscEngine;

        fn create_reflector(context: &mut Self::Context, obj: &Self::JsObject) -> Self::Reflector {
            JscManagedValue::new_weak(context.gc_context(), &obj.as_value())
                .unwrap_or_else(JscManagedValue::empty)
        }

        fn upgrade_reflector(
            _context: &mut Self::Context,
            reflector: &Self::Reflector,
        ) -> Option<Self::JsObject> {
            reflector.get_object()
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

#[cfg(all(not(feature = "boa"), not(feature = "v8")))]
mod persistent_handle_trace_impls {
    use super::GcTraceable;
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
    // wrappers (they must use GcCell instead).
    unsafe impl<T: Trace> Trace for std::rc::Rc<std::cell::RefCell<T>> {}
    unsafe impl<T: Trace> Trace for std::rc::Rc<std::cell::Cell<T>> {}
    unsafe impl<T: Trace + GcTraceable> Trace for super::GcCell<T> {}
    unsafe impl<A: Trace, B: Trace> Trace for (A, B) {}
    unsafe impl<A: Trace, B: Trace, C: Trace> Trace for (A, B, C) {}
    unsafe impl<A: Trace, B: Trace, C: Trace, D: Trace> Trace for (A, B, C, D) {}
    unsafe impl<A: Trace, B: Trace, C: Trace, D: Trace, E: Trace> Trace for (A, B, C, D, E) {}
}

// V8: the same blanket impls with real trace bodies. `Cell<T>` values are
// Copy-only, so they hold no edges; the others walk their contents.
#[cfg(feature = "v8")]
mod v8_trace_impls {
    use super::Trace;
    use crate::v8_gc::Visitor;

    macro_rules! empty_trace {
        ($($ty:ty),* $(,)?) => {
            $(
                unsafe impl Trace for $ty {
                    unsafe fn trace(&self, _visitor: &mut Visitor) {}

                    fn store(&mut self, _ec: &mut dyn crate::ExecutionContext<crate::v8::V8Types>) {}
                }
            )*
        };
    }

    empty_trace!(
        (),
        bool,
        char,
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
        String,
    );

    unsafe impl<T: Trace> Trace for std::rc::Rc<std::cell::Cell<T>> {
        unsafe fn trace(&self, _visitor: &mut Visitor) {}

        fn store(&mut self, _ec: &mut dyn crate::ExecutionContext<crate::v8::V8Types>) {}
    }

    unsafe impl<T: Trace + 'static> Trace for super::GcCell<T> {
        unsafe fn trace(&self, visitor: &mut Visitor) {
            crate::v8_gc::Traced::trace(&self.0, visitor);
        }

        fn store(&mut self, _ec: &mut dyn crate::ExecutionContext<crate::v8::V8Types>) {
            // The cell's contents are converted when they are written.
        }
    }

    macro_rules! tuple_trace {
        ($(($t:ident, $i:tt)),* $(,)?) => {
            unsafe impl<$($t: Trace),*> Trace for ($($t,)*) {
                unsafe fn trace(&self, visitor: &mut Visitor) {
                    $(
                        // SAFETY: Delegated to the element's own trace.
                        unsafe { Trace::trace(&self.$i, visitor) }
                    )*
                }

                fn store(&mut self, ec: &mut dyn crate::ExecutionContext<crate::v8::V8Types>) {
                    $(
                        self.$i.store(ec);
                    )*
                }
            }
        };
    }

    tuple_trace!((A, 0), (B, 1));
    tuple_trace!((A, 0), (B, 1), (C, 2));
    tuple_trace!((A, 0), (B, 1), (C, 2), (D, 3));
    tuple_trace!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
}
