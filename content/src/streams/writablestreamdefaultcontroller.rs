use std::{cell::{Cell, RefCell}, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsArgs, JsData, JsError, JsNativeError, JsResult, JsString, JsValue,
    class::Class,
    native_function::NativeFunction,
    object::{JsObject, builtins::{JsFunction, JsPromise}},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::{
    boa::platform_objects::{document_object, object_for_existing_node, resolve_element_object},
    dom::{AbortSignal, Event, EventDispatchHost, create_abort_signal, signal_abort},
    streams::SizeAlgorithm,
    webidl::{EcmascriptHost, promise_from_value, rejected_promise, resolved_promise},
};

use super::{SourceMethod, WritableStream, WritableStreamController, WritableStreamState};

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller-from-underlying-sink>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum StartAlgorithm {
    ReturnUndefined,
    ReturnValue(JsValue),
    JavaScript(SourceMethod),
}

impl StartAlgorithm {
    /// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller>
    fn call(
        &self,
        controller: &WritableStreamDefaultController,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        match self {
            Self::ReturnUndefined => Ok(JsValue::undefined()),
            Self::ReturnValue(value) => Ok(value.clone()),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller.object()?);
                callback.call(&[arg], context)
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-writealgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum WriteAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl WriteAlgorithm {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-writealgorithm>
    fn call(
        &self,
        controller: &WritableStreamDefaultController,
        chunk: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), context),
            Self::JavaScript(callback) => {
                let controller_value = JsValue::from(controller.object()?);
                match callback.call(&[chunk, controller_value], context) {
                    Ok(value) => promise_from_value(value, context),
                    Err(error) => rejected_promise(error.into_opaque(context)?, context),
                }
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum CloseAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl CloseAlgorithm {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
    fn call(&self, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), context),
            Self::JavaScript(callback) => match callback.call(&[], context) {
                Ok(value) => promise_from_value(value, context),
                Err(error) => rejected_promise(error.into_opaque(context)?, context),
            },
        }
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum AbortAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl AbortAlgorithm {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortalgorithm>
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
#[derive(Clone, Trace, Finalize)]
struct QueueEntry {
    value: QueueEntryValue,

    #[unsafe_ignore_trace]
    size: f64,
}
#[derive(Clone, Trace, Finalize)]
enum QueueEntryValue {
    Chunk(JsValue),
    CloseSentinel,
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct WritableStreamDefaultController {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-stream>
    stream: Gc<GcRefCell<Option<WritableStream>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortcontroller>
    abort_signal: Gc<GcRefCell<Option<AbortSignal>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-queue>
    queue: Gc<GcRefCell<Vec<QueueEntry>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-queuetotalsize>
    #[unsafe_ignore_trace]
    queue_total_size: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-started>
    #[unsafe_ignore_trace]
    started: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategysizealgorithm>
    strategy_size_algorithm: Gc<GcRefCell<Option<SizeAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-writealgorithm>
    write_algorithm: Gc<GcRefCell<Option<WriteAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
    close_algorithm: Gc<GcRefCell<Option<CloseAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortalgorithm>
    abort_algorithm: Gc<GcRefCell<Option<AbortAlgorithm>>>,
}

impl WritableStreamDefaultController {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            stream: Gc::new(GcRefCell::new(None)),
            abort_signal: Gc::new(GcRefCell::new(None)),
            queue: Gc::new(GcRefCell::new(Vec::new())),
            queue_total_size: Rc::new(Cell::new(0.0)),
            started: Rc::new(Cell::new(false)),
            strategy_size_algorithm: Gc::new(GcRefCell::new(None)),
            strategy_high_water_mark: Rc::new(Cell::new(1.0)),
            write_algorithm: Gc::new(GcRefCell::new(None)),
            close_algorithm: Gc::new(GcRefCell::new(None)),
            abort_algorithm: Gc::new(GcRefCell::new(None)),
        }
    }
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is missing its JavaScript object")
                .into()
        })
    }
    pub(crate) fn stream_slot(&self) -> JsResult<WritableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is not attached to a stream")
                .into()
        })
    }
    pub(crate) fn set_stream_slot(&self, stream: Option<WritableStream>) {
        *self.stream.borrow_mut() = stream;
    }

    pub(crate) fn set_abort_signal_slot(&self, signal: AbortSignal) {
        *self.abort_signal.borrow_mut() = Some(signal);
    }

    pub(crate) fn signal(&self) -> JsResult<AbortSignal> {
        self.abort_signal.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is missing its abort signal")
                .into()
        })
    }

    pub(crate) fn started(&self) -> bool {
        self.started.get()
    }

    pub(crate) fn set_started(&self, started: bool) {
        self.started.set(started);
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-signal>
    pub(crate) fn signal_value(&self) -> JsResult<JsObject> {
        self.signal()?.object()
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-error>
    pub(crate) fn error(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        let state = self.stream_slot()?.state();
        if state != WritableStreamState::Writable {
            return Ok(());
        }

        self.error_controller(error, context)
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-private-abort>
    pub(crate) fn abort_steps(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        let result = self.abort_algorithm()?.call(reason, context)?;
        self.clear_algorithms();
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-private-error>
    pub(crate) fn error_steps(&self) {
        self.reset_queue();
    }

    pub(crate) fn signal_abort(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        let mut host = ContextEventDispatchHost::new(context);
        signal_abort(&mut host, &self.signal()?, reason)
    }

    fn write_algorithm(&self) -> JsResult<WriteAlgorithm> {
        self.write_algorithm.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is missing its write algorithm")
                .into()
        })
    }

    fn close_algorithm(&self) -> JsResult<CloseAlgorithm> {
        self.close_algorithm.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is missing its close algorithm")
                .into()
        })
    }

    fn abort_algorithm(&self) -> JsResult<AbortAlgorithm> {
        self.abort_algorithm.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStreamDefaultController is missing its abort algorithm")
                .into()
        })
    }

    fn get_desired_size(&self) -> JsResult<f64> {
        Ok(self.strategy_high_water_mark.get() - self.queue_total_size.get())
    }

    fn get_backpressure(&self) -> JsResult<bool> {
        Ok(self.get_desired_size()? <= 0.0)
    }

    fn get_chunk_size(&self, chunk: &JsValue, context: &mut Context) -> JsResult<f64> {
        let Some(strategy_size_algorithm) = self.strategy_size_algorithm.borrow().clone() else {
            debug_assert_ne!(self.stream_slot()?.state(), WritableStreamState::Writable);
            return Ok(1.0);
        };

        match strategy_size_algorithm.size(chunk, context) {
            Ok(size) => Ok(size),
            Err(error) => {
                self.error_if_needed(error.into_opaque(context)?, context)?;
                Ok(1.0)
            }
        }
    }

    fn clear_algorithms(&self) {
        *self.write_algorithm.borrow_mut() = None;
        *self.close_algorithm.borrow_mut() = None;
        *self.abort_algorithm.borrow_mut() = None;
        *self.strategy_size_algorithm.borrow_mut() = None;
    }

    fn error_controller(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        debug_assert_eq!(stream.state(), WritableStreamState::Writable);

        self.clear_algorithms();
        stream.start_erroring(error, context)
    }

    fn error_if_needed(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        if self.stream_slot()?.state() == WritableStreamState::Writable {
            self.error_controller(error, context)?;
        }
        Ok(())
    }

    fn close_controller(&self, context: &mut Context) -> JsResult<()> {
        self.enqueue_value_with_size(QueueEntryValue::CloseSentinel, 0.0)?;
        self.advance_queue_if_needed(context)
    }

    fn write_controller(
        &self,
        chunk: JsValue,
        chunk_size: f64,
        context: &mut Context,
    ) -> JsResult<()> {
        if let Err(error) = self.enqueue_value_with_size(QueueEntryValue::Chunk(chunk), chunk_size) {
            self.error_if_needed(error.into_opaque(context)?, context)?;
            return Ok(());
        }

        let stream = self.stream_slot()?;
        if !stream.close_queued_or_in_flight() && stream.state() == WritableStreamState::Writable {
            let backpressure = self.get_backpressure()?;
            stream.update_backpressure(backpressure, context)?;
        }

        self.advance_queue_if_needed(context)
    }

    fn advance_queue_if_needed(&self, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        if !self.started() {
            return Ok(());
        }

        if stream.in_flight_write_request_slot().is_some() {
            return Ok(());
        }

        let state = stream.state();
        debug_assert!(
            state != WritableStreamState::Closed && state != WritableStreamState::Errored,
            "WritableStreamDefaultControllerAdvanceQueueIfNeeded() cannot run on a closed or errored stream",
        );

        if state == WritableStreamState::Erroring {
            stream.finish_erroring(context)?;
            return Ok(());
        }

        if self.queue.borrow().is_empty() {
            return Ok(());
        }

        match self.peek_queue_value()? {
            QueueEntryValue::CloseSentinel => self.process_close(context),
            QueueEntryValue::Chunk(ref chunk) => self.process_write(chunk.clone(), context),
        }
    }

    fn process_close(&self, context: &mut Context) -> JsResult<()> {
        let stream = self.stream_slot()?;
        stream.mark_close_request_in_flight()?;
        self.dequeue_value()?;
        debug_assert!(self.queue.borrow().is_empty());

        let sink_close_promise = self.close_algorithm()?.call(context)?;
        self.clear_algorithms();
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, stream: &WritableStream, context| {
                stream.finish_in_flight_close(context)?;
                Ok(JsValue::undefined())
            },
            stream_for_fulfilled,
        )
        .to_js_function(context.realm());
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, stream: &WritableStream, context| {
                stream.finish_in_flight_close_with_error(
                    args.get_or_undefined(0).clone(),
                    context,
                )?;
                Ok(JsValue::undefined())
            },
            stream,
        )
        .to_js_function(context.realm());
        let _ = JsPromise::from_object(sink_close_promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)?;
        Ok(())
    }

    fn process_write(&self, chunk: JsValue, context: &mut Context) -> JsResult<()> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot()?;

        // Step 2: "Perform ! WritableStreamMarkFirstWriteRequestInFlight(stream)."
        stream.mark_first_write_request_in_flight()?;

        // Step 3: "Let sinkWritePromise be the result of performing controller.[[writeAlgorithm]], passing in chunk."
        let sink_write_promise = self.write_algorithm()?.call(self, chunk, context)?;

        let controller_for_fulfilled = self.clone();
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, captures: &(WritableStreamDefaultController, WritableStream), context| {
                let (controller, stream) = captures;

                // Step 4.1: "Perform ! WritableStreamFinishInFlightWrite(stream)."
                stream.finish_in_flight_write(context)?;

                // Step 4.2: "Let state be stream.[[state]]."
                let state = stream.state();

                // Step 4.3: "Assert: state is \"writable\" or \"erroring\"."
                debug_assert!(
                    state == WritableStreamState::Writable || state == WritableStreamState::Erroring
                );

                // Step 4.4: "Perform ! DequeueValue(controller)."
                controller.dequeue_value()?;

                // Step 4.5: "If ! WritableStreamCloseQueuedOrInFlight(stream) is false and state is \"writable\","
                if !stream.close_queued_or_in_flight() && state == WritableStreamState::Writable {
                    // Step 4.5.1: "Let backpressure be ! WritableStreamDefaultControllerGetBackpressure(controller)."
                    let backpressure = controller.get_backpressure()?;

                    // Step 4.5.2: "Perform ! WritableStreamUpdateBackpressure(stream, backpressure)."
                    stream.update_backpressure(backpressure, context)?;
                }

                // Step 4.6: "Perform ! WritableStreamDefaultControllerAdvanceQueueIfNeeded(controller)."
                controller.advance_queue_if_needed(context)?;
                Ok(JsValue::undefined())
            },
            (controller_for_fulfilled, stream_for_fulfilled),
        )
        .to_js_function(context.realm());
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, captures: &(WritableStreamDefaultController, WritableStream), context| {
                let (controller, stream) = captures;

                // Step 5.1: "If stream.[[state]] is \"writable\", perform ! WritableStreamDefaultControllerClearAlgorithms(controller)."
                if stream.state() == WritableStreamState::Writable {
                    controller.clear_algorithms();
                }

                // Step 5.2: "Perform ! WritableStreamFinishInFlightWriteWithError(stream, reason)."
                stream.finish_in_flight_write_with_error(
                    args.get_or_undefined(0).clone(),
                    context,
                )?;
                Ok(JsValue::undefined())
            },
            (self.clone(), stream),
        )
        .to_js_function(context.realm());
        let _ = JsPromise::from_object(sink_write_promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)?;
        Ok(())
    }

    fn enqueue_value_with_size(&self, value: QueueEntryValue, chunk_size: f64) -> JsResult<()> {
        if chunk_size.is_nan() || chunk_size < 0.0 {
            return Err(JsNativeError::range()
                .with_message("queue size must be a non-negative number")
                .into());
        }

        self.queue.borrow_mut().push(QueueEntry {
            value,
            size: chunk_size,
        });
        self.queue_total_size.set(self.queue_total_size.get() + chunk_size);
        Ok(())
    }

    fn reset_queue(&self) {
        self.queue.borrow_mut().clear();
        self.queue_total_size.set(0.0);
    }

    fn peek_queue_value(&self) -> JsResult<QueueEntryValue> {
        self.queue
            .borrow()
            .first()
            .map(|entry| entry.value.clone())
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("WritableStreamDefaultController queue is empty")
                    .into()
            })
    }

    fn dequeue_value(&self) -> JsResult<QueueEntryValue> {
        let mut queue = self.queue.borrow_mut();
        let entry = queue.remove(0);
        drop(queue);
        let value = entry.value.clone();

        let mut queue_total_size = self.queue_total_size.get() - entry.size;
        if queue_total_size == -0.0 {
            queue_total_size = 0.0;
        }
        self.queue_total_size.set(queue_total_size);

        Ok(value)
    }
}
struct ContextEventDispatchHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEventDispatchHost<'a> {
    fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl EcmascriptHost for ContextEventDispatchHost<'_> {
    fn context(&mut self) -> &mut Context {
        self.context
    }

    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(JsString::from(property), self.context)
    }

    fn is_callable(&self, object: &JsObject) -> bool {
        object.is_callable()
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message("callback is not callable"))
        })?;
        function.call(this_arg, args, self.context)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn report_exception(&mut self, error: JsError, _callback: &JsObject) {
        eprintln!("uncaught abort listener error: {error}");
    }
}

