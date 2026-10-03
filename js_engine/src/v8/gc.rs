//! V8 backend GC cells backed by `rusty_v8::cppgc`.
//!
//! A [`V8GcCell`] is a reference to a [`HeapCell`] allocated on the isolate's
//! `cppgc::Heap`, in one of two modes: a cppgc `Member` edge while the cell is
//! stored in traced storage, or a cppgc `Persistent` root once it leaves (a
//! clone held in Rust memory). The `Member` edge is kept alive by the traced
//! owner that holds it; the `Persistent` is a GC root, so a clone captured in
//! a queued job or a plain struct keeps the heap cell — and, through it, the
//! JS objects in the cell's edges — alive until the clone is dropped. Cloning
//! a cell always produces the `Persistent` mode; [`Trace::store`] converts a
//! clone back to the `Member` mode when it re-enters traced storage. The
//! wrapped value lives in an `UnsafeCell`, so mutation is only granted with
//! isolate-scoped proof — the execution context. A runtime borrow counter
//! restores the double-borrow checks `RefCell` provides on other engines.

use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use log::error;
use rusty_v8 as v8;
use v8::cppgc::{self, GarbageCollected, Member, Persistent};

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
    // Interior mutability: the domain data is mutated through
    // `with_object_any_mut`, whose only pointer is derived from a shared
    // reference to the platform object (rusty_v8 hands out `*const` from the
    // cppgc object). Routing the write through `UnsafeCell::get` keeps the
    // write from going through a `*mut` derived from `&`.
    data: UnsafeCell<Box<dyn Any>>,
    trace_fn: unsafe fn(&dyn Any, &mut cppgc::Visitor),
}

impl V8PlatformData {
    /// Wrap traceable domain data (a `#[gc_struct]` platform object).
    pub fn new<T: Any + Trace>(data: T) -> Self {
        Self {
            data: UnsafeCell::new(Box::new(data)),
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
            data: UnsafeCell::new(data),
            trace_fn: |_data, _visitor| {},
        }
    }

    /// Whether `data` is already a [`V8PlatformData`] wrapper.
    pub fn try_recover(data: Box<dyn Any>) -> Result<Self, Box<dyn Any>> {
        data.downcast().map(|boxed| *boxed)
    }

    pub fn as_any(&self) -> &dyn Any {
        // SAFETY: shared access is unique while the caller holds no mutable
        // borrow; marks are stop-the-world on the isolate thread.
        unsafe { &**self.data.get() }
    }

    /// Raw pointer to the boxed value, for mutation through the cell.
    ///
    /// # Safety
    ///
    /// The caller must hold the exclusive platform-data access path for the
    /// borrow derived from the returned pointer (the engine's
    /// `with_object_any_mut` uses its `&mut self` receiver as that proof).
    /// The write goes through `UnsafeCell`, so no `*mut` is derived from a
    /// shared reference.
    pub(crate) fn data_mut_ptr(&self) -> *mut Box<dyn Any> {
        self.data.get()
    }
}

// SAFETY: The trace delegates to the concrete platform type's `Trace` impl,
// which visits every edge exactly once. The `Box` heap allocation is stable;
// only the trace reads it during stop-the-world marking.
unsafe impl GarbageCollected for V8PlatformData {
    fn trace(&self, visitor: &mut cppgc::Visitor) {
        // SAFETY: The trace runs during stop-the-world marking on the isolate
        // thread; no Rust code mutates the platform data concurrently.
        unsafe { (self.trace_fn)(&**self.data.get(), visitor) }
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

/// A shared, cloneable GC-managed cell: a [`HeapCell`] referenced in either
/// the traced (`Member`) or rooted (`Persistent`) mode.
///
/// The cell is kept alive while a traced owner holds the `Member` edge or
/// while any rooted clone holds the `Persistent`; it is reclaimed by the
/// isolate's cppgc heap once neither remains.
enum CellLink<T: Trace + 'static> {
    /// A cppgc edge, traced by the heap object that holds the cell.
    Member(Member<HeapCell<T>>),
    /// A cppgc root held by Rust-owned memory, e.g. a cloned cell captured in
    /// a queued job.
    Strong(Persistent<HeapCell<T>>),
}

pub struct V8GcCell<T: Trace + 'static>(CellLink<T>);

impl<T: Trace + 'static> Clone for V8GcCell<T> {
    fn clone(&self) -> Self {
        // A clone may land in Rust-owned memory that no cppgc owner traces, so
        // it takes the rooted mode: `Persistent::new` reads the pointee through
        // `GetRustObj` and roots the same heap cell. `Trace::store` converts it
        // back to the traced mode when the clone re-enters traced storage.
        let strong = match &self.0 {
            CellLink::Member(member) => Persistent::new(member),
            CellLink::Strong(persistent) => Persistent::new(persistent),
        };
        Self(CellLink::Strong(strong))
    }
}

