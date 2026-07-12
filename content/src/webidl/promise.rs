use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::{Types, create_builtin_fn_static};

type JsValue = <Types as JsTypes>::JsValue;

/// Helper: a builtin function that ignores its arguments and returns undefined.
pub(crate) fn resolve_to_undefined_impl(
    _args: &[JsValue],
    _this: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(ec.value_undefined())
}
type JsObject = <Types as JsTypes>::JsObject;

/// **Web IDL Promise Manipulation**
///
/// Helpers for creating and transforming promises per https://webidl.spec.whatwg.org/#js-promise-manipulation
///
/// Each helper maps directly to a Web IDL operation:
/// - `resolved_promise` → § a-promise-resolved-with
/// - `rejected_promise` → § a-promise-rejected-with
/// - `promise_from_value` → § js-to-promise
/// - `transform_promise_to_undefined` → § dfn-perform-steps-once-promise-is-settled

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
pub(crate) fn resolved_promise(
    value: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let value be the result of converting x to a JavaScript value."
    // Note: Step 1 is a no-op — value is already a JsValue.
    // Step 2: "Let constructor be realm.[[Intrinsics]].[[%Promise%]]."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    // Step 3: "Let promiseCapability be ? NewPromiseCapability(constructor)."
    let capability = ec.new_promise_capability(intrinsics.promise)?;
    // Step 4: "Perform ! Call(promiseCapability.[[Resolve]], undefined, « value »)."
    let resolve_obj = <Types as JsTypes>::object_from_function(capability.resolve);
    let undefined = ec.value_undefined();
    ec.call(&resolve_obj, &undefined, &[value])?;
    // Step 5: "Return promiseCapability."
    Ok(<Types as JsTypes>::value_as_object(&capability.promise)
        .unwrap_or_else(|| ec.realm_global_object()))
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
pub(crate) fn rejected_promise(
    reason: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let constructor be realm.[[Intrinsics]].[[%Promise%]]."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    // Step 2: "Let promiseCapability be ? NewPromiseCapability(constructor)."
    let capability = ec.new_promise_capability(intrinsics.promise)?;
    // Step 3: "Perform ! Call(promiseCapability.[[Reject]], undefined, « r »)."
    let reject_obj = <Types as JsTypes>::object_from_function(capability.reject);
    let undefined = ec.value_undefined();
    ec.call(&reject_obj, &undefined, &[reason])?;
    // Step 4: "Return promiseCapability."
    Ok(<Types as JsTypes>::value_as_object(&capability.promise)
        .unwrap_or_else(|| ec.realm_global_object()))
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
///
/// Converts a value into a promise, following the "JS-to-promise" coercion rules.
pub(crate) fn promise_from_value(
    value: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: "Let promiseCapability be ? NewPromiseCapability(%Promise%)."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let capability = ec.new_promise_capability(intrinsics.promise)?;
    // Step 2: "Perform ? Call(promiseCapability.[[Resolve]], undefined, « V »)."
    let resolve_obj = <Types as JsTypes>::object_from_function(capability.resolve);
    let undefined = ec.value_undefined();
    ec.call(&resolve_obj, &undefined, &[value])?;
    // Step 3: "Return promiseCapability."
    Ok(<Types as JsTypes>::value_as_object(&capability.promise)
        .unwrap_or_else(|| ec.realm_global_object()))
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
///
/// Creates a rejected promise from a `JsValue` error reason.
pub(crate) fn rejected_promise_from_error(
    error: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsObject {
    rejected_promise(error, ec).unwrap_or_else(|_| ec.realm_global_object())
}

/// <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
///
/// Chains a promise to return `undefined` when settled.  Implements the pattern:
/// "React to promise with a fulfillment step that returns undefined."
pub(crate) fn transform_promise_to_undefined(
    promise_object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let not_promise_err =
        ec.new_type_error("transform_promise_to_undefined: value is not a Promise");
    let name_key = ec.property_key_from_str("");
    let on_fulfilled = create_builtin_fn_static(ec, resolve_to_undefined_impl, 1, name_key);
    // Step 7 of react: "PerformPromiseThen(promise, onFulfilled, ...)."
    // Note: We pass None for result_capability because our trait impl
    // ignores it (calls promise.then() which creates its own).  The
    // returned promise resolves to undefined when promise_object settles.
    let js_promise =
        <Types as JsTypes>::object_as_promise(promise_object).ok_or_else(|| not_promise_err)?;
    let result = ec.perform_promise_then(js_promise, Some(on_fulfilled), None, None)?;
    // Step 8 of react: "Return newCapability."
    Ok(<Types as JsTypes>::value_as_object(&result).unwrap_or_else(|| ec.realm_global_object()))
}

/// <https://webidl.spec.whatwg.org/#mark-a-promise-as-handled>
///
/// Marks a promise as "handled" to suppress unhandled-rejection warnings.
pub(crate) fn mark_promise_as_handled(
    promise_object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    let not_promise_err = ec.new_type_error("mark_promise_as_handled: value is not a Promise");
    let name_key = ec.property_key_from_str("");
    let on_rejected = create_builtin_fn_static(ec, resolve_to_undefined_impl, 1, name_key);
    // PerformPromiseThen with rejection-only handler.
    let js_promise =
        <Types as JsTypes>::object_as_promise(promise_object).ok_or_else(|| not_promise_err)?;
    ec.perform_promise_then(js_promise, None, Some(on_rejected), None)?;
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#react>
///
/// Reacts to a promise with optional fulfillment and rejection steps.
/// Wraps NewPromiseCapability + PerformPromiseThen into a single call.
/// Returns the new promise capability's promise.
///
/// `on_fulfilled` and `on_rejected` are already-created JS functions
/// (e.g. from `create_builtin_fn_static` or `create_builtin_fn_with_traced_captures`).
pub(crate) fn upon_settlement(
    promise: JsObject,
    on_fulfilled: Option<<Types as JsTypes>::Function>,
    on_rejected: Option<<Types as JsTypes>::Function>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let realm = ec.current_realm();
    let global = ec.realm_global_object();
    let not_promise_err = ec.new_type_error("upon_settlement: value is not a Promise");

    // Step 5 of react: Let constructor be %Promise%.
    let intrinsics = ec.realm_intrinsics(&realm);
    let promise_constructor = intrinsics.promise;

    // Step 6 of react: Let newCapability be ? NewPromiseCapability(constructor).
    let capability = ec.new_promise_capability(promise_constructor)?;
    let result_promise = capability.promise.clone();

    // Step 7 of react: PerformPromiseThen(promise, onFulfilled, onRejected, newCapability).
    let js_promise =
        <Types as JsTypes>::object_as_promise(&promise).ok_or_else(|| not_promise_err.clone())?;
    ec.perform_promise_then(js_promise, on_fulfilled, on_rejected, Some(capability))?;

    // Step 8 of react: Return newCapability.
    Ok(Types::value_as_object(&result_promise).unwrap_or(global))
}
