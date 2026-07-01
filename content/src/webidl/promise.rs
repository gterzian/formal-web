use std::cell::RefCell;

use boa_engine::{
    Context, JsError, JsNativeError, JsValue,
    builtins::promise::ResolvingFunctions,
    native_function::NativeFunction,
    object::{
        JsObject,
        builtins::{JsFunction, JsPromise},
    },
};

use js_engine::{Completion, ExecutionContext, JsTypes};
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
#[inline]
pub(crate) fn a_new_promise(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> (JsObject, ResolvingFunctions) {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    a_new_promise_boa(context)
}

/// <https://webidl.spec.whatwg.org/#a-new-promise>
pub(crate) fn a_new_promise_boa(context: &mut Context) -> (JsObject, ResolvingFunctions) {
    let (promise, resolvers) = JsPromise::new_pending(context);
    (promise.into(), resolvers)
}

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
pub(crate) fn resolved_promise(
    value: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Return a promise resolved with value."
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    resolved_promise_boa(value, context)
}

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
fn resolved_promise_boa(
    value: JsValue,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Return a promise resolved with value."
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Return a promise rejected with reason."
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    rejected_promise_boa(reason, context)
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
fn rejected_promise_boa(
    reason: JsValue,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Return a promise rejected with reason."
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    promise_from_value_boa(value, context)
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
fn promise_from_value_boa(
    value: JsValue,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsPromise {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    promise_from_completion_boa(completion, context)
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
fn promise_from_completion_boa(
    completion: boa_engine::JsResult<JsValue>,
    context: &mut Context,
) -> JsPromise {
    JsPromise::from_result(completion, context).unwrap_or_else(|error| {
        JsPromise::from_object(rejected_promise_from_error_boa(error, context))
            .expect("rejected_promise_from_error must return a Promise object")
    })
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
///
/// Creates a rejected promise from a `JsError`, using the Web IDL coercion rules.
/// Falls back to a TypeError with a generic message if conversion fails.
pub(crate) fn rejected_promise_from_error(
    error: JsError,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsObject {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    rejected_promise_from_error_boa(error, context)
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
fn rejected_promise_from_error_boa(error: JsError, context: &mut Context) -> JsObject {
    let reason = error_to_rejection_reason_boa(error, context);
    if let Ok(promise) = resolved_promise_boa(reason, context) {
        return promise;
    }
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsValue {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    error_to_rejection_reason_boa(error, context)
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
fn error_to_rejection_reason_boa(error: JsError, context: &mut Context) -> JsValue {
    if let Some(reason) = error.as_opaque().cloned() {
        return reason;
    }

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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    transform_promise_to_undefined_boa(promise_object, context)
}

/// <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
fn transform_promise_to_undefined_boa(
    promise_object: &JsObject,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    mark_promise_as_handled_boa(promise_object, context)
}

/// <https://webidl.spec.whatwg.org/#mark-a-promise-as-handled>
fn mark_promise_as_handled_boa(
    promise_object: &JsObject,
    context: &mut Context,
) -> Completion<(), crate::js::Types> {
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

// ── Web IDL Promise Reaction (upon fulfillment / upon rejection) ────────
//
// Implements <https://webidl.spec.whatwg.org/#upon-fulfillment>,
// <https://webidl.spec.whatwg.org/#upon-rejection>, and the underlying
// <https://webidl.spec.whatwg.org/#react> algorithm.
//
// These wrap the engine-specific promise-chaining mechanism so that domain
// code never reaches for NativeFunction::from_closure / to_js_function.
// For the Boa backend the closure is registered via NativeFunction; for JSC
// it would use the JSC callback API.

/// <https://webidl.spec.whatwg.org/#upon-fulfillment>
///
/// Performs steps upon fulfillment of a promise.  Returns a new promise
/// that resolves with the result of the steps.
pub(crate) fn upon_fulfillment<F>(
    promise: JsObject,
    steps: F,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types>
where
    F: FnOnce(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>
        + 'static,
{
    upon_settlement::<
        F,
        fn(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>,
    >(promise, Some(steps), None, ec)
}

/// <https://webidl.spec.whatwg.org/#upon-rejection>
///
/// Performs steps upon rejection of a promise.  Returns a new promise
/// that resolves with the result of the steps (or rejects if the steps
/// return a rejected promise).
pub(crate) fn upon_rejection<R>(
    promise: JsObject,
    steps: R,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types>
where
    R: FnOnce(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>
        + 'static,
{
    upon_settlement::<
        fn(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>,
        R,
    >(promise, None, Some(steps), ec)
}

/// <https://webidl.spec.whatwg.org/#react>
///
/// Reacts to a promise with optional fulfillment and rejection steps.
/// Wraps CreateBuiltinFunction + NewPromiseCapability + PerformPromiseThen
/// into a single call.  Returns the new promise capability's promise.
pub(crate) fn upon_settlement<F, R>(
    promise: JsObject,
    on_fulfilled: Option<F>,
    on_rejected: Option<R>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types>
where
    F: FnOnce(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>
        + 'static,
    R: FnOnce(
            JsValue,
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>
        + 'static,
{
    // Extract everything we need from `ec` before creating closures,
    // since create_builtin_function takes &mut self.
    let realm = ec.current_realm();
    let global = ec.realm_global_object();
    let not_promise_err = ec.new_type_error("upon_settlement: value is not a Promise");

    // Wrap FnOnce steps in RefCell so they satisfy the Fn bound required
    // by create_builtin_function.  Each callback is called at most once
    // (promise reactions are single-fire).
    let fulfilled_cell = on_fulfilled.map(|s| RefCell::new(Some(s)));
    let rejected_cell = on_rejected.map(|s| RefCell::new(Some(s)));

    // Step 2 of react: CreateBuiltinFunction(onFulfilledSteps, 1, "", « »)
    let on_fulfilled_fn: Option<<crate::js::Types as JsTypes>::Function> =
        if fulfilled_cell.is_some() {
            let cell = fulfilled_cell.unwrap();
            Some(ec.create_builtin_function(
                Box::new(
                    move |args: &[JsValue],
                          _this: JsValue,
                          inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                          -> Completion<JsValue, crate::js::Types> {
                        let value = args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| inner_ec.value_undefined());
                        if let Some(steps) = cell.borrow_mut().take() {
                            steps(value, inner_ec)
                        } else {
                            Ok(inner_ec.value_undefined())
                        }
                    },
                ),
                1,
                ec.property_key_from_str(""),
            ))
        } else {
            None
        };

    // Step 4 of react: CreateBuiltinFunction(onRejectedSteps, 1, "", « »)
    let on_rejected_fn: Option<<crate::js::Types as JsTypes>::Function> =
        if rejected_cell.is_some() {
            let cell = rejected_cell.unwrap();
            Some(ec.create_builtin_function(
                Box::new(
                    move |args: &[JsValue],
                          _this: JsValue,
                          inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                          -> Completion<JsValue, crate::js::Types> {
                        let reason = args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| inner_ec.value_undefined());
                        if let Some(steps) = cell.borrow_mut().take() {
                            steps(reason, inner_ec)
                        } else {
                            Ok(inner_ec.value_undefined())
                        }
                    },
                ),
                1,
                ec.property_key_from_str(""),
            ))
        } else {
            None
        };

    // Step 5 of react: Let constructor be %Promise%.
    let intrinsics = ec.realm_intrinsics(&realm);
    let promise_constructor = intrinsics.promise;

    // Step 6 of react: Let newCapability be ? NewPromiseCapability(constructor).
    let capability = ec.new_promise_capability(promise_constructor)?;
    let result_promise = capability.promise.clone();

    // Step 7 of react: PerformPromiseThen(promise, onFulfilled, onRejected, newCapability).
    let js_promise =
        <crate::js::Types as JsTypes>::object_as_promise(&promise)
            .ok_or_else(|| not_promise_err.clone())?;
    ec.perform_promise_then(
        js_promise,
        on_fulfilled_fn,
        on_rejected_fn,
        Some(capability),
    )?;

    // Step 8 of react: Return newCapability.
    Ok(crate::js::Types::value_as_object(&result_promise).unwrap_or(global))
}
