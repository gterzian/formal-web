//! V8 backend GC cells backed by `rusty_v8::cppgc`.
//!
//! A [`V8GcCell`] is a cppgc `Member` edge to a [`HeapCell`] allocated on the
//! isolate's `cppgc::Heap`. Cloning a cell creates a second `Member` edge to
//! the same heap cell (via `GetRustObj`), mirroring the clone semantics of
//! Boa's `Gc<GcRefCell<T>>`: the cell stays alive while any edge is traced by
//! a live owner, and is reclaimed once the last owner dies. The wrapped value
//! lives in an `UnsafeCell`, so mutation is only granted with isolate-scoped
//! proof — the execution context. A runtime borrow counter restores the
//! double-borrow checks `RefCell` provides on other engines.

use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use log::error;
use rusty_v8 as v8;
use v8::cppgc::{self, GarbageCollected, GetRustObj, Member};

use crate::ExecutionContext;
use crate::gc::Trace;
use crate::v8::{V8Engine, V8Types};
use crate::v8_gc::{Traced, Visitor};

/// Type-erased cppgc platform object: the domain data lives inside a
/// cppgc-managed heap object, and its edges are traced through the concrete
/// type's `Trace` implementation. The JS wrapper traces this object through
/// the `v8::Object::wrap` link, so the unified heap collects wrapper/platform
/// pairs (and cycles through their cells) together.
pub struct V8PlatformData {
    data: Box<dyn Any>,
    trace_fn: unsafe fn(&dyn Any, &mut cppgc::Visitor),
}

impl V8PlatformData {
    /// Wrap traceable domain data (a `#[gc_struct]` platform object).
    pub fn new<T: Any + Trace>(data: T) -> Self {
        Self {
            data: Box::new(data),
            trace_fn: |data, visitor| {
                // SAFETY: The box holds exactly the `T` this closure was
                // created for; the trace implementation visits its edges.
                unsafe {
                    <T as Trace>::trace(
                        data.downcast_ref::<T>()
                            .expect("platform data type mismatch"),
                        visitor,
                    )
                }
            },
        }
    }

    /// Wrap non-traceable data (prototypes, namespace objects) with no edges.
    pub fn noop(data: Box<dyn Any>) -> Self {
        Self {
            data,
            trace_fn: |_data, _visitor| {},
        }
    }

    /// Whether `data` is already a [`V8PlatformData`] wrapper.
    pub fn try_recover(data: Box<dyn Any>) -> Result<Self, Box<dyn Any>> {
        data.downcast().map(|boxed| *boxed)
    }

    pub fn as_any(&self) -> &dyn Any {
        &*self.data
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut *self.data
    }
}

// SAFETY: The trace delegates to the concrete platform type's `Trace` impl,
// which visits every edge exactly once. The `Box` heap allocation is stable;
// only the trace reads it during stop-the-world marking.
unsafe impl GarbageCollected for V8PlatformData {
    fn trace(&self, visitor: &mut cppgc::Visitor) {
        // SAFETY: The trace runs during stop-the-world marking on the isolate
        // thread; no Rust code mutates the platform data concurrently.
        unsafe { (self.trace_fn)(&*self.data, visitor) }
    }

    fn get_name(&self) -> &'static std::ffi::CStr {
        c"js_engine::platform object"
    }
}

/// The cppgc heap object backing a [`V8GcCell`].
///
/// `value` is guarded by the isolate-scoped access discipline: reads and
/// writes are only granted through the execution context's engine. A runtime
/// borrow counter (`readers`/`writer`) restores the double-borrow checks
/// `RefCell` provides on the other engines.
pub(crate) struct HeapCell<T> {
    value: UnsafeCell<T>,
    readers: Cell<u32>,
    writer: Cell<bool>,
}

impl<T> HeapCell<T> {
    fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            readers: Cell::new(0),
            writer: Cell::new(false),
        }
    }
}

impl<T> HeapCell<T> {
    /// Returns `Err` when a mutable borrow of the cell is live during marking.
    ///
    /// A live `V8GcRefMut` means Rust code holds `&mut T` into `value`; the
    /// marker independently deriving a shared `&T` there would alias the
    /// exclusive reference (undefined behavior). The check runs before any
    /// dereference so the violation is detected while the state is still
    /// sound.
    fn trace_conflict(&self) -> Result<(), &'static str> {
        if self.writer.get() {
            Err("GcCell<T> is mutably borrowed during cppgc marking")
        } else {
            Ok(())
        }
    }
}

