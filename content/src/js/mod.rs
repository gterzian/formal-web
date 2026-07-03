#[cfg(boa_backend)]
pub(crate) mod bindings;
#[cfg(boa_backend)]
mod downcast;
#[cfg(boa_backend)]
pub(crate) mod platform_objects;

/// Generic engine builder — the single entry point for creating a JS engine
/// context.  Uses `#[cfg]` internally to switch between Boa and JSC backends.
pub(crate) mod build_context;

/// Generic bootstrap module — uses only [`ExecutionContext<T>`] trait methods.
/// Not engine-specific; only compiled when the Boa-specific bindings are
/// not available (JSC backend).
#[cfg(not(boa_backend))]
pub(crate) mod console_generic;

#[cfg(boa_backend)]
pub(crate) use bindings::{
    install_console_namespace, install_css_namespace, install_document_property,
};

/// Generic console namespace installer available on all backends.
#[cfg(not(boa_backend))]
pub(crate) use console_generic::install_console_namespace;
#[cfg(boa_backend)]
pub(crate) use downcast::{
    try_with_abort_controller_ref, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref,
    with_abort_signal_ref, with_event_mut, with_event_target_mut, with_event_target_ref,
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
#[cfg(boa_backend)]
pub(crate) type Engine = js_engine::boa::BoaContext;

#[cfg(not(boa_backend))]
pub(crate) type Engine = js_engine::jsc::JscEngine;

use js_engine::JsEngine;

/// Convert a `JsResult<T>` into a `Completion<T, crate::js::Types>` by mapping
/// `JsError` errors to their opaque `JsValue` form via `context`.
///
/// This is the standard bridge used during the migration to thread
/// `ExecutionContext<T>` through domain code: functions still returning
/// `JsResult` are wrapped with this helper at call sites in
/// `Completion`-returning functions.
#[cfg(boa_backend)]
#[allow(dead_code)]
pub(crate) fn js_result_to_completion<T>(
    result: boa_engine::JsResult<T>,
    context: &mut boa_engine::Context,
) -> js_engine::Completion<T, crate::js::Types> {
    result.map_err(|e| {
        e.into_opaque(context)
            .unwrap_or_else(|_| boa_engine::JsValue::undefined())
    })
}

/// Convert a `JsNativeError` into a `JsValue` suitable as a `Completion` error.
#[cfg(boa_backend)]
pub(crate) fn native_error_to_js_value(
    error: boa_engine::JsNativeError,
    context: &mut boa_engine::Context,
) -> boa_engine::JsValue {
    let js_error: boa_engine::JsError = error.into();
    js_error
        .into_opaque(context)
        .unwrap_or_else(|_| boa_engine::JsValue::undefined())
}

/// Convenience wrapper for `create_builtin_function_with_captures` that works
/// from `&mut Context` (the legacy domain-code entry point).
#[cfg(boa_backend)]
pub(crate) fn builtin_with_captures_ctx<C: js_engine::gc::Trace + 'static>(
    context: &mut boa_engine::Context,
    captures: C,
    behaviour: fn(
        &[boa_engine::JsValue],
        boa_engine::JsValue,
        &C,
        &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types>,
    length: u32,
) -> boa_engine::object::builtins::JsFunction {
    let name = boa_engine::property::PropertyKey::from(boa_engine::js_string!(""));
    js_engine::boa::context_as_engine(context)
        .create_builtin_function_with_captures(captures, behaviour, length, name)
}

/// Convenience wrapper: creates a builtin function with captures through
/// the [`ExecutionContext::create_builtin_function_from_behaviour`] method.
/// Zero bridges — no `ec_to_ctx`, no unsafe.
#[cfg(boa_backend)]
pub(crate) fn builtin_with_captures<C: js_engine::gc::Trace + 'static>(
    ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    captures: C,
    behaviour: fn(
        &[boa_engine::JsValue],
        boa_engine::JsValue,
        &C,
        &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types>,
    length: u32,
) -> boa_engine::object::builtins::JsFunction {
    struct Captured<C> {
        captures: C,
        fn_ptr: fn(
            &[boa_engine::JsValue],
            boa_engine::JsValue,
            &C,
            &mut dyn js_engine::ExecutionContext<crate::js::Types>,
        ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types>,
    }

    impl<C: 'static> js_engine::Behaviour<crate::js::Types> for Captured<C> {
        fn call(
            &self,
            args: &[boa_engine::JsValue],
            this: boa_engine::JsValue,
            ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>,
        ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types> {
            (self.fn_ptr)(args, this, &self.captures, ec)
        }
    }

    let name = boa_engine::property::PropertyKey::from(boa_engine::js_string!(""));
    ec.create_builtin_function_from_behaviour(
        Box::new(Captured {
            captures,
            fn_ptr: behaviour,
        }),
        length,
        name,
    )
}

/// Convenience wrapper that creates a `Callback` from `builtin_with_captures_ctx`.
/// Used by SourceMethod-wrapped closures in streams (e.g. writeAlgorithm,
/// abortAlgorithm, closeAlgorithm).
#[cfg(boa_backend)]
pub(crate) fn builtin_callback_ctx<C: js_engine::gc::Trace + 'static>(
    context: &mut boa_engine::Context,
    captures: C,
    behaviour: fn(
        &[boa_engine::JsValue],
        boa_engine::JsValue,
        &C,
        &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types>,
    length: u32,
) -> crate::webidl::Callback {
    crate::webidl::Callback::from_object(
        builtin_with_captures_ctx(context, captures, behaviour, length).into(),
    )
}

/// Convenience wrapper that creates a `Callback` from `builtin_with_captures`.
/// Used by SourceMethod-wrapped closures in streams that already take EC.
#[cfg(boa_backend)]
pub(crate) fn builtin_callback<C: js_engine::gc::Trace + 'static>(
    ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    captures: C,
    behaviour: fn(
        &[boa_engine::JsValue],
        boa_engine::JsValue,
        &C,
        &mut dyn js_engine::ExecutionContext<crate::js::Types>,
    ) -> js_engine::Completion<boa_engine::JsValue, crate::js::Types>,
    length: u32,
) -> crate::webidl::Callback {
    crate::webidl::Callback::from_object(
        builtin_with_captures(ec, captures, behaviour, length).into(),
    )
}
