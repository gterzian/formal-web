use js_engine::{Completion, ExecutionContext, JsTypes};

pub(crate) mod bindings;
/// Generic platform-object downcast helpers:
/// `try_with_*` functions using [`ExecutionContext::with_object_any`] / `with_object_any_mut`.
pub(crate) mod downcast;
/// Generic platform-object resolution helpers.
/// Uses only [`ExecutionContext`] trait methods.
pub(crate) mod platform_objects;

/// Generic engine builder — the single entry point for creating a JS engine
/// context.  Uses `#[cfg]` internally to switch between Boa and JSC backends.
pub(crate) mod build_context;

/// Generic bootstrap modules — use only [`ExecutionContext<T>`] trait methods.
/// Not engine-specific; compiled on all backends.
pub(crate) mod console_generic;
pub(crate) mod css_generic;

pub(crate) use console_generic::install_console_namespace;
pub(crate) use css_generic::install_css_namespace;

pub(crate) use bindings::install_document_property;
pub(crate) use downcast::{
    try_with_abort_controller_ref, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref,
    with_abort_signal_ref,
};

/// Create a builtin function whose captures are stored in GC-traceable
/// storage, preserving proper GC reachability of JS-object references
/// inside the captures.
///
/// On the Boa backend, the captures are stored as a concrete traceable
/// type `C` directly in `NativeFunction::from_copy_closure_with_captures`,
/// bypassing the no-op trace of the default `create_builtin_function`.
/// On the JSC backend, delegates to `create_builtin_function` (capuring
/// in a `Box<dyn Fn>`), which is safe because JSC's raw-pointer-backed
/// function objects keep captured values alive by pinning the heap.
///
/// Use this instead of `ec.create_builtin_fn(Box::new(...), ...)` when
/// the behaviour closure captures GC-traced domain types (stream
/// controllers, readers, promises, etc.).
#[cfg(not(jsc_backend))]
pub(crate) fn create_builtin_fn_with_traced_captures<C: boa_gc::Trace + 'static>(
    ec: &mut dyn ExecutionContext<Types>,
    captures: C,
    behaviour: fn(
        &[<Types as JsTypes>::JsValue],
        <Types as JsTypes>::JsValue,
        &C,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, Types>,
    length: u32,
    name: <Types as JsTypes>::PropertyKey,
    is_constructor: bool,
) -> <Types as JsTypes>::Function {
    // On Boa, Types = BoaTypes, so dyn ExecutionContext<Types> =
    // dyn ExecutionContext<BoaTypes>.  The cast is a no-op.
    js_engine::boa::create_builtin_fn_with_captures(
        ec,
        captures,
        behaviour,
        length,
        name,
        is_constructor,
    )
}

/// JSC fallback: wrap captures in a Box<dyn Fn> closure and delegate.
#[cfg(jsc_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<C: 'static>(
    ec: &mut dyn ExecutionContext<Types>,
    captures: C,
    behaviour: fn(
        &[<Types as JsTypes>::JsValue],
        <Types as JsTypes>::JsValue,
        &C,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, Types>,
    length: u32,
    name: <Types as JsTypes>::PropertyKey,
    is_constructor: bool,
) -> <Types as JsTypes>::Function {
    if is_constructor {
        ec.create_builtin_function(
            Box::new(move |args, this, ec| behaviour(args, this, &captures, ec)),
            length,
            name,
            true,
        )
    } else {
        ec.create_builtin_fn(
            Box::new(move |args, this, ec| behaviour(args, this, &captures, ec)),
            length,
            name,
        )
    }
}

/// Content-level type alias for the concrete JS types in use.
/// Set by the build script based on the target platform:
/// `jsc_backend` on Apple platforms, `boa_backend` on others.
#[cfg(jsc_backend)]
pub(crate) type Types = js_engine::jsc::JscTypes;

#[cfg(not(jsc_backend))]
pub(crate) type Types = js_engine::boa::BoaTypes;

/// Content-level type alias for the concrete JS engine in use.
/// `BoaContext` on Boa, `JscEngine` on JSC.
#[cfg(jsc_backend)]
pub(crate) type Engine = js_engine::jsc::JscEngine;

#[cfg(not(jsc_backend))]
pub(crate) type Engine = js_engine::boa::BoaContext;
