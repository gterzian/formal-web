use boa_engine::{
    Context, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString, JsValue,
    class::Class,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::streams::SizeAlgorithm;

use super::{
    ReadRequest, ReadableStream, ReadableStreamController, ReadableStreamState, SourceMethod,
    promise_from_value, range_error_value, rejected_promise, resolved_promise,
};
use super::stream::{
    readable_stream_add_read_request, readable_stream_close, readable_stream_error,
    readable_stream_fulfill_read_request, readable_stream_get_num_read_requests,
};

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum PullAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl PullAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    fn call(
        &self,
        controller: &ReadableStreamDefaultController,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), context),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller.object()?);
                match callback.call(&[arg], context) {
                    Ok(value) => promise_from_value(value, context),
                    Err(error) => rejected_promise(error.into_opaque(context)?, context),
                }
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum CancelAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl CancelAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    fn call(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), context),
            Self::JavaScript(callback) => match callback.call(&[reason], context) {
                Ok(value) => promise_from_value(value, context),
                Err(error) => rejected_promise(error.into_opaque(context)?, context),
            },
        }
    }
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller-from-underlying-source>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum StartAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl StartAlgorithm {
    /// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller>
    fn call(
        &self,
        controller: &ReadableStreamDefaultController,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        match self {
            Self::ReturnUndefined => Ok(JsValue::undefined()),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller.object()?);
                callback.call(&[arg], context)
            }
        }
    }
}

/// Note: Stores a queued chunk together with the queue size contribution that
/// `EnqueueValueWithSize` computes for it.
#[derive(Clone, Trace, Finalize)]
struct QueueEntry {
    /// Note: Stores the queued chunk value.
    chunk: JsValue,

    /// Note: Stores the queue-size contribution that the chunk added.
    #[unsafe_ignore_trace]
    size: f64,
}

/// Note: Groups the spec-defined internal slots carried by
/// `ReadableStreamDefaultController`.
#[derive(Trace, Finalize)]
struct ReadableStreamDefaultControllerSlots {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-stream>
    stream: Option<ReadableStream>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-queue>
    queue: Vec<QueueEntry>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-queuetotalsize>
    #[unsafe_ignore_trace]
    queue_total_size: f64,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-started>
    #[unsafe_ignore_trace]
    started: bool,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-closerequested>
    #[unsafe_ignore_trace]
    close_requested: bool,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullagain>
    #[unsafe_ignore_trace]
    pull_again: bool,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pulling>
    #[unsafe_ignore_trace]
    pulling: bool,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-strategysizealgorithm>
    strategy_size_algorithm: Option<SizeAlgorithm>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: f64,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    pull_algorithm: Option<PullAlgorithm>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    cancel_algorithm: Option<CancelAlgorithm>,
}

/// <https://streams.spec.whatwg.org/#rs-default-controller-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStreamDefaultController {
    /// Note: Stores the JavaScript wrapper object that carries the controller's Web IDL brand.
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// Note: Shares the controller's spec-defined internal slots across Rust clones.
    slots: Gc<GcRefCell<ReadableStreamDefaultControllerSlots>>,
}