// SAFETY: `HeapCell` is traced by delegating to `T`'s trace — every
// `TracedReference` edge and nested cell reachable from `T` is visited during
// marking. Marking is stop-the-world (the heap is created with atomic marking
// support), so the `UnsafeCell` is never read concurrently with a write.
unsafe impl<T: Trace + 'static> GarbageCollected for HeapCell<T> {
    fn trace(&self, visitor: &mut cppgc::Visitor) {
        // A live `borrow_mut` guard means the cell is being mutated while the
        // marker would read it — the aliasing hazard described on
        // `trace_conflict`. The trace runs inside V8's C++ marking visitor
        // (`rusty_v8_RustObj_trace`), so a Rust panic here would unwind
        // across the FFI boundary; fail with a hard abort instead, after
        // logging, so the interleaving becomes a deterministic, debuggable
        // crash rather than silent undefined behavior. Shared borrows are
        // legal aliasing and do not trip this check.
        if let Err(message) = self.trace_conflict() {
            error!("{message}; aborting to avoid aliasing undefined behavior");
            std::process::abort();
        }
        // SAFETY: The trace runs during stop-the-world marking on the isolate
        // thread and the borrow counter proves no mutable borrow is live; no
        // Rust code mutates the cell while the marker reads it.
        unsafe { <T as Trace>::trace(&*self.value.get(), visitor) }
    }

    fn get_name(&self) -> &'static std::ffi::CStr {
        c"js_engine::GcCell"
    }
}

/// A shared, cloneable GC-managed cell: a cppgc `Member` edge to a heap cell.
///
/// The cell is kept alive while any clone (edge) is traced by a live owner
/// and is reclaimed by the isolate's cppgc heap once the last edge is
/// unreachable.
pub struct V8GcCell<T: Trace + 'static>(Member<HeapCell<T>>);

impl<T: Trace + 'static> Clone for V8GcCell<T> {
    fn clone(&self) -> Self {
        // `Member::new` reads the pointee through `GetRustObj` and creates a
        // second strong edge to the same heap cell; the cell stays alive while
        // any edge is traced.
        Self(Member::new(&self.0))
    }
}

impl<T: Trace + 'static> V8GcCell<T> {
    /// Allocate a new cell on the engine's isolate cppgc heap.
    pub(crate) fn new(value: T, engine: &V8Engine) -> Self {
        let heap_cell = HeapCell::new(value);
        let pointer = engine.with_cpp_heap(|heap| {
            // SAFETY: `make_garbage_collected` returns an `UnsafePtr` which is
            // immediately moved into the `Member` edge below — the required
            // destination for a stack-created pointer.
            unsafe { v8::cppgc::make_garbage_collected(heap, heap_cell) }
        });
        Self(Member::new(&pointer))
    }

    /// Immutably borrow the wrapped value.
    ///
    /// The returned guard's lifetime is tied to `&self`, not to the execution
    /// context, so the compiler does not prevent calling back into the engine
    /// while a borrow is held — and the borrow discipline forbids it: an
    /// engine call can allocate and trigger a cppgc trace that reads the cell
    /// while the borrow is live (see `js_engine/README.md`). Clone the value
    /// out instead, or scope the borrow to a non-engine section. The heap
    /// cell is kept alive by this edge (`Member`) for the whole borrow, and
    /// the borrow counter prevents mutable aliasing.
    pub(crate) fn borrow<'a>(&'a self, _ec: &dyn ExecutionContext<V8Types>) -> V8GcRef<'a, T> {
        let heap_cell = unsafe { self.0.get() }.expect("V8 GcCell edge holds no heap cell");
        if heap_cell.writer.get() {
            panic!("GcCell<T> already mutably borrowed");
        }
        heap_cell.readers.set(heap_cell.readers.get() + 1);
        let value = heap_cell.value.get() as *const T;
        V8GcRef {
            value,
            cell: heap_cell as *const HeapCell<T>,
            _marker: PhantomData,
        }
    }

    /// Mutably borrow the wrapped value.
    ///
    /// The returned guard holds the execution context until it is dropped,
    /// so the compiler prevents calling back into the engine while the
    /// borrow is held. Dropping the guard runs `Trace::store` over the cell's
    /// contents: any rooted handle written through the guard is converted to
    /// a cppgc edge before the borrow ends, so a `borrow_mut().push(...)` or
    /// field assignment cannot leave a strong root inside traced storage.
    pub(crate) fn borrow_mut<'a>(
        &'a self,
        ec: &'a mut dyn ExecutionContext<V8Types>,
    ) -> V8GcRefMut<'a, T> {
        let heap_cell = unsafe { self.0.get() }.expect("V8 GcCell edge holds no heap cell");
        if heap_cell.writer.get() || heap_cell.readers.get() > 0 {
            panic!("GcCell<T> already borrowed");
        }
        heap_cell.writer.set(true);
        let value = heap_cell.value.get();
        V8GcRefMut {
            value,
            cell: heap_cell as *const HeapCell<T>,
            ec,
            _marker: PhantomData,
        }
    }

    /// Replace the wrapped value.
    pub(crate) fn set(&self, value: T, _ec: &mut dyn ExecutionContext<V8Types>) {
        let heap_cell = unsafe { self.0.get() }.expect("V8 GcCell edge holds no heap cell");
        if heap_cell.writer.get() || heap_cell.readers.get() > 0 {
            panic!("GcCell<T> already borrowed");
        }
        // SAFETY: The borrow counter proves exclusive access.
        unsafe {
            *heap_cell.value.get() = value;
        }
    }

    /// Compare two cells for pointer equality.
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        self.0.get_rust_obj() == other.0.get_rust_obj()
    }
}