impl EventDispatchHost for ContextEventDispatchHost<'_> {
    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        Event::from_data(event, self.context)
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        document_object(self.context)
    }

    fn global_object(&mut self) -> JsObject {
        self.context.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        resolve_element_object(node_id, self.context)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        object_for_existing_node(document, node_id, self.context)
    }

    fn current_time_millis(&self) -> f64 {
        0.0
    }
}

pub(crate) fn create_writable_stream_default_controller(
    context: &mut Context,
) -> JsResult<WritableStreamDefaultController> {
    let controller = WritableStreamDefaultController::new(None);
    let controller_object = WritableStreamDefaultController::from_data(controller.clone(), context)?;
    controller.set_reflector(controller_object);
    Ok(controller)
}

pub(crate) fn with_writable_stream_default_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&WritableStreamDefaultController) -> R,
) -> JsResult<R> {
    let controller = object
        .downcast_ref::<WritableStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("object is not a WritableStreamDefaultController")
        })?;
    Ok(f(&controller))
}

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller>
pub(crate) fn set_up_writable_stream_default_controller(
    stream: WritableStream,
    controller: WritableStreamDefaultController,
    start_algorithm: StartAlgorithm,
    write_algorithm: WriteAlgorithm,
    close_algorithm: CloseAlgorithm,
    abort_algorithm: AbortAlgorithm,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: stream implements WritableStream."
    // Step 2: "Assert: stream.[[controller]] is undefined."

    // Step 3: "Set controller.[[stream]] to stream."
    controller.set_stream_slot(Some(stream.clone()));

    // Step 4: "Set stream.[[controller]] to controller."
    stream.set_controller_slot(Some(WritableStreamController::Default(controller.clone())));

    // Step 5: "Perform ! ResetQueue(controller)."
    reset_controller_queue(&controller);

    // Step 6: "Set controller.[[abortController]] to a new AbortController."
    // The runtime stores the exposed AbortSignal carrier directly because the controller getter
    // only needs the signal object.
    controller.set_abort_signal_slot(create_abort_signal(AbortSignal::new(), context)?);

    // Step 7: "Set controller.[[started]] to false."
    controller.set_started(false);

    // Step 8: "Set controller.[[strategySizeAlgorithm]] to sizeAlgorithm."
    *controller.strategy_size_algorithm.borrow_mut() = Some(size_algorithm);

    // Step 9: "Set controller.[[strategyHWM]] to highWaterMark."
    controller.strategy_high_water_mark.set(high_water_mark);

    // Step 10: "Set controller.[[writeAlgorithm]] to writeAlgorithm."
    *controller.write_algorithm.borrow_mut() = Some(write_algorithm);

    // Step 11: "Set controller.[[closeAlgorithm]] to closeAlgorithm."
    *controller.close_algorithm.borrow_mut() = Some(close_algorithm);

    // Step 12: "Set controller.[[abortAlgorithm]] to abortAlgorithm."
    *controller.abort_algorithm.borrow_mut() = Some(abort_algorithm);

    // Step 13: "Let backpressure be ! WritableStreamDefaultControllerGetBackpressure(controller)."
    let backpressure = controller.get_backpressure()?;

    // Step 14: "Perform ! WritableStreamUpdateBackpressure(stream, backpressure)."
    stream.update_backpressure(backpressure, context)?;

    // Step 15: "Let startResult be the result of performing startAlgorithm."
    let start_result = start_algorithm.call(&controller, context)?;

    // Step 16: "Let startPromise be a promise resolved with startResult."
    let start_promise = JsPromise::resolve(start_result, context)?;

    // Step 17: "Upon fulfillment of startPromise..."
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, controller: &WritableStreamDefaultController, context| {
            // Step 17.2: "Set controller.[[started]] to true."
            controller.set_started(true);

            // Step 17.3: "Perform ! WritableStreamDefaultControllerAdvanceQueueIfNeeded(controller)."
            controller.advance_queue_if_needed(context)?;
            Ok(JsValue::undefined())
        },
        controller.clone(),
    )
    .to_js_function(context.realm());

    // Step 18: "Upon rejection of startPromise with reason r..."
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, controller: &WritableStreamDefaultController, context| {
            // Step 18.2: "Set controller.[[started]] to true."
            controller.set_started(true);

            // Step 18.3: "Perform ! WritableStreamDealWithRejection(stream, r)."
            let stream = controller.stream_slot()?;
            stream.deal_with_rejection(args.get_or_undefined(0).clone(), context)?;
            Ok(JsValue::undefined())
        },
        controller,
    )
    .to_js_function(context.realm());
    let _ = start_promise.then(Some(on_fulfilled), Some(on_rejected), context)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller-from-underlying-sink>
