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

/// Create a builtin function with GC-traceable captures.
///
/// Use this instead of the old `create_builtin_function`/`create_builtin_fn`
/// trait methods (removed) when the behaviour needs to capture GC-traced
/// domain types (stream controllers, readers, promises, JsObject, etc.).
///
/// For stateless behaviour that captures nothing, pass `()` as the captures.
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
    js_engine::boa::create_builtin_fn_with_captures(
        ec,
        captures,
        behaviour,
        length,
        name,
        is_constructor,
    )
}

/// JSC fallback: wrap captures and function pointer into a Box<dyn Fn>.
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
    let _ = ec;
    let _ = captures;
    let _ = behaviour;
    let _ = length;
    let _ = name;
    let _ = is_constructor;
    unimplemented!("create_builtin_fn_with_traced_captures on JSC backend");
}

/// Convert a stateless raw function pointer into a builtin function.
/// This is the safe replacement for the removed `create_builtin_fn` trait method.
pub(crate) fn create_builtin_fn_static(
    ec: &mut dyn ExecutionContext<Types>,
    behaviour: fn(
        &[<Types as JsTypes>::JsValue],
        <Types as JsTypes>::JsValue,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, Types>,
    length: u32,
    name: <Types as JsTypes>::PropertyKey,
) -> <Types as JsTypes>::Function {
    // Use the ExecutionContext trait method.
    ec.create_builtin_fn_static(behaviour, length, name)
}

/// Convert a stateless raw function pointer into a constructor builtin function.
pub(crate) fn create_constructor_static(
    ec: &mut dyn ExecutionContext<Types>,
    behaviour: fn(
        &[<Types as JsTypes>::JsValue],
        <Types as JsTypes>::JsValue,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as JsTypes>::JsValue, Types>,
    length: u32,
    name: <Types as JsTypes>::PropertyKey,
) -> <Types as JsTypes>::Function {
    ec.create_builtin_function(Box::new(behaviour), length, name, true)
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