// The cell edge is traced by visiting the underlying `Member`: a parent heap
// object tracing a nested `GcCell` field keeps the cell alive and traces its
// contents.
impl<T: Trace + 'static> Traced for V8GcCell<T> {
    fn trace(&self, visitor: &mut Visitor) {
        visitor.trace(&self.0);
    }
}

/// Immutable borrow guard for [`V8GcCell`].
pub struct V8GcRef<'a, T> {
    value: *const T,
    cell: *const HeapCell<T>,
    _marker: PhantomData<&'a T>,
}

impl<T> Deref for V8GcRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: The edge held by the originating `V8GcCell` keeps the heap
        // cell alive for the lifetime of this guard (`'a`), and the borrow
        // counter guarantees no mutable borrow is active.
        unsafe { &*self.value }
    }
}

impl<T> Drop for V8GcRef<'_, T> {
    fn drop(&mut self) {
        // SAFETY: `cell` points into the same heap cell that supplied `value`;
        // it is kept alive by the edge for the guard's lifetime.
        unsafe {
            (*self.cell).readers.set((*self.cell).readers.get() - 1);
        }
    }
}

/// Mutable borrow guard for [`V8GcCell`].
///
/// Carries the execution context so dropping the guard can run
/// `Trace::store` over the mutated contents (see `V8GcCell::borrow_mut`).
pub struct V8GcRefMut<'a, T: Trace + 'static> {
    value: *mut T,
    cell: *const HeapCell<T>,
    ec: &'a mut dyn ExecutionContext<V8Types>,
    _marker: PhantomData<&'a mut T>,
}

impl<T: Trace + 'static> Deref for V8GcRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: See `V8GcRef::deref`; the borrow counter guarantees this is
        // the only active borrow.
        unsafe { &*self.value }
    }
}

impl<T: Trace + 'static> DerefMut for V8GcRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: The guard is the only mutable borrow (checked at creation);
        // no other accessor holds a reference into this cell.
        unsafe { &mut *self.value }
    }
}

impl<T: Trace + 'static> Drop for V8GcRefMut<'_, T> {
    fn drop(&mut self) {
        // Clear the writer flag before storing: `store` materializes V8
        // locals (allocating a `TracedReference` through the engine's value
        // scope), and a mark must not observe a live mutable borrow.
        // SAFETY: Same liveness argument as `V8GcRef::drop`.
        unsafe {
            (*self.cell).writer.set(false);
        }
        // SAFETY: The heap cell is kept alive by the originating edge for the
        // guard's lifetime, the writer flag is cleared, and `&mut *value` is
        // the only live reference into the cell.
        unsafe {
            Trace::store(&mut *self.value, self.ec);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The writer flag is the sole signal the marker has that a `borrow_mut`
    /// guard is live; `trace_conflict` must report it before `trace` would
    /// dereference the cell.
    #[test]
    fn mutably_borrowed_cell_flagged_during_marking() {
        let cell = HeapCell::new(());
        assert!(
            cell.trace_conflict().is_ok(),
            "an unborrowed cell must pass the marking check"
        );
        cell.writer.set(true);
        assert!(
            cell.trace_conflict().is_err(),
            "a mutably borrowed cell must be flagged during marking"
        );
        cell.writer.set(false);
        assert!(
            cell.trace_conflict().is_ok(),
            "the check must clear once the borrow ends"
        );
    }
}
