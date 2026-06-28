pub(crate) mod bindings;
mod downcast;
pub(crate) mod platform_objects;
pub(crate) use bindings::{
    install_console_namespace, install_css_namespace, install_document_property,
};
pub(crate) use downcast::{
    with_abort_controller_ref, with_abort_signal_mut, with_abort_signal_ref, with_event_mut,
    with_event_target_mut, with_event_target_ref,
};

// Content-local alias for the concrete engine type.
// This is the only place `BoaEngine` is imported by name from `js_engine`.
pub(crate) use js_engine::boa::BoaEngine as Engine;

/// Convert a `JsResult<T>` into a `Completion<T, BoaTypes>` by mapping
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
) -> js_engine::Completion<T, js_engine::boa::BoaTypes> {
    result.map_err(|e| {
        e.into_opaque(context)
            .unwrap_or_else(|_| boa_engine::JsValue::undefined())
    })
}

/// Convert a `Completion<T, BoaTypes>` back to `JsResult<T>` by wrapping
/// the error JsValue in a `JsError`.  Used as a bridge at unconverted
/// domain files that still return `JsResult` and call `Completion`-returning
/// helpers.
pub(crate) fn completion_to_js_result<T>(
    result: js_engine::Completion<T, js_engine::boa::BoaTypes>,
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

/// SAFETY: `BoaEngine` is `#[repr(transparent)]` over `Context`, so a
/// `&mut Context` can be safely cast to `&mut BoaEngine`.  This cast is
/// used to pass Boa's Context through the generic `ExecutionContext<BoaTypes>`
/// trait interface.  The engine must NOT be moved during the borrow.
/// SAFETY: `BoaEngine` is `#[repr(transparent)]` over `Context`, so a
/// `&mut Context` can be safely cast to `&mut BoaEngine`.  This cast is
/// used to pass Boa's Context through the generic `ExecutionContext<BoaTypes>`
/// or `JsEngine<BoaTypes>` trait interface.  The engine must NOT be moved
/// during the borrow.
pub(crate) fn context_as_engine(context: &mut boa_engine::Context) -> &mut Engine {
    // SAFETY: BoaEngine has the same repr as Context (repr(transparent)),
    // and this function produces a reference with the same lifetime as the input.
    unsafe { &mut *(context as *mut boa_engine::Context as *mut Engine) }
}

pub(crate) fn context_as_ec(
    context: &mut boa_engine::Context,
) -> &mut dyn js_engine::ExecutionContext<js_engine::boa::BoaTypes> {
    context_as_engine(context)
}

pub(crate) fn context_as_ec_ref(
    context: &boa_engine::Context,
) -> &dyn js_engine::ExecutionContext<js_engine::boa::BoaTypes> {
    unsafe { &*(context as *const boa_engine::Context as *const Engine) }
}

/// SAFETY: Convert a `&mut dyn ExecutionContext<BoaTypes>` back to `&mut Context`
/// via the `#[repr(transparent)]` guarantee of `BoaEngine` over `Context`.
/// Used in binding functions that need to call existing helpers taking `&mut Context`.
/// SAFETY: Convert a `&mut dyn ExecutionContext<BoaTypes>` back to `&mut Context`
/// via the `#[repr(transparent)]` guarantee of `BoaEngine` over `Context`.
/// Currently unused — available for binding functions that need to call
/// existing domain helpers taking `&mut Context`.
#[allow(dead_code)]
pub(crate) unsafe fn ec_to_ctx<'a>(
    ec: &'a mut dyn js_engine::ExecutionContext<js_engine::boa::BoaTypes>,
) -> &'a mut boa_engine::Context {
    // SAFETY: BoaEngine is repr(transparent) over Context, so the data pointer
    // of dyn ExecutionContext<BoaTypes> points to a BoaEngine whose first field is Context.
    unsafe {
        &mut *(ec as *mut dyn js_engine::ExecutionContext<js_engine::boa::BoaTypes> as *mut Engine
            as *mut boa_engine::Context)
    }
}
