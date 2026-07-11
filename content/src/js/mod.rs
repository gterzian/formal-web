use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use js_engine::JsTypesWithRealm;

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
/// Generic over `T` so Web IDL infrastructure (operation.rs, attribute.rs)
/// can call it with their own type parameter.
#[cfg(not(jsc_backend))]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: fn(
        &[T::JsValue],
        T::JsValue,
        &C,
        &mut dyn ExecutionContext<T>,
    ) -> Completion<T::JsValue, T>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: js_engine::gc::Trace + 'static,
{
    js_engine::boa::create_builtin_fn_with_captures(ec, captures, behaviour, length, name, is_constructor)
}

#[cfg(jsc_backend)]
pub(crate) fn create_builtin_fn_with_traced_captures<T, C>(
    ec: &mut dyn ExecutionContext<T>,
    captures: C,
    behaviour: fn(
        &[T::JsValue],
        T::JsValue,
        &C,
        &mut dyn ExecutionContext<T>,
    ) -> Completion<T::JsValue, T>,
    length: u32,
    name: T::PropertyKey,
    is_constructor: bool,
) -> T::Function
where
    T: JsTypes + JsTypesWithRealm,
    C: 'static,
{
    js_engine::jsc::create_builtin_fn_with_captures(ec, captures, behaviour, length, name, is_constructor)
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

/// Capture for a function pointer following the getter/setter signature.
/// The fn pointer carries no GC references, so its trace is a no-op.
#[gc_struct]
pub(crate) struct FnCapture {
    #[ignore_trace]
    pub(crate) func: FnCaptureFn,
}

/// Signature for the function pointers used in getter/setter/operation captures.
pub(crate) type FnCaptureFn = fn(
    &<Types as JsTypes>::JsValue,
    &[<Types as JsTypes>::JsValue],
    &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::JsValue, Types>;

/// Behaviour: delegates to the captured fn pointer (reverse arg order).
pub(crate) fn fn_capture_behaviour(
    args: &[<Types as JsTypes>::JsValue],
    this: <Types as JsTypes>::JsValue,
    captures: &FnCapture,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::JsValue, Types> {
    (captures.func)(&this, args, ec)
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
