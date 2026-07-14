use std::cell::RefCell;
use std::rc::Rc;

use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
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
///
/// Note: Spec-complete but not yet wired to any domain call site.
#[allow(dead_code)]
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
    #[gc_struct]
    struct RejectionCapture<TFailure> {
        state_clone: js_engine::gc::GcCell<WaitAllState>,
        #[ignore_trace]
        failure_cell: std::rc::Rc<std::cell::RefCell<Option<TFailure>>>,
    }

    fn rejection_behaviour<TFailure>(
        args: &[JsValue],
        _this: JsValue,
        captures: &RejectionCapture<TFailure>,
        handler_ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types>
    where
        TFailure:
            FnOnce(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<(), Types> + 'static,
    {
        let arg = args
            .first()
            .cloned()
            .unwrap_or_else(|| handler_ec.value_undefined());
        let mut state_ref = captures.state_clone.borrow_mut();
        // Step 3.1: If rejected is true, abort these steps.
        if state_ref.rejected {
            return Ok(handler_ec.value_undefined());
        }
        // Step 3.2: Set rejected to true.
        state_ref.rejected = true;
        // Step 3.3: Perform failureSteps given arg.
        drop(state_ref);
        if let Some(failure_steps) = captures.failure_cell.borrow_mut().take() {
            let _ = failure_steps(arg, handler_ec);
        }
        Ok(handler_ec.value_undefined())
    }

    let name_key = ec.property_key_from_str("");
    let rejection_handler = crate::js::create_builtin_fn_with_traced_captures(
        ec,
        RejectionCapture {
            state_clone,
            failure_cell,
        },
        rejection_behaviour::<TFailure>,
        1,
        name_key.clone(),
        false,
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
        #[gc_struct]
        struct FulfillmentCapture<TSuccess> {
            state_for_fulfillment: js_engine::gc::GcCell<WaitAllState>,
            #[ignore_trace]
            success_cell_for_fulfillment: std::rc::Rc<std::cell::RefCell<Option<TSuccess>>>,
            #[ignore_trace]
            promise_index: usize,
        }

        fn fulfillment_behaviour<TSuccess>(
            args: &[JsValue],
            _this: JsValue,
            captures: &FulfillmentCapture<TSuccess>,
            handler_ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types>
        where
            TSuccess: FnOnce(Vec<JsValue>, &mut dyn ExecutionContext<Types>) -> Completion<(), Types>
                + 'static,
        {
            let arg = args
                .first()
                .cloned()
                .unwrap_or_else(|| handler_ec.value_undefined());
            let mut state_ref = captures.state_for_fulfillment.borrow_mut();
            // Step 9.2.1: Set result[promiseIndex] to arg.
            if captures.promise_index < state_ref.result.len() {
                state_ref.result[captures.promise_index] = Some(arg);
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
                    captures.success_cell_for_fulfillment.borrow_mut().take()
                {
                    let _ = success_steps(results, handler_ec);
                }
            }
            Ok(handler_ec.value_undefined())
        }

        let state_for_fulfillment = state.clone();
        let success_cell_for_fulfillment: Rc<RefCell<Option<TSuccess>>> = success_cell.clone();
        let fulfillment_handler = crate::js::create_builtin_fn_with_traced_captures(
            ec,
            FulfillmentCapture {
                state_for_fulfillment,
                success_cell_for_fulfillment,
                promise_index,
            },
            fulfillment_behaviour::<TSuccess>,
            1,
            name_key.clone(),
            false,
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
///
/// Note: Spec-complete but not yet wired to any domain call site.
#[allow(dead_code)]
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
