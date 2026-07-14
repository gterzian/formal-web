use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

pub(crate) mod bindings;
pub(crate) mod build_context;
pub(crate) mod builtin_fn;
pub(crate) mod console_generic;
pub(crate) mod css_generic;
/// Generic platform-object downcast helpers.
pub(crate) mod downcast;
/// Generic platform-object resolution helpers.
pub(crate) mod platform_objects;

pub(crate) use console_generic::install_console_namespace;
pub(crate) use css_generic::install_css_namespace;

pub(crate) use bindings::install_document_property;
pub(crate) use builtin_fn::create_builtin_fn_with_traced_captures;
pub(crate) use downcast::{
    try_with_abort_controller_ref, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref,
    with_abort_signal_ref,
};

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
