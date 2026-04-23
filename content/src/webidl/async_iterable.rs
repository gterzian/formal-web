use std::{cell::Cell, rc::Rc};

use boa_engine::{
    Context, JsArgs, JsData, JsError, JsNativeError, JsResult, JsValue,
    builtins::{iterable::create_iter_result_object, object::OrdinaryObject},
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, JsObject, ObjectInitializer, builtins::JsPromise},
    property::Attribute,
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use super::promise::{rejected_promise, resolved_promise};

#[derive(Clone, Trace, Finalize)]
enum IteratorOperation {
    Next,
    Return(JsValue),
}

/// <https://webidl.spec.whatwg.org/#asynchronous-iterator-initialization-steps>
pub(crate) trait AsyncValueIterable: Clone + Trace + Finalize + 'static {
    type State: Clone + Trace + Finalize + 'static;

    fn create_async_iterator_state(
        &self,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self::State>;

    fn get_next_iteration_result(
        &self,
        state: &Self::State,
        context: &mut Context,
    ) -> JsResult<JsObject>;

    fn finish_async_iterator(
        &self,
        _state: &Self::State,
        _context: &mut Context,
    ) -> JsResult<()> {
        Ok(())
    }

    fn has_async_iterator_return() -> bool {
        false
    }

    fn return_async_iterator(
        &self,
        _state: &Self::State,
        _value: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        resolved_promise(JsValue::undefined(), context)
    }
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterable>
pub(crate) fn create_value_async_iterator<T>(
    target: T,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsObject>
where
    T: AsyncValueIterable,
{
    // Step 6: "Let iterator be a newly created default asynchronous iterator object for definition with idlObject as its target, \"value\" as its kind, and is finished set to false."
    // Step 7: "Run the asynchronous iterator initialization steps for definition with idlObject, iterator, and idlArgs, if any such steps exist."
    // Note: No current content-runtime async iterable needs the JavaScript iterator object's identity during initialization, so the interface-specific hook returns the iterator state before the wrapper object is allocated.
    let state = target.create_async_iterator_state(args, context)?;

    let iterator = DefaultAsyncIterator::new(target, state);

    // Step 8: "Return iterator."
    create_default_async_iterator_object(iterator, context)
}

/// <https://webidl.spec.whatwg.org/#js-default-asynchronous-iterator-object>
#[derive(Clone, Trace, Finalize, JsData)]
struct DefaultAsyncIterator<T>
where
    T: AsyncValueIterable,
{
    target: T,
    state: T::State,
    last_operation: Gc<GcRefCell<Option<JsObject>>>,
    #[unsafe_ignore_trace]
    finished: Rc<Cell<bool>>,
}

impl<T> DefaultAsyncIterator<T>
where
    T: AsyncValueIterable,
{
    fn new(target: T, state: T::State) -> Self {
        Self {
            target,
            state,
            last_operation: Gc::new(GcRefCell::new(None)),
            finished: Rc::new(Cell::new(false)),
        }
    }

    fn queue_operation(
        &self,
        operation: IteratorOperation,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        let promise = if let Some(previous) = self.last_operation.borrow().clone() {
            let on_settled = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_copy_closure_with_captures(
                    |_, _, captures: &(DefaultAsyncIterator<T>, IteratorOperation), context| {
                        let (iterator, operation) = captures;
                        Ok(JsValue::from(iterator.start_operation(operation.clone(), context)?))
                    },
                    (self.clone(), operation),
                ),
            )
            .name(js_string!())
            .length(0)
            .constructor(false)
            .build();

            JsPromise::from_object(previous)?
                .then(Some(on_settled.clone()), Some(on_settled), context)?
                .into()
        } else {
            self.start_operation(operation, context)?
        };

        *self.last_operation.borrow_mut() = Some(promise.clone());
        Ok(promise)
    }

    fn start_operation(
        &self,
        operation: IteratorOperation,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        match operation {
            IteratorOperation::Next => self.start_next(context),
            IteratorOperation::Return(ref value) => self.start_return(value.clone(), context),
        }
    }

    /// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
    fn start_next(&self, context: &mut Context) -> JsResult<JsObject> {
        // Step 8.2: "If object's is finished is true, then:"
        if self.finished.get() {
            return resolved_promise(
                create_iter_result_object(JsValue::undefined(), true, context),
                context,
            );
        }

        // Step 8.4: "Let nextPromise be the result of getting the next iteration result with object's target and object."
        let next_promise = match self.target.get_next_iteration_result(&self.state, context) {
            Ok(next_promise) => next_promise,
            Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
        };

        let on_fulfilled = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |_, args, iterator: &DefaultAsyncIterator<T>, context| {
                    let result = args.get_or_undefined(0).clone();

                    if let Some(result_object) = result.as_object() {
                        let done = result_object.get(js_string!("done"), context)?.to_boolean();

                        // Step 8.5.2: "If next is end of iteration, then:"
                        if done {
                            iterator.finished.set(true);
                            iterator
                                .target
                                .finish_async_iterator(&iterator.state, context)?;
                        }
                    }

                    Ok(result)
                },
                self.clone(),
            ),
        )
        .name(js_string!())
        .length(1)
        .constructor(false)
        .build();

        let on_rejected = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |_, args, iterator: &DefaultAsyncIterator<T>, context| {
                    // Step 8.7.2: "Set object's is finished to true."
                    iterator.finished.set(true);

                    iterator
                        .target
                        .finish_async_iterator(&iterator.state, context)?;

                    // Step 8.7.3: "Throw reason."
                    Err(JsError::from_opaque(args.get_or_undefined(0).clone()))
                },
                self.clone(),
            ),
        )
        .name(js_string!())
        .length(1)
        .constructor(false)
        .build();

        // Step 8.9: "Perform PerformPromiseThen(nextPromise, onFulfilled, onRejected, nextPromiseCapability)."
        JsPromise::from_object(next_promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)
            .map(Into::into)
    }

    /// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
    fn start_return(&self, value: JsValue, context: &mut Context) -> JsResult<JsObject> {
        // Step 8.2: "If object's is finished is true, then:"
        if self.finished.get() {
            return resolved_promise(create_iter_result_object(value, true, context), context);
        }

        if !T::has_async_iterator_return() {
            self.finished.set(true);
            return resolved_promise(create_iter_result_object(value, true, context), context);
        }

        // Step 8.3: "Set object's is finished to true."
        self.finished.set(true);

        // Step 8.4: "Return the result of running the asynchronous iterator return algorithm for interface, given object's target, object, and value."
        let return_promise = match self
            .target
            .return_async_iterator(&self.state, value.clone(), context)
        {
            Ok(return_promise) => return_promise,
            Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
        };

        let on_fulfilled = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |_, _, value: &JsValue, context| {
                    Ok(create_iter_result_object(value.clone(), true, context))
                },
                value,
            ),
        )
        .name(js_string!())
        .length(0)
        .constructor(false)
        .build();

        let on_rejected = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_fn_ptr(|_, args, _| {
                Err(JsError::from_opaque(args.get_or_undefined(0).clone()))
            }),
        )
        .name(js_string!())
        .length(1)
        .constructor(false)
        .build();

        // Step 14: "Perform PerformPromiseThen(object's ongoing promise, onFulfilled, undefined, returnPromiseCapability)."
        JsPromise::from_object(return_promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)
            .map(Into::into)
    }
}

