pub(crate) mod bindings;
mod downcast;
pub(crate) mod platform_objects;
pub(crate) use bindings::{
    install_console_namespace, install_css_namespace, install_document_property,
};
pub(crate) use downcast::{
    try_with_abort_controller_ref, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref,
    with_abort_signal_ref, with_event_mut, with_event_target_mut, with_event_target_ref,
};

/// Content-level type alias for the concrete JS types in use.
/// This is the **only** place `BoaTypes` is imported.  When we support
/// a second backend (JSC), changing this one line switches the entire crate.
pub(crate) type Types = js_engine::boa::BoaTypes;

use js_engine::JsEngine;

/// Convert a `JsResult<T>` into a `Completion<T, crate::js::Types>` by mapping
/// `JsError` errors to their opaque `JsValue` form via `context`.
///
/// This is the standard bridge used during the migration to thread
/// `ExecutionContext<T>` through domain code: functions still returning
/// `JsResult` are wrapped with this helper at call sites in
/// `Completion`-returning functions.
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

/// Generic `_ec` wrapper for `js_result_to_completion` that takes
/// `&mut dyn ExecutionContext<T>` instead of `&mut Context`.
pub(crate) fn js_result_to_completion_ec<T>(
    result: boa_engine::JsResult<T>,
    ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>,
) -> js_engine::Completion<T, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    js_result_to_completion(result, context)
}

/// Convert a `Completion<T, crate::js::Types>` back to `JsResult<T>` by wrapping
/// the error JsValue in a `JsError`.  Used as a bridge at unconverted
/// domain files that still return `JsResult` and call `Completion`-returning
/// helpers.
pub(crate) fn completion_to_js_result<T>(
    result: js_engine::Completion<T, crate::js::Types>,
) -> boa_engine::JsResult<T> {
    result.map_err(boa_engine::JsError::from_opaque)
}

/// Convert a `JsNativeError` into a `JsValue` suitable as a `Completion` error.
#[allow(dead_code)]
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
/// from `&mut Context` (the common domain-code entry point).
pub(crate) fn builtin_with_captures<C: js_engine::gc::Trace + 'static>(
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

/// EC-based version of `builtin_with_captures`.
/// Use this in functions that already have `&mut dyn ExecutionContext`.
/// Bridges to Context internally since `create_builtin_function_with_captures`
/// lives on `JsEngine<T>` (factory trait), not `ExecutionContext<T>`.
pub(crate) fn builtin_with_captures_ec<C: js_engine::gc::Trace + 'static>(
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
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    builtin_with_captures(context, captures, behaviour, length)
}

/// Convenience wrapper that creates a `Callback` from `builtin_with_captures`.
/// Used by SourceMethod-wrapped closures in streams (e.g. writeAlgorithm,
/// abortAlgorithm, closeAlgorithm).
pub(crate) fn builtin_callback<C: js_engine::gc::Trace + 'static>(
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
        builtin_with_captures(context, captures, behaviour, length).into(),
    )
}

/// EC-based version of `builtin_callback`.
/// Use this in functions that already have `&mut dyn ExecutionContext`.
pub(crate) fn builtin_callback_ec<C: js_engine::gc::Trace + 'static>(
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
        builtin_with_captures_ec(ec, captures, behaviour, length).into(),
    )
}
