pub(crate) mod bindings;
mod downcast;
pub(crate) mod platform_objects;
pub(crate) use bindings::{
    install_console_namespace, install_css_namespace, install_document_property,
};
pub(crate) use downcast::{
    try_with_abort_controller_ref,
    with_abort_signal_ref, with_event_mut, with_event_target_mut, with_event_target_ref,
};

/// Content-level type alias for the concrete JS types in use.
/// This is the **only** place `BoaTypes` is imported.  When we support
/// a second backend (JSC), changing this one line switches the entire crate.
pub(crate) type Types = js_engine::boa::BoaTypes;

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
