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
#[cfg(feature = "v8")]
use crate::v8::V8Types;

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

/// An RAII guard that holds a rooted JS value for as long as the handle lives.
pub struct GcRootHandle<T: JsTypes> {
    /// The rooted JS value. Callers can read this to pass the value
    /// to trait methods like `EcmascriptHost::call`.
    pub value: T::JsValue,
}

impl<T: JsTypes> GcRootHandle<T> {
    /// Creates a new root handle.
    pub fn new(value: T::JsValue) -> Self {
        Self { value }
    }
}

impl<T: JsTypes> Clone for GcRootHandle<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

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

// ── V8 backend ─────────────────────────────────────────────────────────────
//
// `GcCell<T>` is a cppgc `Member` edge to a heap cell allocated on the
// isolate's `cppgc::Heap`. Cloning creates a second edge (via `GetRustObj`),
// mirroring Boa's `Gc<GcRefCell<T>>` clone semantics. The value itself lives
// in an `UnsafeCell` guarded by the isolate-scoped access discipline; the
// borrow counter restores the runtime double-borrow checks of `RefCell`.
//
// Borrow discipline: never call engine methods (any `ec` operation) while a
// shared borrow guard is live. An engine call may allocate and trigger a
// cppgc trace that reads the cell while the borrow is live, which is legal
// aliasing for a shared borrow but forbidden so call sites do not have to
// know which operations allocate. A mutable guard holds the execution
// context for its lifetime, so the compiler rejects an `ec` call while it is
// live; clone the value out (`borrow(ec).clone()` … `set(value, ec)`) or
// scope the mutable borrow to a non-engine section. Dropping a mutable guard
// runs `store` over the cell contents, converting rooted handles written
// through it into cppgc edges. `HeapCell::trace` aborts on a live mutable
// borrow as a backstop.
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
        pub fn borrow_mut<'a>(
            &'a self,
            ec: &'a mut dyn ExecutionContext<V8Types>,
        ) -> GcRefMut<'a, T> {
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
/// (the cppgc heap on V8). Boa ignores it but accepts it for API uniformity.
#[cfg(feature = "boa")]
pub fn gc_cell_new<T: boa_gc::Trace + 'static>(
    value: T,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> GcCell<T> {
    boa_cells::gc_cell_new(value, ec)
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
#[cfg(feature = "v8")]
pub fn gc_cell_ptr_eq<T: Trace + 'static>(a: &GcCell<T>, b: &GcCell<T>) -> bool {
    a.ptr_eq(b)
}

/// Associate Rust platform data with an existing JS object (e.g. the Window
/// platform object with the realm's global object).
///
/// V8 stores the data where its `with_object_any` machinery can find it
/// again, in a per-realm association list. (The Boa backend builds the global
/// object directly through its host hooks, so it has no need for this.)
///
/// The data must be GC-traceable (`Trace` + `Finalize`): the bound is
/// satisfied by `#[gc_struct]` types, whose cells and JS edges participate in
/// the engine's tracing.
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
/// JS edges from the JS wrapper.
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
/// JS edges from the JS wrapper.
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
/// on Boa.
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
