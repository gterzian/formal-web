use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};

/// **Web IDL Promise Manipulation**
/// 
/// Helpers for creating and transforming promises per https://webidl.spec.whatwg.org/#js-promise-manipulation
///
/// When an algorithm in a spec needs to create or return a promise, these helpers provide the
/// Web IDL-canonical way to do so. Each helper maps directly to a Web IDL operation:
/// - `resolved_promise` → § a-promise-resolved-with
/// - `rejected_promise` → § a-promise-rejected-with
/// - `promise_from_value` → § js-to-promise
/// - `transform_promise_to_undefined` → § dfn-perform-steps-once-promise-is-settled
///
/// Call sites should use these helpers when converting Rust-side exceptions to promise
/// rejections or when implementing spec operations that need to return settled promises.

/// Creates a promise already resolved with the given value.
/// 
/// Implements: https://webidl.spec.whatwg.org/#a-promise-resolved-with
/// Step 1: "Return a promise resolved with value."
pub(crate) fn resolved_promise(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::resolve(value, context)?.into())
}

/// Creates a promise already rejected with the given reason.
/// 
/// Implements: https://webidl.spec.whatwg.org/#a-promise-rejected-with
/// Step 1: "Return a promise rejected with reason."
pub(crate) fn rejected_promise(reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::reject(JsError::from_opaque(reason), context)?.into())
}

/// Converts a value into a promise, following the "JS-to-promise" coercion rules.
/// 
/// Implements: https://webidl.spec.whatwg.org/#js-to-promise
/// This is used when an algorithm receives a value that might be a promise or any other value:
/// Step 1: "Let promiseCapability be ? NewPromiseCapability(%Promise%)."
/// Step 2: "Perform ? Call(promiseCapability.[[Resolve]], undefined, « V »)."
/// Step 3: "Return promiseCapability."
/// 
/// Note: `Promise.resolve(value)` implements these steps directly.
pub(crate) fn promise_from_value(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    Ok(JsPromise::resolve(value, context)?.into())
}

/// Creates a rejected promise from a `JsError`, using the Web IDL coercion rules.
/// 
/// When an internal Rust error must be surfaced as a promise rejection, this helper
/// ensures the error is converted to a JS exception object following Web IDL's
/// error-to-rejection-reason mapping. Falls back to a TypeError with a generic message
/// if conversion fails.
pub(crate) fn rejected_promise_from_error(error: JsError, context: &mut Context) -> JsObject {
    rejected_promise(error_to_rejection_reason(error, context), context).unwrap_or_else(|_| {
        let (promise, resolvers) = JsPromise::new_pending(context);
        let _ = resolvers.reject.call(
            &JsValue::undefined(),
            &[JsValue::undefined()],
            context,
        );
        promise.into()
    })
}

/// Converts a Rust `JsError` into a JS rejection reason following Web IDL rules.
/// 
/// Per https://webidl.spec.whatwg.org/#a-promise-rejected-with, a rejection reason must be
/// a JS value, not an internal error. This helper unwraps opaque error values or converts
/// Rust-internal exceptions into serializable JS exceptions.
pub(crate) fn error_to_rejection_reason(error: JsError, context: &mut Context) -> JsValue {
    if let Some(reason) = error.as_opaque().cloned() {
        return reason;
    }

    match error.into_opaque(context) {
        Ok(reason) => reason,
        Err(_) => JsNativeError::typ()
            .with_message(
                "Promise-returning operation could not convert an internal error into a rejection reason",
            )
            .into_opaque(context)
            .into(),
    }
}

/// Chains a promise to return `undefined` when settled.
/// 
/// Implements the pattern described in: https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled
/// "React to promise with a fulfillment step that returns undefined."
/// 
/// Used when a promise-returning operation needs to propagate the promise's settlement
/// (and suppress its value/reason), as in many stream pipe-through scenarios.
pub(crate) fn transform_promise_to_undefined(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<JsObject> {
    let on_fulfilled = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    Ok(JsPromise::from_object(promise_object.clone())?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

/// Marks a promise as "handled" to suppress unhandled-rejection warnings.
/// 
/// Called when a spec algorithm attaches an internal reaction to a promise but doesn't
/// expose that promise to user code, preventing spurious unhandled rejection warnings.
/// Adds an inert catch handler through Boa's native promise hooks.
pub(crate) fn mark_promise_as_handled(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let on_rejected = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    let _ = JsPromise::from_object(promise_object.clone())?.catch(on_rejected, context)?;
    Ok(())
}

fn return_undefined(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}