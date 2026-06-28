use boa_engine::{
    Context, JsError, JsNativeError, JsValue,
    builtins::promise::ResolvingFunctions,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};
use log::error;

/// **Web IDL Promise Manipulation**
///
/// Helpers for creating and transforming promises per https://webidl.spec.whatwg.org/#js-promise-manipulation
///
/// Each helper maps directly to a Web IDL operation:
/// - `resolved_promise` → § a-promise-resolved-with
/// - `rejected_promise` → § a-promise-rejected-with
/// - `promise_from_value` → § js-to-promise
/// - `transform_promise_to_undefined` → § dfn-perform-steps-once-promise-is-settled

/// <https://webidl.spec.whatwg.org/#a-new-promise>
pub(crate) fn a_new_promise(
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> (JsObject, ResolvingFunctions) {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let (promise, resolvers) = JsPromise::new_pending(context);
    (promise.into(), resolvers)
}

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
pub(crate) fn resolved_promise(
    value: JsValue,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    // Step 1: "Return a promise resolved with value."
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    JsPromise::resolve(value, context)
        .map(JsObject::from)
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
pub(crate) fn rejected_promise(
    reason: JsValue,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    // Step 1: "Return a promise rejected with reason."
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    JsPromise::reject(JsError::from_opaque(reason.clone()), context)
        .map(JsObject::from)
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
///
/// Converts a value into a promise, following the "JS-to-promise" coercion rules.
/// Step 1: "Let promiseCapability be ? NewPromiseCapability(%Promise%)."
/// Step 2: "Perform ? Call(promiseCapability.[[Resolve]], undefined, « V »)."
/// Step 3: "Return promiseCapability."
///
/// Note: `Promise.resolve(value)` implements these steps directly.
pub(crate) fn promise_from_value(
    value: JsValue,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    JsPromise::resolve(value, context)
        .map(JsObject::from)
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
///
/// Converts a completion result into a `Promise`, rejecting it when the completion throws.
pub(crate) fn promise_from_completion(
    completion: boa_engine::JsResult<JsValue>,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsPromise {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    JsPromise::from_result(completion, context).unwrap_or_else(|error| {
        JsPromise::from_object(rejected_promise_from_error(error, ec))
            .expect("rejected_promise_from_error must return a Promise object")
    })
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
///
/// Creates a rejected promise from a `JsError`, using the Web IDL coercion rules.
/// Falls back to a TypeError with a generic message if conversion fails.
pub(crate) fn rejected_promise_from_error(
    error: JsError,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsObject {
    let reason = error_to_rejection_reason(error, ec);
    if let Ok(promise) = rejected_promise(reason, ec) {
        return promise;
    }
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let (promise, resolvers) = JsPromise::new_pending(context);
    if let Err(error) =
        resolvers
            .reject
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)
    {
        error!(
            "[webidl] failed to reject fallback promise in rejected_promise_from_error: {error}"
        );
    }
    promise.into()
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
///
/// Converts a Rust `JsError` into a JS rejection reason.
/// Unwraps opaque error values or converts Rust-internal exceptions into serializable
/// JS exceptions.
pub(crate) fn error_to_rejection_reason(
    error: JsError,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsValue {
    if let Some(reason) = error.as_opaque().cloned() {
        return reason;
    }

    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    match error.into_opaque(context) {
        Ok(reason) => reason,
        Err(_js_error) => JsNativeError::typ()
            .with_message(
                "Promise-returning operation could not convert an internal error into a rejection reason",
            )
            .into_opaque(context)
            .into(),
    }
}

/// <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
///
/// Chains a promise to return `undefined` when settled.  Implements the pattern:
/// "React to promise with a fulfillment step that returns undefined."
pub(crate) fn transform_promise_to_undefined(
    promise_object: &JsObject,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let on_fulfilled =
        NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    JsPromise::from_object(promise_object.clone())
        .and_then(|p| p.then(Some(on_fulfilled), None, context))
        .map(JsObject::from)
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
}

/// <https://webidl.spec.whatwg.org/#mark-a-promise-as-handled>
///
/// Marks a promise as "handled" to suppress unhandled-rejection warnings.
pub(crate) fn mark_promise_as_handled(
    promise_object: &JsObject,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<(), BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };
    let on_rejected = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    let _ = JsPromise::from_object(promise_object.clone())
        .and_then(|p| p.catch(on_rejected, context))
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
    Ok(())
}

fn return_undefined(_: &JsValue, _: &[JsValue], _: &mut Context) -> boa_engine::JsResult<JsValue> {
    Ok(JsValue::undefined())
}
