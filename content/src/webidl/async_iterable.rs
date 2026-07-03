use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use std::{cell::Cell, rc::Rc};

use js_engine::{Behaviour, Completion, ExecutionContext, JsTypes};

use super::promise::resolved_promise;

type Types = crate::js::Types;
type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

// ── Helper to convert a JsObject to a Promise ────────────────────────────

fn promise_from_object(
    obj: JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<<Types as JsTypes>::Promise, Types> {
    Types::object_as_promise(&obj).ok_or_else(|| ec.new_type_error("value is not a Promise"))
}

// ── Iterator operation ────────────────────────────────────────────────────

#[gc_struct]
enum IteratorOperation {
    Next,
    Return(JsValue),
}

// ── Behaviour captures for builtin functions ──────────────────────────────

/// Behaviour for onFulfilled of `start_next`:
/// checks result's done property, sets finished state, returns iter result.
struct NextOnFulfilled<T: AsyncValueIterable> {
    iterator: DefaultAsyncIterator<T>,
}

impl<T: AsyncValueIterable> Behaviour<Types> for NextOnFulfilled<T> {
    fn call(
        &self,
        args: &[JsValue],
        _this: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        let result = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());

        // Try to extract the "done" property from the resolved value.
        // If it's an iterator result object ({value, done}), check done.
        if let Some(result_object) = Types::value_as_object(&result) {
            let done_result = js_engine::EcmascriptHost::get(ec, &result_object, "done");
            if let Ok(done_val) = done_result {
                if ec.to_boolean(&done_val) {
                    // Step 8.5.2: "If next is end of iteration, then:"
                    self.iterator.finished.set(true);
                    self.iterator
                        .target
                        .finish_async_iterator(&self.iterator.state, ec)?;
                    // Return CreateIteratorResultObject(undefined, true)
                    return Ok(Types::value_from_object(create_iterator_result_object(
                        ec.value_undefined(),
                        true,
                        ec,
                    )));
                }
            }
        }

        // Step 8.5.4: Return the result as-is (the value is the iteration value)
        Ok(result)
    }
}

/// Behaviour for onRejected of `start_next`:
/// sets finished state and throws the reason.
struct NextOnRejected<T: AsyncValueIterable> {
    iterator: DefaultAsyncIterator<T>,
}

impl<T: AsyncValueIterable> Behaviour<Types> for NextOnRejected<T> {
    fn call(
        &self,
        args: &[JsValue],
        _this: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        // Step 8.7.2: "Set object's is finished to true."
        self.iterator.finished.set(true);

        self.iterator
            .target
            .finish_async_iterator(&self.iterator.state, ec)?;

        // Step 8.7.3: "Throw reason."
        let reason = args
            .first()
            .cloned()
            .unwrap_or_else(|| ec.value_undefined());
        Err(reason)
    }
}

/// Behaviour for onSettled of `queue_operation`:
/// chains the next operation after a previous promise settles.
struct OperationOnSettled<T: AsyncValueIterable> {
    iterator: DefaultAsyncIterator<T>,
    operation: IteratorOperation,
}

impl<T: AsyncValueIterable> Behaviour<Types> for OperationOnSettled<T> {
    fn call(
        &self,
        _args: &[JsValue],
        _this: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        let promise = self.iterator.start_operation(self.operation.clone(), ec)?;
        Ok(Types::value_from_object(promise))
    }
}

/// Behaviour for onFulfilled of `start_return`:
/// returns CreateIteratorResultObject(value, true) regardless of fulfillment value.
struct ReturnOnFulfilled {
    value: JsValue,
}

impl Behaviour<Types> for ReturnOnFulfilled {
    fn call(
        &self,
        _args: &[JsValue],
        _this: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        // Step 12.1: "Return CreateIteratorResultObject(value, true)."
        Ok(Types::value_from_object(create_iterator_result_object(
            self.value.clone(),
            true,
            ec,
        )))
    }
}

/// Behaviour for onRejected of `start_return`:
/// re-throws the rejection reason.
struct ReThrowRejected;

