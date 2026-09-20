//! `Trace` implementations for the V8 wrapper types and the container types
//! used inside `#[gc_struct]` platform objects and `GcCell` contents.
//!
//! Each impl provides two operations:
//!
//! - `trace` — visit every cppgc edge (`TracedReference` to a JS object, or a
//!   nested `GcCell` `Member`) reachable from the value during marking.
//!   Root-mode handles have nothing to visit — they are strong V8 roots
//!   already; edge-mode handles keep their referent alive only while the
//!   owning heap object is traced.
//! - `store` — convert every rooted JS handle into a cppgc edge. Called when
//!   the value is stored into traced storage (`gc_cell_new`/`GcCell::set`, or
//!   a traced platform-object field through the engine's store helpers).

use std::collections::VecDeque;
use std::rc::Rc;

use rusty_v8 as v8;

use crate::ExecutionContext;
use crate::gc::GcRootHandle;
use crate::gc::Trace;
use crate::records::PromiseResolvers;
use crate::v8::V8Engine;
use crate::v8::V8Types;
use crate::v8::types::{V8BigInt, V8Handle, V8Object, V8String, V8Symbol, V8Value};
use crate::v8::{
    V8ArrayBuffer, V8AsyncGenerator, V8Constructor, V8DataView, V8Function, V8Generator, V8Map,
    V8Promise, V8Set, V8SharedArrayBuffer, V8TypedArray, V8WeakMap, V8WeakRef, V8WeakSet,
};
use crate::v8_gc::Visitor;

/// Visit the referent of an optional `V8Handle` when it is stored as an edge.
fn trace_optional_handle<T>(handle: &Option<V8Handle<T>>, visitor: &mut Visitor) {
    if let Some(V8Handle::Edge(edge)) = handle {
        visitor.trace(&**edge);
    }
}

/// Convert the referent of an optional `V8Handle` into an edge.
fn store_optional_handle<T>(
    handle: &mut Option<V8Handle<T>>,
    scope: &mut v8::PinScope<'_, '_, ()>,
) {
    if let Some(handle) = handle {
        handle.store_edge(scope);
    }
}

/// Assert that a handle reached by cppgc tracing is an edge, never a strong
/// root. A root here means a write into traced storage bypassed `store`; the
/// marker would silently skip it and the referent would be over-retained
/// instead of collected with its owner.
fn debug_assert_stored<T>(handle: &V8Handle<T>) {
    debug_assert!(
        matches!(handle, V8Handle::Edge(_)),
        "a rooted JS handle reached cppgc tracing: the store invariant was bypassed"
    );
}

/// Assert that every present handle in an optional pair is an edge.
fn debug_assert_optional_stored<T>(handle: &Option<V8Handle<T>>) {
    if let Some(handle) = handle {
        debug_assert_stored(handle);
    }
}

// SAFETY: Each impl visits every edge held by the value exactly once, and
// `store` converts every rooted handle into an edge exactly once.
unsafe impl Trace for V8Value {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        debug_assert_stored(&self.handle);
        if let V8Handle::Edge(edge) = &self.handle {
            visitor.trace(&**edge);
        }
        if let Some(profile) = &self.object_profile {
            debug_assert_stored(&profile.object_handle);
            if let V8Handle::Edge(edge) = &profile.object_handle {
                visitor.trace(&**edge);
            }
            debug_assert_optional_stored(&profile.array_buffer_handle);
            debug_assert_optional_stored(&profile.shared_array_buffer_handle);
            debug_assert_optional_stored(&profile.typed_array_handle);
            debug_assert_optional_stored(&profile.data_view_handle);
            debug_assert_optional_stored(&profile.promise_handle);
            debug_assert_optional_stored(&profile.function_handle);
            debug_assert_optional_stored(&profile.map_handle);
            debug_assert_optional_stored(&profile.set_handle);
            trace_optional_handle(&profile.array_buffer_handle, visitor);
            trace_optional_handle(&profile.shared_array_buffer_handle, visitor);
            trace_optional_handle(&profile.typed_array_handle, visitor);
            trace_optional_handle(&profile.data_view_handle, visitor);
            trace_optional_handle(&profile.promise_handle, visitor);
            trace_optional_handle(&profile.function_handle, visitor);
            trace_optional_handle(&profile.map_handle, visitor);
            trace_optional_handle(&profile.set_handle, visitor);
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        let engine = ec
            .as_any_mut()
            .downcast_mut::<V8Engine>()
            .expect("V8 value stored with a non-V8 execution context");
        engine.with_value_scope(|scope| {
            self.handle.store_edge(scope);
            if let Some(profile) = &mut self.object_profile {
                profile.object_handle.store_edge(scope);
                store_optional_handle(&mut profile.array_buffer_handle, scope);
                store_optional_handle(&mut profile.shared_array_buffer_handle, scope);
                store_optional_handle(&mut profile.typed_array_handle, scope);
                store_optional_handle(&mut profile.data_view_handle, scope);
                store_optional_handle(&mut profile.promise_handle, scope);
                store_optional_handle(&mut profile.function_handle, scope);
                store_optional_handle(&mut profile.map_handle, scope);
                store_optional_handle(&mut profile.set_handle, scope);
            }
        });
    }
}

// SAFETY: See `V8Value`; both handles are edges to the same object.
unsafe impl Trace for V8Object {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        debug_assert_stored(&self.1);
        // SAFETY: Delegated to the inner value's trace.
        unsafe { self.0.trace(visitor) }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        self.0.store(ec);
        let engine = ec
            .as_any_mut()
            .downcast_mut::<V8Engine>()
            .expect("V8 object stored with a non-V8 execution context");
        engine.with_value_scope(|scope| {
            self.1.store_edge(scope);
        });
    }
}