impl<T: Trace + 'static> V8GcCell<T> {
    /// Allocate a new cell on the engine's isolate cppgc heap.
    ///
    /// The cell starts in the rooted (`Persistent`) mode: a freshly created
    /// cell has no traced owner yet, so an edge would be swept by the first
    /// collection before `Trace::store` moves it into traced storage. The
    /// `Persistent` keeps it alive until then; `store_as_member` converts it
    /// to an edge when it enters a traced owner.
    pub(crate) fn new(value: T, engine: &V8Engine) -> Self {
        let heap_cell = HeapCell::new(value);
        let pointer = engine.with_cpp_heap(|heap| {
            // SAFETY: `make_garbage_collected` returns an `UnsafePtr` which is
            // immediately moved into the `Persistent` root below — a valid
            // destination for a stack-created pointer.
            unsafe { v8::cppgc::make_garbage_collected(heap, heap_cell) }
        });
        Self(CellLink::Strong(Persistent::new(&pointer)))
    }

    /// The heap cell this reference points at.
    ///
    /// The cell is kept alive for the borrow: a `Member` by the traced owner
    /// that holds it (the borrower follows the borrow discipline), a
    /// `Persistent` by its own root.
    fn heap_cell(&self) -> &HeapCell<T> {
        match &self.0 {
            // SAFETY: The `Member` appears in the trace implementation of the
            // owner that holds this reference, which is alive for the borrow.
            CellLink::Member(member) => {
                unsafe { member.get() }.expect("V8 GcCell member holds no heap cell")
            }
            CellLink::Strong(persistent) => {
                persistent.get().expect("V8 GcCell root holds no heap cell")
            }
        }
    }

    /// Convert a rooted clone back to a traced edge so the cell is reclaimed
    /// with the owner that now holds it. Idempotent for edges.
    pub(crate) fn store_as_member(&mut self) {
        let member = match &self.0 {
            CellLink::Member(_) => return,
            // The rooted reference keeps the cell alive while the edge is
            // created; no allocation can run between the two.
            CellLink::Strong(persistent) => Member::new(persistent),
        };
        self.0 = CellLink::Member(member);
    }

    /// Root the cell so a reference held in Rust-owned memory keeps it (and
    /// the JS objects in its edges) alive. Idempotent for roots.
    pub(crate) fn root(&mut self) {
        let strong = match &self.0 {
            CellLink::Strong(_) => return,
            CellLink::Member(member) => Persistent::new(member),
        };
        self.0 = CellLink::Strong(strong);
    }

    /// Immutably borrow the wrapped value.
    ///
    /// The returned guard's lifetime is tied to `&self`, not to the execution
    /// context, so the compiler does not prevent calling back into the engine
    /// while a borrow is held — and the borrow discipline forbids it: an
    /// engine call can allocate and trigger a cppgc trace that reads the cell
    /// while the borrow is live (see `js_engine/README.md`). Clone the value
    /// out instead, or scope the borrow to a non-engine section. The cell's
    /// reference is held for the whole borrow, and the borrow counter prevents
    /// mutable aliasing.
    pub(crate) fn borrow<'a>(&'a self, _ec: &dyn ExecutionContext<V8Types>) -> V8GcRef<'a, T> {
        let heap_cell = self.heap_cell();
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
        let heap_cell = self.heap_cell();
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
        let heap_cell = self.heap_cell();
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
        std::ptr::eq(self.heap_cell(), other.heap_cell())
    }
}

// The cell edge is traced by visiting the underlying `Member`: a parent heap
// object tracing a nested `GcCell` field keeps the cell alive and traces its
// contents. A rooted cell reached here means a clone was stored in traced
// storage without `Trace::store` converting it back — the referent is
// over-retained (the root keeps it alive) rather than dangling; report it like
// a rooted handle reached by tracing.
impl<T: Trace + 'static> Traced for V8GcCell<T> {
    fn trace(&self, visitor: &mut Visitor) {
        match &self.0 {
            CellLink::Member(member) => visitor.trace(member),
            CellLink::Strong(_) => {
                crate::v8::trace::record_strong_cell_reached_during_trace();
                debug_assert!(
                    false,
                    "a rooted GcCell reached cppgc tracing: the store invariant was bypassed"
                );
            }
        }
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
            let readers = (*self.cell).readers.get();
            (*self.cell).readers.set(
                readers
                    .checked_sub(1)
                    .expect("GcCell reader count underflow"),
            );
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
        // Keep the writer flag set through `store`: the marker derives `&T`
        // from the cell when tracing it, and clearing the flag first would
        // let a mark that runs during `store` alias the live `&mut T`. The
        // flag is cleared only after the store finishes. `store` walks the
        // cell's contents and never re-borrows this cell, so this cannot
        // introduce a self-conflict; if a mark does run, `HeapCell::trace`
        // aborts rather than aliasing the mutable borrow.
        // SAFETY: The heap cell is kept alive by the originating edge for the
        // guard's lifetime, and `&mut *value` is the only live reference into
        // the cell.
        unsafe {
            Trace::store(&mut *self.value, self.ec);
            (*self.cell).writer.set(false);
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
