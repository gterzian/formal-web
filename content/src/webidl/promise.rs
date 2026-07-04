use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;
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

/// <https://webidl.spec.whatwg.org/#a-new-promise>
pub(crate) fn a_new_promise(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(JsObject, js_engine::PromiseResolvers<Types>), Types> {
    let (promise, resolvers) = ec.new_promise_pending()?;
    let promise_obj =
        <Types as JsTypes>::value_as_object(&promise).unwrap_or_else(|| ec.realm_global_object());
    Ok((promise_obj, resolvers))
}

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

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
///
/// Converts a `JsValue` error into a JS rejection reason (identity, already a value).
pub(crate) fn error_to_rejection_reason(
    error: JsValue,
    _ec: &mut dyn ExecutionContext<Types>,
) -> JsValue {
    error
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
    // Step 1-2 of react: CreateBuiltinFunction returning undefined on fulfillment.
    let on_fulfilled = ec.create_builtin_function(
        Box::new(
            |_args: &[JsValue],
             _this: JsValue,
             on_fulfilled_ec: &mut dyn ExecutionContext<Types>| {
                Ok(on_fulfilled_ec.value_undefined())
            },
        ),
        1,
        ec.property_key_from_str(""),
    );
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
    // CreateBuiltinFunction returning undefined on rejection.
    let on_rejected = ec.create_builtin_function(
        Box::new(
            |_args: &[JsValue],
             _this: JsValue,
             on_rejected_ec: &mut dyn ExecutionContext<Types>| {
                Ok(on_rejected_ec.value_undefined())
            },
        ),
        1,
        ec.property_key_from_str(""),
    );
    // PerformPromiseThen with rejection-only handler.
    let js_promise =
        <Types as JsTypes>::object_as_promise(promise_object).ok_or_else(|| not_promise_err)?;
    ec.perform_promise_then(js_promise, None, Some(on_rejected), None)?;
    Ok(())
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types>
where
    F: FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> + 'static,
{
    upon_settlement::<F, fn(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>>(
        promise,
        Some(steps),
        None,
        ec,
    )
}

/// <https://webidl.spec.whatwg.org/#upon-rejection>
///
/// Performs steps upon rejection of a promise.  Returns a new promise
/// that resolves with the result of the steps (or rejects if the steps
/// return a rejected promise).
pub(crate) fn upon_rejection<R>(
    promise: JsObject,
    steps: R,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types>
where
    R: FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> + 'static,
{
    upon_settlement::<fn(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>, R>(
        promise,
        None,
        Some(steps),
        ec,
    )
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types>
where
    F: FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> + 'static,
    R: FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> + 'static,
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
    let on_fulfilled_fn: Option<<Types as JsTypes>::Function> = if fulfilled_cell.is_some() {
        let cell = fulfilled_cell.unwrap();
        Some(ec.create_builtin_function(
            Box::new(
                move |args: &[JsValue],
                      _this: JsValue,
                      inner_ec: &mut dyn ExecutionContext<Types>|
                      -> Completion<JsValue, Types> {
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
    let on_rejected_fn: Option<<Types as JsTypes>::Function> = if rejected_cell.is_some() {
        let cell = rejected_cell.unwrap();
        Some(ec.create_builtin_function(
            Box::new(
                move |args: &[JsValue],
                      _this: JsValue,
                      inner_ec: &mut dyn ExecutionContext<Types>|
                      -> Completion<JsValue, Types> {
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
        <Types as JsTypes>::object_as_promise(&promise).ok_or_else(|| not_promise_err.clone())?;
    ec.perform_promise_then(
        js_promise,
        on_fulfilled_fn,
        on_rejected_fn,
        Some(capability),
    )?;

    // Step 8 of react: Return newCapability.
    Ok(Types::value_as_object(&result_promise).unwrap_or(global))
}

// ── Wait for all ──────────────────────────────────────────────────────
//
// Implements <https://webidl.spec.whatwg.org/#wait-for-all> and
// <https://webidl.spec.whatwg.org/#get-a-promise-to-wait-for-all>.

/// Shared mutable state for the "wait for all" algorithm.
#[gc_struct]
struct WaitAllState {
    /// Number of promises fulfilled so far (= Step 1's fulfilledCount).
    #[ignore_trace]
    fulfilled_count: usize,

    /// Whether any promise has rejected (= Step 2's rejected flag).
    #[ignore_trace]
    rejected: bool,

    /// Total number of promises to wait for (= Step 5's total).
    #[ignore_trace]
    total: usize,

    /// Collected results indexed by promise position.
    /// Initialized with total null values (Step 8).
    result: Vec<Option<JsValue>>,
}

/// <https://webidl.spec.whatwg.org/#wait-for-all>
///
/// Wait for all with a list of Promise<T> values, performing success steps
/// (given a list of T values) or failure steps (given a rejection reason).
pub(crate) fn wait_for_all<TSuccess, TFailure>(
    promises: Vec<JsObject>,
    success_steps: TSuccess,
    failure_steps: TFailure,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types>
where
    TSuccess:
        FnOnce(Vec<JsValue>, &mut dyn ExecutionContext<Types>) -> Completion<(), Types> + 'static,
    TFailure: FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<(), Types> + 'static,
{
    // Step 1: Let fulfilledCount be 0.
    // Step 2: Let rejected be false.
    let state = gc_cell_new(WaitAllState {
        fulfilled_count: 0,
        rejected: false,
        total: 0,
        result: Vec::new(),
    });

    // Wrap FnOnce steps in Rc<RefCell<Option<...>>> so they can be shared
    // across multiple closures (the rejection handler and each iteration's
    // fulfillment handler in the for loop).
    let success_cell: Rc<RefCell<Option<TSuccess>>> = Rc::new(RefCell::new(Some(success_steps)));
    let failure_cell: Rc<RefCell<Option<TFailure>>> = Rc::new(RefCell::new(Some(failure_steps)));

    // Clone `state` before capturing in the rejection handler so we
    // can still use it after creating that closure.
    let state_clone = state.clone();

    // Step 3: Let rejectionHandlerSteps be the following steps given arg:
    //   3.1: If rejected is true, abort these steps.
    //   3.2: Set rejected to true.
    //   3.3: Perform failureSteps given arg.
    // Step 4: Let rejectionHandler be CreateBuiltinFunction(rejectionHandlerSteps, 1, "", « »).
    let rejection_handler = ec.create_builtin_function(
        Box::new(
            move |args: &[JsValue],
                  _this: JsValue,
                  handler_ec: &mut dyn ExecutionContext<Types>|
                  -> Completion<JsValue, Types> {
                let arg = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| handler_ec.value_undefined());
                let mut state_ref = state_clone.borrow_mut();
                // Step 3.1: If rejected is true, abort these steps.
                if state_ref.rejected {
                    return Ok(handler_ec.value_undefined());
                }
                // Step 3.2: Set rejected to true.
                state_ref.rejected = true;
                // Step 3.3: Perform failureSteps given arg.
                drop(state_ref);
                if let Some(failure_steps) = failure_cell.borrow_mut().take() {
                    let _ = failure_steps(arg, handler_ec);
                }
                Ok(handler_ec.value_undefined())
            },
        ),
        1,
        ec.property_key_from_str(""),
    );

    // Step 5: Let total be promises's size.
    let total = promises.len();
    state.borrow_mut().total = total;

    // Step 6: If total is 0, then:
    if total == 0 {
        // Step 6.1: Queue a microtask to perform successSteps given « ».
        let realm = ec.current_realm();
        ec.enqueue_job_with_realm(
            realm,
            Box::new(move |job_ec: &mut dyn ExecutionContext<Types>| {
                if let Some(success_steps) = success_cell.borrow_mut().take() {
                    let _ = success_steps(Vec::new(), job_ec);
                }
            }),
        );
        // Step 6.2: Return.
        return Ok(());
    }

    // Step 7: Let index be 0.
    // Step 8: Let result be a list containing total null values.
    {
        let null_values: Vec<Option<JsValue>> = (0..total).map(|_| None).collect();
        state.borrow_mut().result = null_values;
    }

    // Step 9: For each promise of promises:
    for (promise_index, promise) in promises.into_iter().enumerate() {
        // Step 9.1: Let promiseIndex be index.
        // (already `promise_index` from enumerate)

        // Step 9.2: Let fulfillmentHandler be the following steps given arg:
        //   9.2.1: Set result[promiseIndex] to arg.
        //   9.2.2: Set fulfilledCount to fulfilledCount + 1.
        //   9.2.3: If fulfilledCount equals total, then perform successSteps given result.
        // Step 9.3: Let fulfillmentHandler be CreateBuiltinFunction(fulfillmentHandler, 1, "", « »).
        let state_for_fulfillment = state.clone();
        let success_cell_for_fulfillment: Rc<RefCell<Option<TSuccess>>> = success_cell.clone();
        let fulfillment_handler = ec.create_builtin_function(
            Box::new(
                move |args: &[JsValue],
                      _this: JsValue,
                      handler_ec: &mut dyn ExecutionContext<Types>|
                      -> Completion<JsValue, Types> {
                    let arg = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| handler_ec.value_undefined());
                    let mut state_ref = state_for_fulfillment.borrow_mut();
                    // Step 9.2.1: Set result[promiseIndex] to arg.
                    if promise_index < state_ref.result.len() {
                        state_ref.result[promise_index] = Some(arg);
                    }
                    // Step 9.2.2: Set fulfilledCount to fulfilledCount + 1.
                    state_ref.fulfilled_count += 1;
                    // Step 9.2.3: If fulfilledCount equals total, then perform successSteps given result.
                    if state_ref.fulfilled_count == state_ref.total {
                        let results: Vec<JsValue> = state_ref
                            .result
                            .iter()
                            .map(|opt| opt.clone().unwrap_or_else(|| handler_ec.value_undefined()))
                            .collect();
                        drop(state_ref);
                        if let Some(success_steps) =
                            success_cell_for_fulfillment.borrow_mut().take()
                        {
                            let _ = success_steps(results, handler_ec);
                        }
                    }
                    Ok(handler_ec.value_undefined())
                },
            ),
            1,
            ec.property_key_from_str(""),
        );

        // Step 9.4: Perform PerformPromiseThen(promise, fulfillmentHandler, rejectionHandler).
        let js_promise = <Types as JsTypes>::object_as_promise(&promise)
            .ok_or_else(|| ec.new_type_error("wait_for_all: value is not a Promise"))?;
        ec.perform_promise_then(
            js_promise,
            Some(fulfillment_handler),
            Some(rejection_handler.clone()),
            None,
        )?;

        // Step 9.5: Set index to index + 1.
        // (handled by for loop)
    }

    Ok(())
}

/// <https://webidl.spec.whatwg.org/#get-a-promise-to-wait-for-all>
///
/// Creates a new Promise that resolves when all promises in the list have
/// fulfilled, or rejects on the first rejection.
pub(crate) fn wait_for_all_get_promise(
    promises: Vec<JsObject>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let promise be a new promise of type Promise<sequence<T>> in realm.
    // Note: new_promise_pending returns (promise_value, resolvers), not a capability struct.
    let (promise_value, resolvers) = ec.new_promise_pending()?;
    let promise_obj = <Types as JsTypes>::value_as_object(&promise_value)
        .unwrap_or_else(|| ec.realm_global_object());

    // Share resolvers between success and failure closures via Rc.
    let resolvers = Rc::new(resolvers);

    // Step 2: Let successSteps be the following steps, given results:
    //   2.1: Resolve promise with results.
    // Step 3: Let failureSteps be the following steps, given reason:
    //   3.1: Reject promise with reason.
    // Step 4: Wait for all with promises, given successSteps and failureSteps.
    let resolvers_for_success = resolvers.clone();
    wait_for_all(
        promises,
        Box::new(
            move |results: Vec<JsValue>, inner_ec: &mut dyn ExecutionContext<Types>| {
                // Step 2.1: Resolve promise with results.
                let resolve: JsObject = resolvers_for_success.resolve.clone().into();
                let undefined = inner_ec.value_undefined();
                // Convert results to JS array for Promise<sequence<T>>
                let array = inner_ec.create_empty_array();
                for value in results {
                    inner_ec.array_push(&array, value)?;
                }
                inner_ec.call(&resolve, &undefined, &[Types::value_from_object(array)])?;
                Ok(())
            },
        ),
        Box::new(
            move |reason: JsValue, inner_ec: &mut dyn ExecutionContext<Types>| {
                // Step 3.1: Reject promise with reason.
                let reject: JsObject = resolvers.reject.clone().into();
                let undefined = inner_ec.value_undefined();
                inner_ec.call(&reject, &undefined, &[reason])?;
                Ok(())
            },
        ),
        ec,
    )?;

    // Step 5: Return promise.
    Ok(promise_obj)
}
