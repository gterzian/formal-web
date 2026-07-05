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

use js_engine::{Completion, ExecutionContext, gc::Trace};

/// Create a built-in function with traceable captures.
///
/// On the Boa backend, stores the concrete captures type `C` directly in the
/// NativeFunction's GC-heap, preserving proper trace/finalize through the type
/// erasure boundary.  On JSC (no GC tracing), uses a `Behaviour` trait object.
#[cfg(jsc_backend)]
pub(crate) fn builtin_with_captures<C: Trace + 'static>(
    ec: &mut dyn ExecutionContext<Types>,
    captures: C,
    behaviour: fn(
        &[<Types as js_engine::JsTypes>::JsValue],
        <Types as js_engine::JsTypes>::JsValue,
        &C,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as js_engine::JsTypes>::JsValue, Types>,
    length: u32,
) -> <Types as js_engine::JsTypes>::Function {
    struct Captured<C> {
        captures: C,
        fn_ptr: fn(
            &[<Types as js_engine::JsTypes>::JsValue],
            <Types as js_engine::JsTypes>::JsValue,
            &C,
            &mut dyn ExecutionContext<Types>,
        ) -> Completion<<Types as js_engine::JsTypes>::JsValue, Types>,
    }

    impl<C: 'static> js_engine::Behaviour<Types> for Captured<C> {
        fn call(
            &self,
            args: &[<Types as js_engine::JsTypes>::JsValue],
            this: <Types as js_engine::JsTypes>::JsValue,
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<<Types as js_engine::JsTypes>::JsValue, Types> {
            (self.fn_ptr)(args, this, &self.captures, ec)
        }
    }

    let name = ec.property_key_from_str("");
    ec.create_builtin_function_from_behaviour(
        Box::new(Captured {
            captures,
            fn_ptr: behaviour,
        }),
        length,
        name,
    )
}

/// Create a built-in function with traceable captures (Boa backend).
///
/// Stores `C` directly in the NativeFunction heap so Boa's GC traces through
/// GcCell fields inside platform objects captured by stream/controller/reader
/// callbacks.  Without this, GC collects referenced `JsObject`s and produces
/// "TypeError: not a callable function".
#[cfg(not(jsc_backend))]
pub(crate) fn builtin_with_captures<C: Trace + 'static>(
    ec: &mut dyn ExecutionContext<Types>,
    captures: C,
    behaviour: fn(
        &[<Types as js_engine::JsTypes>::JsValue],
        <Types as js_engine::JsTypes>::JsValue,
        &C,
        &mut dyn ExecutionContext<Types>,
    ) -> Completion<<Types as js_engine::JsTypes>::JsValue, Types>,
    length: u32,
) -> <Types as js_engine::JsTypes>::Function {
    use js_engine::JsEngine;
    use js_engine::boa::{context_as_engine, ec_to_ctx};

    let name = ec.property_key_from_str("");

    // SAFETY: On the Boa backend, `ec` is always a `BoaContext` (repr
    // transparent over `Context`).  `ec_to_ctx` performs the raw pointer
    // cast from `&mut dyn ExecutionContext<BoaTypes>` to `&mut Context`,
    // which is correct because `BoaContext` is `#[repr(transparent)]`.
    let context = unsafe { ec_to_ctx(ec) };

    // context_as_engine is a safe cast from &mut Context to &mut BoaContext.
    let boa = context_as_engine(context);

    boa.create_builtin_function_with_captures(captures, behaviour, length, name)
}