impl ReadableStreamDefaultController {
    /// Note: Allocates a controller carrier with empty spec-defined internal slots.
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            slots: Gc::new(GcRefCell::new(ReadableStreamDefaultControllerSlots {
                stream: None,
                queue: Vec::new(),
                queue_total_size: 0.0,
                started: false,
                close_requested: false,
                pull_again: false,
                pulling: false,
                strategy_size_algorithm: None,
                strategy_high_water_mark: 0.0,
                pull_algorithm: None,
                cancel_algorithm: None,
            })),
        }
    }

    /// Note: Records the controller's JavaScript wrapper once Boa allocates it.
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    /// Note: Returns the JavaScript wrapper object for the controller carrier.
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its JavaScript object")
                .into()
        })
    }

    /// Note: Reads `ReadableStreamDefaultController.[[stream]]`.
    fn stream_slot(&self) -> JsResult<ReadableStream> {
        self.slots.borrow().stream.clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its stream")
                .into()
        })
    }

    /// Note: Reports whether the controller's queue currently has any entries.
    fn queue_is_empty(&self) -> bool {
        self.slots.borrow().queue.is_empty()
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-desired-size>
    pub(crate) fn desired_size(&self) -> JsResult<Option<f64>> {
        // Step 1: "Return ! ReadableStreamDefaultControllerGetDesiredSize(this)."
        readable_stream_default_controller_get_desired_size(self.clone())
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-close>
    pub(crate) fn close(&mut self, context: &mut Context) -> JsResult<()> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !readable_stream_default_controller_can_close_or_enqueue(self.clone())? {
            return Err(JsNativeError::typ()
                .with_message("The stream is not in a state that permits close")
                .into());
        }

        // Step 2: "Perform ! ReadableStreamDefaultControllerClose(this)."
        readable_stream_default_controller_close(self.clone(), context)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-enqueue>
    pub(crate) fn enqueue(&mut self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !readable_stream_default_controller_can_close_or_enqueue(self.clone())? {
            return Err(JsNativeError::typ()
                .with_message("The stream is not in a state that permits enqueue")
                .into());
        }

        // Step 2: "Perform ? ReadableStreamDefaultControllerEnqueue(this, chunk)."
        readable_stream_default_controller_enqueue(self.clone(), chunk, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-error>
    pub(crate) fn error(&mut self, error: JsValue, context: &mut Context) -> JsResult<()> {
        // Step 1: "Perform ! ReadableStreamDefaultControllerError(this, e)."
        readable_stream_default_controller_error(self.clone(), error, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-private-cancel>
    pub(crate) fn cancel_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        // Step 1: "Perform ! ResetQueue(this)."
        reset_controller_queue(self);

        let cancel_algorithm = self.slots.borrow().cancel_algorithm.clone();

        // Step 2: "Let result be the result of performing this.[[cancelAlgorithm]], passing reason."
        let result = match cancel_algorithm {
            Some(cancel_algorithm) => cancel_algorithm.call(reason, context)?,
            None => resolved_promise(JsValue::undefined(), context)?,
        };

        // Step 3: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(this)."
        readable_stream_default_controller_clear_algorithms(self);

        // Step 4: "Return result."
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-private-pull>
    pub(crate) fn pull_steps(&self, read_request: ReadRequest, context: &mut Context) -> JsResult<()> {
        // Step 1: "Let stream be this.[[stream]]."
        let stream = self.stream_slot()?;

        // Step 2: "If this.[[queue]] is not empty,"
        if !self.queue_is_empty() {
            let (chunk, should_close_stream) = {
                let mut slots = self.slots.borrow_mut();

                // Step 2.1: "Let chunk be ! DequeueValue(this)."
                let entry = slots.queue.remove(0);
                slots.queue_total_size -= entry.size;
                if slots.queue_total_size == -0.0 {
                    slots.queue_total_size = 0.0;
                }

                // Step 2.2: "If this.[[closeRequested]] is true and this.[[queue]] is empty,"
                let should_close_stream = slots.close_requested && slots.queue.is_empty();
                (entry.chunk.clone(), should_close_stream)
            };

            if should_close_stream {
                // Step 2.2.1: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(this)."
                readable_stream_default_controller_clear_algorithms(self);

                // Step 2.2.2: "Perform ! ReadableStreamClose(stream)."
                readable_stream_close(stream, context)?;
            } else {
                // Step 2.3: "Otherwise, perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
                readable_stream_default_controller_call_pull_if_needed(self.clone(), context)?;
            }

            // Step 2.4: "Perform readRequest's chunk steps, given chunk."
            return read_request.chunk_steps(chunk, context);
        }

        // Step 3.1: "Perform ! ReadableStreamAddReadRequest(stream, readRequest)."
        readable_stream_add_read_request(stream.clone(), read_request)?;

        // Step 3.2: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
        readable_stream_default_controller_call_pull_if_needed(self.clone(), context)
    }

    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaultcontroller-releasesteps>
    pub(crate) fn release_steps(&self) -> JsResult<()> {
        // Step 1: "Return."
        Ok(())
    }
}

/// Note: Allocates a controller carrier and its JavaScript wrapper together.
pub(crate) fn create_readable_stream_default_controller(
    context: &mut Context,
) -> JsResult<ReadableStreamDefaultController> {
    let controller = ReadableStreamDefaultController::new(None);
    let controller_object = ReadableStreamDefaultController::from_data(controller.clone(), context)?;
    controller.set_reflector(controller_object);
    Ok(controller)
}

/// Note: Borrows a default controller carrier from a JavaScript object without mutating it.
pub(crate) fn with_readable_stream_default_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStreamDefaultController) -> R,
) -> JsResult<R> {
    let controller = object.downcast_ref::<ReadableStreamDefaultController>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableStreamDefaultController")
    })?;
    Ok(f(&controller))
}

/// Note: Borrows a default controller carrier mutably from a JavaScript object.
pub(crate) fn with_readable_stream_default_controller_mut<R>(
    object: &JsObject,
    f: impl FnOnce(&mut ReadableStreamDefaultController) -> R,
) -> JsResult<R> {
    let Some(mut controller) = object.downcast_mut::<ReadableStreamDefaultController>() else {
        return Err(JsNativeError::typ()
            .with_message("object is not a ReadableStreamDefaultController")
            .into());
    };
    Ok(f(&mut controller))
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-call-pull-if-needed>
pub(crate) fn readable_stream_default_controller_call_pull_if_needed(
    controller: ReadableStreamDefaultController,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let shouldPull be ! ReadableStreamDefaultControllerShouldCallPull(controller)."
    let should_pull = readable_stream_default_controller_should_call_pull(controller.clone())?;

    // Step 2: "If shouldPull is false, return."
    if !should_pull {
        return Ok(());
    }

    // Step 3: "If controller.[[pulling]] is true,"
    if controller.slots.borrow().pulling {
        // Step 3.1: "Set controller.[[pullAgain]] to true."
        controller.slots.borrow_mut().pull_again = true;

        // Step 3.2: "Return."
        return Ok(());
    }

    // Step 4: "Assert: controller.[[pullAgain]] is false."
    debug_assert!(!controller.slots.borrow().pull_again);

    // Step 5: "Set controller.[[pulling]] to true."
    controller.slots.borrow_mut().pulling = true;

    // Step 6: "Let pullPromise be the result of performing controller.[[pullAlgorithm]]."
    let pull_algorithm = controller.slots.borrow().pull_algorithm.clone();
    let pull_promise = match pull_algorithm {
        Some(pull_algorithm) => pull_algorithm.call(&controller, context)?,
        None => resolved_promise(JsValue::undefined(), context)?,
    };

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, controller: &ReadableStreamDefaultController, context| {
            // Step 7.1: "Set controller.[[pulling]] to false."
            controller.slots.borrow_mut().pulling = false;

            let should_pull_again = controller.slots.borrow().pull_again;
            if should_pull_again {
                // Step 7.2.1: "Set controller.[[pullAgain]] to false."
                controller.slots.borrow_mut().pull_again = false;

                // Step 7.2.2: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
                readable_stream_default_controller_call_pull_if_needed(controller.clone(), context)?;
            }

            Ok(JsValue::undefined())
        },
        controller.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, controller: &ReadableStreamDefaultController, context| {
            // Step 8.1: "Perform ! ReadableStreamDefaultControllerError(controller, e)."
            readable_stream_default_controller_error(
                controller.clone(),
                args.get_or_undefined(0).clone(),
                context,
            )?;
            Ok(JsValue::undefined())
        },
        controller,
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(pull_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-error>
pub(crate) fn readable_stream_default_controller_error(
    controller: ReadableStreamDefaultController,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 2: "If stream.[[state]] is not \"readable\", return."
    if stream.state() != ReadableStreamState::Readable {
        return Ok(());
    }

    // Step 3: "Perform ! ResetQueue(controller)."
    reset_controller_queue(&controller);

    // Step 4: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
    readable_stream_default_controller_clear_algorithms(&controller);

    // Step 5: "Perform ! ReadableStreamError(stream, e)."
    readable_stream_error(stream, error, context)
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller>
pub(crate) fn set_up_readable_stream_default_controller(
    stream: ReadableStream,
    controller: ReadableStreamDefaultController,
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: stream.[[controller]] is undefined."
    debug_assert!(stream.controller_slot().is_none());

    {
        let mut slots = controller.slots.borrow_mut();

        // Step 2: "Set controller.[[stream]] to stream."
        slots.stream = Some(stream.clone());

        // Step 3: "Perform ! ResetQueue(controller)."
        slots.queue.clear();
        slots.queue_total_size = 0.0;

        // Step 4: "Set controller.[[started]], controller.[[closeRequested]], controller.[[pullAgain]], and controller.[[pulling]] to false."
        slots.started = false;
        slots.close_requested = false;
        slots.pull_again = false;
        slots.pulling = false;

        // Step 5: "Set controller.[[strategySizeAlgorithm]] to sizeAlgorithm and controller.[[strategyHWM]] to highWaterMark."
        slots.strategy_size_algorithm = Some(size_algorithm);
        slots.strategy_high_water_mark = high_water_mark;

        // Step 6: "Set controller.[[pullAlgorithm]] to pullAlgorithm."
        slots.pull_algorithm = Some(pull_algorithm);

        // Step 7: "Set controller.[[cancelAlgorithm]] to cancelAlgorithm."
        slots.cancel_algorithm = Some(cancel_algorithm);
    }

    // Step 8: "Set stream.[[controller]] to controller."
    stream.set_controller_slot(Some(ReadableStreamController::Default(controller.clone())));

    // Step 9: "Let startResult be the result of performing startAlgorithm. (This might throw an exception.)"
    let start_result = start_algorithm.call(&controller, context)?;

    // Step 10: "Let startPromise be a promise resolved with startResult."
    let start_promise = JsPromise::resolve(start_result, context)?;
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, controller: &ReadableStreamDefaultController, context| {
            {
                let mut slots = controller.slots.borrow_mut();

                // Step 11.1: "Set controller.[[started]] to true."
                slots.started = true;

                // Step 11.2: "Assert: controller.[[pulling]] is false."
                debug_assert!(!slots.pulling);

                // Step 11.3: "Assert: controller.[[pullAgain]] is false."
                debug_assert!(!slots.pull_again);
            }

            // Step 11.4: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
            readable_stream_default_controller_call_pull_if_needed(controller.clone(), context)?;
            Ok(JsValue::undefined())
        },
        controller.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, controller: &ReadableStreamDefaultController, context| {
            // Step 12.1: "Perform ! ReadableStreamDefaultControllerError(controller, r)."
            readable_stream_default_controller_error(
                controller.clone(),
                args.get_or_undefined(0).clone(),
                context,
            )?;
            Ok(JsValue::undefined())
        },
        controller,
    )
    .to_js_function(context.realm());
    let _ = start_promise.then(Some(on_fulfilled), Some(on_rejected), context)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller-from-underlying-source>
pub(crate) fn set_up_readable_stream_default_controller_from_underlying_source(
    stream: ReadableStream,
    underlying_source_object: Option<JsObject>,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let controller be a new ReadableStreamDefaultController."
    let controller = create_readable_stream_default_controller(context)?;

    // Step 2: "Let startAlgorithm be an algorithm that returns undefined."
    let mut start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 3: "Let pullAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut pull_algorithm = PullAlgorithm::ReturnUndefined;

    // Step 4: "Let cancelAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut cancel_algorithm = CancelAlgorithm::ReturnUndefined;

    // Step 5: "If underlyingSourceDict[\"start\"] exists, then set startAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"start\"] with argument list « controller » and callback this value underlyingSource."
    if let Some(start_method) = extract_source_method(underlying_source_object.as_ref(), "start", context)? {
        start_algorithm = StartAlgorithm::JavaScript(start_method);
    }

    // Step 6: "If underlyingSourceDict[\"pull\"] exists, then set pullAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"pull\"] with argument list « controller » and callback this value underlyingSource."
    if let Some(pull_method) = extract_source_method(underlying_source_object.as_ref(), "pull", context)? {
        pull_algorithm = PullAlgorithm::JavaScript(pull_method);
    }

    // Step 7: "If underlyingSourceDict[\"cancel\"] exists, then set cancelAlgorithm to an algorithm which takes an argument reason and returns the result of invoking underlyingSourceDict[\"cancel\"] with argument list « reason » and callback this value underlyingSource."
    if let Some(cancel_method) = extract_source_method(underlying_source_object.as_ref(), "cancel", context)? {
        cancel_algorithm = CancelAlgorithm::JavaScript(cancel_method);
    }

    // Step 8: "Perform ? SetUpReadableStreamDefaultController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller(
        stream,
        controller,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        size_algorithm,
        context,
    )
}

/// Note: Extracts a callable underlying-source dictionary member and records the original
/// underlying source object as the callback this value required by the Streams setup algorithm.
fn extract_source_method(
    source_object: Option<&JsObject>,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<SourceMethod>> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let property_name = JsString::from(name);
    if !source_object.has_property(property_name.clone(), context)? {
        return Ok(None);
    }

    let value = source_object.get(property_name, context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let callback = value.as_object().filter(|object| object.is_callable()).ok_or_else(|| {
        JsNativeError::typ().with_message(format!("underlying source {name} must be callable"))
    })?;

    Ok(Some(SourceMethod {
        this_value: source_object.clone(),
        callback,
    }))
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-should-call-pull>
fn readable_stream_default_controller_should_call_pull(
    controller: ReadableStreamDefaultController,
) -> JsResult<bool> {
    // Step 1: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 2: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return false."
    if !readable_stream_default_controller_can_close_or_enqueue(controller.clone())? {
        return Ok(false);
    }

    // Step 3: "If controller.[[started]] is false, return false."
    if !controller.slots.borrow().started {
        return Ok(false);
    }

    // Step 4: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, return true."
    if stream.is_readable_stream_locked() && readable_stream_get_num_read_requests(stream.clone()) > 0 {
        return Ok(true);
    }

    // Step 5: "Let desiredSize be ! ReadableStreamDefaultControllerGetDesiredSize(controller)."
    let desired_size = readable_stream_default_controller_get_desired_size(controller)?;

    // Step 6: "Assert: desiredSize is not null."
    debug_assert!(desired_size.is_some());

    // Step 7: "If desiredSize > 0, return true."
    if desired_size.unwrap_or(0.0) > 0.0 {
        return Ok(true);
    }

    // Step 8: "Return false."
    Ok(false)
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-clear-algorithms>
fn readable_stream_default_controller_clear_algorithms(
    controller: &ReadableStreamDefaultController,
) {
    let mut slots = controller.slots.borrow_mut();

    // Step 1: "Set controller.[[pullAlgorithm]] to undefined."
    slots.pull_algorithm = None;

    // Step 2: "Set controller.[[cancelAlgorithm]] to undefined."
    slots.cancel_algorithm = None;

    // Step 3: "Set controller.[[strategySizeAlgorithm]] to undefined."
    slots.strategy_size_algorithm = None;
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-close>
fn readable_stream_default_controller_close(
    controller: ReadableStreamDefaultController,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
    if !readable_stream_default_controller_can_close_or_enqueue(controller.clone())? {
        return Ok(());
    }

    // Step 2: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 3: "Set controller.[[closeRequested]] to true."
    controller.slots.borrow_mut().close_requested = true;

    // Step 4: "If controller.[[queue]] is empty,"
    if controller.queue_is_empty() {
        // Step 4.1: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
        readable_stream_default_controller_clear_algorithms(&controller);

        // Step 4.2: "Perform ! ReadableStreamClose(stream)."
        readable_stream_close(stream, context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-enqueue>
fn readable_stream_default_controller_enqueue(
    controller: ReadableStreamDefaultController,
    chunk: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
    if !readable_stream_default_controller_can_close_or_enqueue(controller.clone())? {
        return Ok(());
    }

    // Step 2: "Let stream be controller.[[stream]]."
    let stream = controller.stream_slot()?;

    // Step 3: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, perform ! ReadableStreamFulfillReadRequest(stream, chunk, false)."
    if stream.is_readable_stream_locked() && readable_stream_get_num_read_requests(stream.clone()) > 0 {
        readable_stream_fulfill_read_request(stream, chunk, false, context)?;
    } else {
        // Step 4.1: "Let result be the result of performing controller.[[strategySizeAlgorithm]], passing in chunk, and interpreting the result as a completion record."
        let size_algorithm = controller.slots.borrow().strategy_size_algorithm.clone().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamDefaultController is missing its size algorithm")
        })?;
        let chunk_size = match size_algorithm.size(&chunk, context) {
            Ok(chunk_size) => chunk_size,
            Err(error) => {
                let opaque = error.into_opaque(context)?;

                // Step 4.2.1: "Perform ! ReadableStreamDefaultControllerError(controller, result.[[Value]])."
                readable_stream_default_controller_error(controller.clone(), opaque.clone(), context)?;

                // Step 4.2.2: "Return result."
                return Err(JsError::from_opaque(opaque));
            }
        };

        // Step 4.3: "Let chunkSize be result.[[Value]]."

        // Step 4.4: "Let enqueueResult be EnqueueValueWithSize(controller, chunk, chunkSize)."
        if !chunk_size.is_finite() || chunk_size < 0.0 {
            let error = range_error_value(
                "queue strategy size must be a finite, non-negative number",
                context,
            )?;

            // Step 4.5.1: "Perform ! ReadableStreamDefaultControllerError(controller, enqueueResult.[[Value]])."
            readable_stream_default_controller_error(controller.clone(), error.clone(), context)?;

            // Step 4.5.2: "Return enqueueResult."
            return Err(JsError::from_opaque(error));
        }

        enqueue_value_with_size(&controller, chunk, chunk_size);
    }

    // Step 5: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
    readable_stream_default_controller_call_pull_if_needed(controller, context)
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-get-desired-size>
fn readable_stream_default_controller_get_desired_size(
    controller: ReadableStreamDefaultController,
) -> JsResult<Option<f64>> {
    // Step 1: "Let state be controller.[[stream]].[[state]]."
    let state = controller.stream_slot()?.state();

    // Step 2: "If state is \"errored\", return null."
    if state == ReadableStreamState::Errored {
        return Ok(None);
    }

    // Step 3: "If state is \"closed\", return 0."
    if state == ReadableStreamState::Closed {
        return Ok(Some(0.0));
    }

    // Step 4: "Return controller.[[strategyHWM]] - controller.[[queueTotalSize]]."
    let slots = controller.slots.borrow();
    Ok(Some(slots.strategy_high_water_mark - slots.queue_total_size))
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-controller-can-close-or-enqueue>
fn readable_stream_default_controller_can_close_or_enqueue(
    controller: ReadableStreamDefaultController,
) -> JsResult<bool> {
    // Step 1: "Let state be controller.[[stream]].[[state]]."
    let state = controller.stream_slot()?.state();

    // Step 2: "If controller.[[closeRequested]] is false and state is \"readable\", return true."
    if !controller.slots.borrow().close_requested && state == ReadableStreamState::Readable {
        return Ok(true);
    }

    // Step 3: "Otherwise, return false."
    Ok(false)
}

/// <https://streams.spec.whatwg.org/#enqueue-value-with-size>
fn enqueue_value_with_size(
    controller: &ReadableStreamDefaultController,
    chunk: JsValue,
    chunk_size: f64,
) {
    let mut slots = controller.slots.borrow_mut();
    slots.queue.push(QueueEntry {
        chunk,
        size: chunk_size,
    });
    slots.queue_total_size += chunk_size;
}

/// <https://streams.spec.whatwg.org/#reset-queue>
fn reset_controller_queue(controller: &ReadableStreamDefaultController) {
    let mut slots = controller.slots.borrow_mut();
    slots.queue.clear();
    slots.queue_total_size = 0.0;
}