impl Behaviour<Types> for ReThrowRejected {
    fn call(
        &self,
        args: &[JsValue],
        _this: JsValue,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        Err(args
            .first()
            .cloned()
            .unwrap_or_else(|| unreachable!("Rejection should have a reason")))
    }
}

// ── AsyncValueIterable trait ──────────────────────────────────────────────

/// <https://webidl.spec.whatwg.org/#asynchronous-iterator-initialization-steps>
pub(crate) trait AsyncValueIterable:
    Clone + js_engine::gc::Trace + js_engine::gc::Finalize + 'static
{
    type State: Clone + js_engine::gc::Trace + js_engine::gc::Finalize + 'static;

    fn create_async_iterator_state(
        &self,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self::State, Types>;

    fn get_next_iteration_result(
        &self,
        state: &Self::State,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types>;

    fn finish_async_iterator(
        &self,
        _state: &Self::State,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        Ok(())
    }

    fn has_async_iterator_return() -> bool {
        false
    }

    fn return_async_iterator(
        &self,
        _state: &Self::State,
        _value: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: "Return a promise resolved with undefined."
        resolved_promise(ec.value_undefined(), ec)
    }
}

// ── DefaultAsyncIterator ──────────────────────────────────────────────────

/// <https://webidl.spec.whatwg.org/#js-default-asynchronous-iterator-object>
#[derive(Clone)]
#[cfg_attr(feature = "boa", derive(boa_gc::Finalize, boa_gc::Trace))]
struct DefaultAsyncIterator<T>
where
    T: AsyncValueIterable,
{
    target: T,
    state: T::State,
    ongoing_promise: GcCell<Option<JsObject>>,
    #[ignore_trace]
    finished: Rc<Cell<bool>>,
}

#[cfg(not(boa_backend))]
unsafe impl<T: AsyncValueIterable> js_engine::gc::Trace for DefaultAsyncIterator<T> {}
#[cfg(not(boa_backend))]
impl<T: AsyncValueIterable> js_engine::gc::Finalize for DefaultAsyncIterator<T> {}

impl<T> DefaultAsyncIterator<T>
where
    T: AsyncValueIterable,
{
    fn new(target: T, state: T::State) -> Self {
        Self {
            target,
            state,
            ongoing_promise: gc_cell_new(None),
            finished: Rc::new(Cell::new(false)),
        }
    }

    /// Queue an async iterator operation, chaining after any ongoing promise.
    /// Corresponds to spec Steps 9–11 of "invoke the next property".
    fn queue_operation(
        &self,
        operation: IteratorOperation,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 9: "Let ongoingPromise be object's ongoing promise."
        if let Some(previous) = self.ongoing_promise.borrow().clone() {
            // Step 10: "If ongoingPromise is not null, then:"
            // Step 10.1: "Let afterOngoingPromiseCapability be ! NewPromiseCapability(%Promise%)."
            let after_capability = ec
                .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
                .map_err(|e| e)?;
            let after_promise = after_capability.promise.clone();

            // Step 10.2: "Let onSettled be CreateBuiltinFunction(nextSteps, 0, "", « »)."
            let on_settled_fn = ec.create_builtin_function_from_behaviour(
                Box::new(OperationOnSettled {
                    iterator: self.clone(),
                    operation,
                }),
                0,
                ec.property_key_from_str(""),
            );

            // Step 10.3: "Perform PerformPromiseThen(ongoingPromise, onSettled, onSettled, afterOngoingPromiseCapability)."
            let previous_promise = promise_from_object(previous, ec)?;
            ec.perform_promise_then(
                previous_promise,
                Some(on_settled_fn.clone()),
                Some(on_settled_fn),
                Some(after_capability),
            )?;

            // Step 10.4: "Set object's ongoing promise to afterOngoingPromiseCapability.[[Promise]]."
            let result_obj =
                Types::value_as_object(&after_promise).unwrap_or_else(|| ec.realm_global_object());
            *self.ongoing_promise.borrow_mut() = Some(result_obj.clone());
            Ok(result_obj)
        } else {
            // Step 11: "Otherwise:"
            // Step 11.1: "Set object's ongoing promise to the result of running nextSteps."
            let promise = self.start_operation(operation, ec)?;
            *self.ongoing_promise.borrow_mut() = Some(promise.clone());
            Ok(promise)
        }
    }

    fn start_operation(
        &self,
        operation: IteratorOperation,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        match operation {
            IteratorOperation::Next => self.start_next(ec),
            IteratorOperation::Return(ref value) => self.start_return(value.clone(), ec),
        }
    }

    /// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
    /// Core nextSteps from step 8 of "invoke the next property".
    fn start_next(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
        // Step 8.1: "Let nextPromiseCapability be ! NewPromiseCapability(%Promise%)."
        let next_capability = ec
            .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
            .map_err(|e| e)?;
        let next_promise_value = next_capability.promise.clone();

        // Step 8.2: "If object's is finished is true, then:"
        if self.finished.get() {
            let result = create_iterator_result_object(ec.value_undefined(), true, ec);
            let resolve_obj = Types::object_from_function(next_capability.resolve);
            let undefined = ec.value_undefined();
            ec.call(
                &resolve_obj,
                &undefined,
                &[Types::value_from_object(result)],
            )?;
            return Ok(Types::value_as_object(&next_promise_value)
                .unwrap_or_else(|| ec.realm_global_object()));
        }

        // Step 8.4: "Let nextPromise be the result of getting the next iteration result with object's target and object."
        let next_promise = match self.target.get_next_iteration_result(&self.state, ec) {
            Ok(promise_obj) => promise_obj,
            Err(error) => {
                let reject_obj = Types::object_from_function(next_capability.reject);
                let undefined = ec.value_undefined();
                ec.call(&reject_obj, &undefined, &[error])?;
                return Ok(Types::value_as_object(&next_promise_value)
                    .unwrap_or_else(|| ec.realm_global_object()));
            }
        };

        // Step 8.5–8.6: Create onFulfilled
        let on_fulfilled = ec.create_builtin_function_from_behaviour(
            Box::new(NextOnFulfilled {
                iterator: self.clone(),
            }),
            1,
            ec.property_key_from_str(""),
        );

        // Step 8.7–8.8: Create onRejected
        let on_rejected = ec.create_builtin_function_from_behaviour(
            Box::new(NextOnRejected {
                iterator: self.clone(),
            }),
            1,
            ec.property_key_from_str(""),
        );

        // Step 8.9: "Perform PerformPromiseThen(nextPromise, onFulfilled, onRejected, nextPromiseCapability)."
        let next_promise_obj = promise_from_object(next_promise, ec)?;
        ec.perform_promise_then(
            next_promise_obj,
            Some(on_fulfilled),
            Some(on_rejected),
            Some(next_capability),
        )?;

        // Step 8.10: "Return nextPromiseCapability.[[Promise]]."
        Ok(Types::value_as_object(&next_promise_value).unwrap_or_else(|| ec.realm_global_object()))
    }

    /// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
    /// Core returnSteps from step 8 of "invoke the return property".
    fn start_return(
        &self,
        value: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 8.1: "Let returnPromiseCapability be ! NewPromiseCapability(%Promise%)."
        let return_capability = ec
            .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
            .map_err(|e| e)?;
        let return_promise_value = return_capability.promise.clone();

        // Step 8.2: "If object's is finished is true, then:"
        if self.finished.get() {
            let result = create_iterator_result_object(value, true, ec);
            let resolve_obj = Types::object_from_function(return_capability.resolve);
            let undefined = ec.value_undefined();
            ec.call(
                &resolve_obj,
                &undefined,
                &[Types::value_from_object(result)],
            )?;
            return Ok(Types::value_as_object(&return_promise_value)
                .unwrap_or_else(|| ec.realm_global_object()));
        }

        // Step 8.3: "Set object's is finished to true."
        self.finished.set(true);

        // Step 8.4: "Return the result of running the asynchronous iterator return algorithm for interface..."
        let return_promise = if !T::has_async_iterator_return() {
            let result = create_iterator_result_object(value.clone(), true, ec);
            resolved_promise(Types::value_from_object(result), ec)?
        } else {
            match self
                .target
                .return_async_iterator(&self.state, value.clone(), ec)
            {
                Ok(promise_obj) => promise_obj,
                Err(error) => {
                    let reject_obj = Types::object_from_function(return_capability.reject);
                    let undefined = ec.value_undefined();
                    ec.call(&reject_obj, &undefined, &[error])?;
                    return Ok(Types::value_as_object(&return_promise_value)
                        .unwrap_or_else(|| ec.realm_global_object()));
                }
            }
        };

        // Step 12–13: "Let onFulfilled be CreateBuiltinFunction(fulfillSteps, 1, "", « »)."
        let on_fulfilled = ec.create_builtin_function_from_behaviour(
            Box::new(ReturnOnFulfilled {
                value: value.clone(),
            }),
            1,
            ec.property_key_from_str(""),
        );

        let on_rejected = ec.create_builtin_function_from_behaviour(
            Box::new(ReThrowRejected),
            1,
            ec.property_key_from_str(""),
        );

        // Step 14: "Perform PerformPromiseThen(object's ongoing promise, onFulfilled, undefined, returnPromiseCapability)."
        let return_promise_obj = promise_from_object(return_promise, ec)?;
        ec.perform_promise_then(
            return_promise_obj,
            Some(on_fulfilled),
            Some(on_rejected),
            Some(return_capability),
        )?;

        // Step 15: "Return returnPromiseCapability.[[Promise]]."
        Ok(Types::value_as_object(&return_promise_value)
            .unwrap_or_else(|| ec.realm_global_object()))
    }
}

// ── Helper: CreateIteratorResultObject ─────────────────────────────────────

/// <https://tc39.es/ecma262/#sec-createiterresultobject>
fn create_iterator_result_object(
    value: JsValue,
    done: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsObject {
    let obj = ec.create_plain_object(None);
    let done_value = ec.value_from_bool(done);
    let _ = ec.object_set_property(obj.clone(), "value", value);
    let _ = ec.object_set_property(obj.clone(), "done", done_value);
    obj
}

// Note: Workaround for the move-after-move pattern above —
// we need to avoid cloning JsObject on every call.

// ── Prototype and object construction ─────────────────────────────────────

fn create_async_iterator_prototype<T>(ec: &mut dyn ExecutionContext<Types>) -> JsObject
where
    T: AsyncValueIterable,
{
    let intrinsics = ec.realm_intrinsics(&ec.current_realm());
    let async_iterator_proto = intrinsics.async_iterator_prototype;

    let prototype = ec.create_plain_object(Some(&async_iterator_proto));

    // Create the `next` method — a built-in function that delegates to
    // async_iterator_next_inner.
    let next_fn = ec.create_builtin_function(
        Box::new(
            |args: &[JsValue], this: JsValue, ec: &mut dyn ExecutionContext<Types>| {
                async_iterator_next_inner::<T>(this, args, ec)
            },
        ),
        0,
        ec.property_key_from_str("next"),
    );
    let next_fn_value = Types::value_from_object(Types::object_from_function(next_fn));
    let _ = ec.object_set_property(prototype.clone(), "next", next_fn_value);

    // Create the `return` method if the interface has a return algorithm
    if T::has_async_iterator_return() {
        let return_fn = ec.create_builtin_function(
            Box::new(
                |args: &[JsValue], this: JsValue, ec: &mut dyn ExecutionContext<Types>| {
                    async_iterator_return_inner::<T>(this, args, ec)
                },
            ),
            1,
            ec.property_key_from_str("return"),
        );
        let return_fn_value = Types::value_from_object(Types::object_from_function(return_fn));
        let _ = ec.object_set_property(prototype.clone(), "return", return_fn_value);
    }

    prototype
}

fn create_default_async_iterator_object<T>(
    iterator: DefaultAsyncIterator<T>,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsObject
where
    T: AsyncValueIterable,
{
    let prototype = create_async_iterator_prototype::<T>(ec);
    ec.create_object_with_any(prototype, Box::new(iterator))
}

// ── Binding function helpers ───────────────────────────────────────────────

fn default_async_iterator_from_this<T>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<DefaultAsyncIterator<T>, Types>
where
    T: AsyncValueIterable,
{
    let obj = ec.to_object(this.clone())?;
    let cloned = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<DefaultAsyncIterator<T>>())
        .cloned();
    cloned.ok_or_else(|| ec.new_type_error("object is not a default asynchronous iterator"))
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
fn async_iterator_next_inner<T>(
    this: JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types>
where
    T: AsyncValueIterable,
{
    // Steps 2-5: this validation
    let iterator = match default_async_iterator_from_this::<T>(&this, ec) {
        Ok(iterator) => iterator,
        Err(error) => {
            // Step 5: "IfAbruptRejectPromise(object, thisValidationPromiseCapability)."
            let capability = ec
                .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
                .map_err(|e| e)?;
            let reject_obj = Types::object_from_function(capability.reject);
            let undefined = ec.value_undefined();
            ec.call(&reject_obj, &undefined, &[error])?;
            return Ok(capability.promise);
        }
    };

    // Step 12: "Return object's ongoing promise."
    let promise = iterator.queue_operation(IteratorOperation::Next, ec)?;
    Ok(Types::value_from_object(promise))
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
fn async_iterator_return_inner<T>(
    this: JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types>
where
    T: AsyncValueIterable,
{
    let value = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined());

    // Steps 2-5: this validation
    let iterator = match default_async_iterator_from_this::<T>(&this, ec) {
        Ok(iterator) => iterator,
        Err(error) => {
            let capability = ec
                .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
                .map_err(|e| e)?;
            let reject_obj = Types::object_from_function(capability.reject);
            let undefined = ec.value_undefined();
            ec.call(&reject_obj, &undefined, &[error])?;
            return Ok(capability.promise);
        }
    };

    // Queue the return operation (handles ongoing promise chaining)
    let return_result = iterator.queue_operation(IteratorOperation::Return(value.clone()), ec)?;

    // Step 12–15: Wrap the return result through onFulfilled (CreateIteratorResultObject)
    let on_fulfilled = ec.create_builtin_function_from_behaviour(
        Box::new(ReturnOnFulfilled { value }),
        1,
        ec.property_key_from_str(""),
    );

    let on_rejected = ec.create_builtin_function_from_behaviour(
        Box::new(ReThrowRejected),
        1,
        ec.property_key_from_str(""),
    );

    let capability = ec
        .new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)
        .map_err(|e| e)?;
    let result_promise = capability.promise.clone();

    let return_promise_obj = promise_from_object(return_result, ec)?;
    ec.perform_promise_then(
        return_promise_obj,
        Some(on_fulfilled),
        Some(on_rejected),
        Some(capability),
    )?;

    // Step 15: "Return returnPromiseCapability.[[Promise]]."
    Ok(result_promise)
}

// ── Public entry point ────────────────────────────────────────────────────

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterable>
pub(crate) fn create_value_async_iterator<T>(
    target: T,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types>
where
    T: AsyncValueIterable,
{
    // Step 6: "Let iterator be a newly created default asynchronous iterator object for definition with idlObject as its target, \"value\" as its kind, and is finished set to false."
    // Step 7: "Run the asynchronous iterator initialization steps for definition with idlObject, iterator, and idlArgs, if any such steps exist."
    let state = target.create_async_iterator_state(args, ec)?;

    let iterator = DefaultAsyncIterator::new(target, state);

    // Step 8: "Return iterator."
    Ok(create_default_async_iterator_object(iterator, ec))
}