// SAFETY: The string value (when present) holds the only edges.
unsafe impl Trace for V8String {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        if let Some(value) = &self.value {
            // SAFETY: Delegated to the inner value's trace.
            unsafe { value.trace(visitor) }
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        if let Some(value) = &mut self.value {
            value.store(ec);
        }
    }
}

// SAFETY: The symbol wraps a single value.
unsafe impl Trace for V8Symbol {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: Delegated to the inner value's trace.
        unsafe { self.0.trace(visitor) }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        self.0.store(ec);
    }
}

// SAFETY: The bigint wraps a single value.
unsafe impl Trace for V8BigInt {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: Delegated to the inner value's trace.
        unsafe { self.value.trace(visitor) }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        self.value.store(ec);
    }
}

// SAFETY: Each typed wrapper holds its object handle; both handles are edges
// to the same object.
macro_rules! typed_wrapper_trace {
    ($($name:path),* $(,)?) => {
        $(
            // SAFETY: See `V8Object`.
            unsafe impl Trace for $name {
                unsafe fn trace(&self, visitor: &mut Visitor) {
                    debug_assert_stored(&self.1);
                    // SAFETY: Delegated to the inner object's trace.
                    unsafe { self.0.trace(visitor) }
                    // The typed handle is a distinct edge from the object's
                    // own handle; it must be visited or its referent could be
                    // collected while the wrapper still uses it.
                    if let V8Handle::Edge(edge) = &self.1 {
                        visitor.trace(&**edge);
                    }
                }

                fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
                    self.0.store(ec);
                    let engine = ec
                        .as_any_mut()
                        .downcast_mut::<V8Engine>()
                        .expect("V8 typed wrapper stored with a non-V8 execution context");
                    engine.with_value_scope(|scope| {
                        self.1.store_edge(scope);
                    });
                }
            }
        )*
    };
}

typed_wrapper_trace!(
    V8ArrayBuffer,
    V8SharedArrayBuffer,
    V8TypedArray,
    V8DataView,
    V8Promise,
    V8Map,
    V8Set,
    V8Function,
    V8Constructor,
);

/// The `tagged_v8_object!` wrappers hold only the object (no typed handle).
macro_rules! tagged_wrapper_trace {
    ($($name:path),* $(,)?) => {
        $(
            // SAFETY: See `V8Object`.
            unsafe impl Trace for $name {
                unsafe fn trace(&self, visitor: &mut Visitor) {
                    // SAFETY: Delegated to the inner object's trace.
                    unsafe { self.0.trace(visitor) }
                }

                fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
                    self.0.store(ec);
                }
            }
        )*
    };
}

tagged_wrapper_trace!(
    V8WeakMap,
    V8WeakSet,
    V8WeakRef,
    V8Generator,
    V8AsyncGenerator,
);

// SAFETY: A root handle holds the wrapped value; tracing it keeps any stored
// edge alive while this handle is traced.
unsafe impl Trace for GcRootHandle<V8Types> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: Delegated to the wrapped value's trace.
        unsafe { self.value.trace(visitor) }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        self.value.store(ec);
    }
}

// SAFETY: The promise resolvers hold the two function edges.
unsafe impl Trace for PromiseResolvers<V8Types> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: Delegated to the resolve/reject object traces.
        unsafe {
            self.resolve.trace(visitor);
            self.reject.trace(visitor);
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        self.resolve.store(ec);
        self.reject.store(ec);
    }
}

// SAFETY: The optional value is visited when present.
unsafe impl<T: Trace> Trace for Option<T> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        if let Some(value) = self {
            // SAFETY: Delegated to the inner value's trace.
            unsafe { value.trace(visitor) }
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        if let Some(value) = self {
            value.store(ec);
        }
    }
}

// SAFETY: Every element is visited; `Vec` is only mutated between GCs (the
// heap uses stop-the-world marking).
unsafe impl<T: Trace> Trace for Vec<T> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        for item in self {
            // SAFETY: Delegated to the element's trace.
            unsafe { item.trace(visitor) }
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        for item in self {
            item.store(ec);
        }
    }
}

// SAFETY: Every element is visited.
unsafe impl<T: Trace> Trace for VecDeque<T> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        for item in self {
            // SAFETY: Delegated to the element's trace.
            unsafe { item.trace(visitor) }
        }
    }

    fn store(&mut self, ec: &mut dyn ExecutionContext<V8Types>) {
        for item in self {
            item.store(ec);
        }
    }
}

// NOTE: there is deliberately no `Trace` impl for bare `std::cell::RefCell`.
// A `#[gc_struct]` field that needs interior mutability must use `GcCell<T>`,
// whose runtime borrow counters coordinate with the marker; a bare `RefCell`
// field compiles only when marked `#[ignore_trace]` (content that holds no
// cppgc edges, e.g. an enum state flag). A blanket impl would either alias a
// live `borrow_mut` during marking or panic inside V8's C++ marking visitor.

// SAFETY: The shared value is visited.
unsafe impl<T: Trace> Trace for Rc<T> {
    unsafe fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: Delegated to the inner value's trace.
        unsafe { self.as_ref().trace(visitor) }
    }

    fn store(&mut self, _ec: &mut dyn ExecutionContext<V8Types>) {
        // A shared value is converted at its creation site, not here.
    }
}