fn create_default_async_iterator_object<T>(
    iterator: DefaultAsyncIterator<T>,
    context: &mut Context,
) -> JsResult<JsObject>
where
    T: AsyncValueIterable,
{
    let prototype = create_async_iterator_prototype::<T>(context);
    Ok(ObjectInitializer::with_native_data_and_proto(iterator, prototype, context).build())
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
fn create_async_iterator_prototype<T>(context: &mut Context) -> JsObject
where
    T: AsyncValueIterable,
{
    let realm = context.realm().clone();

    let next = FunctionObjectBuilder::new(
        &realm,
        NativeFunction::from_fn_ptr(async_iterator_next::<T>),
    )
    .name(js_string!("next"))
    .length(0)
    .constructor(false)
    .build();

    let async_iterator_prototype = context
        .intrinsics()
        .objects()
        .iterator_prototypes()
        .async_iterator();
    let mut initializer = ObjectInitializer::with_native_data_and_proto(
        OrdinaryObject,
        async_iterator_prototype,
        context,
    );
    initializer.property(js_string!("next"), next, Attribute::all());

    if T::has_async_iterator_return() {
        let return_method = FunctionObjectBuilder::new(
            &realm,
            NativeFunction::from_fn_ptr(async_iterator_return::<T>),
        )
        .name(js_string!("return"))
        .length(1)
        .constructor(false)
        .build();

        initializer.property(js_string!("return"), return_method, Attribute::all());
    }

    initializer.build()
}

fn default_async_iterator_from_this<T>(
    this: &JsValue,
    context: &mut Context,
) -> Result<DefaultAsyncIterator<T>, JsError>
where
    T: AsyncValueIterable,
{
    let iterator_object = this.to_object(context)?;

    iterator_object
        .downcast_ref::<DefaultAsyncIterator<T>>()
        .map(|iterator| (*iterator).clone())
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("object is not a default asynchronous iterator")
                .into()
        })
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
fn async_iterator_next<T>(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue>
where
    T: AsyncValueIterable,
{
    // Step 4: "Let object be Completion(ToObject(thisValue))."
    // Step 5: "IfAbruptRejectPromise(object, thisValidationPromiseCapability)."
    let iterator = match default_async_iterator_from_this::<T>(this, context) {
        Ok(iterator) => iterator,
        Err(error) => return Ok(JsValue::from(rejected_promise(error.into_opaque(context)?, context)?)),
    };

    // Step 12: "Return object's ongoing promise."
    Ok(JsValue::from(iterator.queue_operation(IteratorOperation::Next, context)?))
}

/// <https://webidl.spec.whatwg.org/#js-asynchronous-iterator-prototype-object>
fn async_iterator_return<T>(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue>
where
    T: AsyncValueIterable,
{
    let value = args.get_or_undefined(0).clone();

    // Step 4: "Let object be Completion(ToObject(thisValue))."
    // Step 5: "IfAbruptRejectPromise(object, returnPromiseCapability)."
    let iterator = match default_async_iterator_from_this::<T>(this, context) {
        Ok(iterator) => iterator,
        Err(error) => return Ok(JsValue::from(rejected_promise(error.into_opaque(context)?, context)?)),
    };

    // Step 15: "Return returnPromiseCapability.[[Promise]]."
    Ok(JsValue::from(iterator.queue_operation(IteratorOperation::Return(value), context)?))
}