pub(crate) fn set_up_writable_stream_default_controller_from_underlying_sink(
    stream: WritableStream,
    underlying_sink_object: Option<JsObject>,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let controller be a new WritableStreamDefaultController."
    let controller = create_writable_stream_default_controller(context)?;

    // Step 2: "Let startAlgorithm be an algorithm that returns undefined."
    let mut start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 3: "Let writeAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut write_algorithm = WriteAlgorithm::ReturnUndefined;

    // Step 4: "Let closeAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut close_algorithm = CloseAlgorithm::ReturnUndefined;

    // Step 5: "Let abortAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut abort_algorithm = AbortAlgorithm::ReturnUndefined;

    if let Some(underlying_sink) = underlying_sink_object {
        // Step 6: "If underlyingSinkDict['start'] exists, then set startAlgorithm ..."
        if let Some(start) = get_callable_method(&underlying_sink, "start", context)? {
            start_algorithm = StartAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                start,
            ));
        }

        // Step 7: "If underlyingSinkDict['write'] exists, then set writeAlgorithm ..."
        if let Some(write) = get_callable_method(&underlying_sink, "write", context)? {
            write_algorithm = WriteAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                write,
            ));
        }

        // Step 8: "If underlyingSinkDict['close'] exists, then set closeAlgorithm ..."
        if let Some(close) = get_callable_method(&underlying_sink, "close", context)? {
            close_algorithm = CloseAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                close,
            ));
        }

        // Step 9: "If underlyingSinkDict['abort'] exists, then set abortAlgorithm ..."
        if let Some(abort) = get_callable_method(&underlying_sink, "abort", context)? {
            abort_algorithm = AbortAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink,
                abort,
            ));
        }
    }

    // Step 10: "Perform ? SetUpWritableStreamDefaultController(...)."
    set_up_writable_stream_default_controller(
        stream,
        controller,
        start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        high_water_mark,
        size_algorithm,
        context,
    )
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-close>
pub(crate) fn writable_stream_default_controller_close(
    controller: WritableStreamDefaultController,
    context: &mut Context,
) -> JsResult<()> {
    controller.close_controller(context)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-error-if-needed>
pub(crate) fn writable_stream_default_controller_error_if_needed(
    controller: WritableStreamDefaultController,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    controller.error_if_needed(error, context)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-chunk-size>
pub(crate) fn writable_stream_default_controller_get_chunk_size(
    controller: WritableStreamDefaultController,
    chunk: &JsValue,
    context: &mut Context,
) -> JsResult<f64> {
    controller.get_chunk_size(chunk, context)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-desired-size>
pub(crate) fn writable_stream_default_controller_get_desired_size(
    controller: WritableStreamDefaultController,
) -> JsResult<f64> {
    controller.get_desired_size()
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-write>
pub(crate) fn writable_stream_default_controller_write(
    controller: WritableStreamDefaultController,
    chunk: JsValue,
    chunk_size: f64,
    context: &mut Context,
) -> JsResult<()> {
    controller.write_controller(chunk, chunk_size, context)
}

fn get_callable_method(
    object: &JsObject,
    property: &'static str,
    context: &mut Context,
) -> JsResult<Option<JsObject>> {
    let value = object.get(JsString::from(property), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let method = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message(format!(
            "WritableStream underlyingSink.{property} must be callable when provided"
        ))
    })?;
    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message(format!(
                "WritableStream underlyingSink.{property} must be callable when provided"
            ))
            .into());
    }

    Ok(Some(method.clone()))
}

fn reset_controller_queue(controller: &WritableStreamDefaultController) {
    controller.reset_queue();
}