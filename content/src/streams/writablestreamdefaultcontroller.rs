use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use std::{cell::Cell, rc::Rc};

use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Trace};

use js_engine::{Completion, ExecutionContext, JsEngine, JsTypes};

use crate::{
    dom::{AbortSignal, create_abort_signal, signal_abort},
    js::bindings::dom::EcDispatchHost,
    streams::SizeAlgorithm,
    webidl::bindings::create_interface_instance,
    webidl::{promise_from_value, rejected_promise, resolved_promise},
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
        controller_object: &JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let result: JsResult<JsValue> = match self {
            Self::ReturnUndefined => Ok(JsValue::undefined()),
            Self::ReturnValue(value) => Ok(value.clone()),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller_object.clone());
                crate::js::completion_to_js_result(callback.call(&[arg], ec))
            }
        };
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        // js_result_to_completion wraps JsError -> JsValue via Boa's Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        crate::js::js_result_to_completion(result, context)
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
        controller_object: &JsObject,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), ec),
            Self::JavaScript(callback) => {
                let controller_value = JsValue::from(controller_object.clone());
                let call_result = callback.call(&[chunk, controller_value], ec);
                match call_result {
                    Ok(value) => promise_from_value(value, ec),
                    Err(error_value) => rejected_promise(error_value, ec),
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
    fn call(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), ec),
            Self::JavaScript(callback) => {
                let call_result = callback.call(&[], ec);
                match call_result {
                    Ok(value) => promise_from_value(value, ec),
                    Err(error_value) => rejected_promise(error_value, ec),
                }
            }
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
    fn call(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(JsValue::undefined(), ec),
            Self::JavaScript(callback) => {
                let call_result = callback.call(&[reason], ec);
                match call_result {
                    Ok(value) => promise_from_value(value, ec),
                    Err(error_value) => rejected_promise(error_value, ec),
                }
            }
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
#[gc_struct]
pub struct WritableStreamDefaultController {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-stream>
    stream: GcCell<Option<WritableStream>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortcontroller>
    abort_signal: GcCell<Option<AbortSignal>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-queue>
    queue: GcCell<Vec<QueueEntry>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-queuetotalsize>
    #[unsafe_ignore_trace]
    queue_total_size: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-started>
    #[unsafe_ignore_trace]
    started: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategysizealgorithm>
    strategy_size_algorithm: GcCell<Option<SizeAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-writealgorithm>
    write_algorithm: GcCell<Option<WriteAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
    close_algorithm: GcCell<Option<CloseAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortalgorithm>
    abort_algorithm: GcCell<Option<AbortAlgorithm>>,
}

impl WritableStreamDefaultController {
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            abort_signal: gc_cell_new(None),
            queue: gc_cell_new(Vec::new()),
            queue_total_size: Rc::new(Cell::new(0.0)),
            started: Rc::new(Cell::new(false)),
            strategy_size_algorithm: gc_cell_new(None),
            strategy_high_water_mark: Rc::new(Cell::new(1.0)),
            write_algorithm: gc_cell_new(None),
            close_algorithm: gc_cell_new(None),
            abort_algorithm: gc_cell_new(None),
        }
    }
    fn stream_slot(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<WritableStream, crate::js::Types> {
        self.stream.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is not attached to a stream")
        })
    }

    fn stream_slot_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<WritableStream, crate::js::Types> {
        self.stream.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is not attached to a stream")
        })
    }

    fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.stream_slot(ec)?
            .controller_object_slot()
            .ok_or_else(|| {
                ec.new_type_error(
                    "WritableStreamDefaultController is missing its JavaScript object",
                )
            })
    }

    fn controller_object_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.stream_slot_ec(ec)?
            .controller_object_slot()
            .ok_or_else(|| {
                ec.new_type_error(
                    "WritableStreamDefaultController is missing its JavaScript object",
                )
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

    pub(crate) fn signal_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<AbortSignal, crate::js::Types> {
        self.abort_signal.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its abort signal")
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
        self.signal()?.object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("AbortSignal is missing its JavaScript object")
                .into()
        })
    }

    pub(crate) fn signal_value_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        self.signal_ec(ec)?
            .object()
            .ok_or_else(|| ec.new_type_error("AbortSignal is missing its JavaScript object"))
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-error>
    pub(crate) fn error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let state = self.stream_slot_ec(ec)?.state();
        if state != WritableStreamState::Writable {
            return Ok(());
        }
        self.error_controller(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-private-abort>
    pub(crate) fn abort_steps(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        let algorithm = self.abort_algorithm(ec)?;
        let result = algorithm.call(reason, ec)?;
        self.clear_algorithms();
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-private-error>
    pub(crate) fn error_steps(&self) {
        self.reset_queue();
    }

    pub(crate) fn signal_abort(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let signal = self.signal_ec(ec)?;
        let mut host = EcDispatchHost::new(ec);
        signal_abort(&mut host, &signal, reason)
    }

    fn write_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<WriteAlgorithm, crate::js::Types> {
        self.write_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its write algorithm")
        })
    }

    fn close_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<CloseAlgorithm, crate::js::Types> {
        self.close_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its close algorithm")
        })
    }

    fn abort_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<AbortAlgorithm, crate::js::Types> {
        self.abort_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its abort algorithm")
        })
    }

    fn get_desired_size(
        &self,
        _ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<f64, crate::js::Types> {
        Ok(self.strategy_high_water_mark.get() - self.queue_total_size.get())
    }

    fn get_backpressure(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        Ok(self.get_desired_size(ec)? <= 0.0)
    }

    fn get_chunk_size(
        &self,
        chunk: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<f64, crate::js::Types> {
        let Some(strategy_size_algorithm) = self.strategy_size_algorithm.borrow().clone() else {
            debug_assert_ne!(
                self.stream_slot(ec)?.state(),
                WritableStreamState::Writable,
            );
            return Ok(1.0);
        };

        match strategy_size_algorithm.size(chunk, ec) {
            Ok(size) => Ok(size),
            Err(error) => {
                self.error_if_needed(error, ec)?;
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

    fn error_controller(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot_ec(ec)?;
        debug_assert_eq!(stream.state(), WritableStreamState::Writable);

        self.clear_algorithms();
        stream.start_erroring(error, ec)
    }

    fn error_if_needed(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.stream_slot_ec(ec)?.state() == WritableStreamState::Writable {
            self.error_controller(error, ec)?;
        }
        Ok(())
    }

    fn close_controller(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        crate::js::js_result_to_completion(
            self.enqueue_value_with_size(QueueEntryValue::CloseSentinel, 0.0),
            context,
        )?;
        self.advance_queue_if_needed(ec)
    }

    fn write_controller(
        &self,
        chunk: JsValue,
        chunk_size: f64,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let backpressure = self.get_backpressure(ec)?;
        let stream = self.stream_slot(ec)?;
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        if let Err(error) = self.enqueue_value_with_size(QueueEntryValue::Chunk(chunk), chunk_size)
        {
            let opaque = crate::js::js_result_to_completion(error.into_opaque(context), context)?;
            self.error_if_needed(opaque, ec)?;
            return Ok(());
        }

        if !stream.close_queued_or_in_flight() && stream.state() == WritableStreamState::Writable {
            stream.update_backpressure(backpressure, ec)?;
        }

        self.advance_queue_if_needed(ec)
    }

    fn advance_queue_if_needed(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
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
            stream.finish_erroring(ec)?;
            return Ok(());
        }

        if self.queue.borrow().is_empty() {
            return Ok(());
        }

        match self.peek_queue_value(ec)? {
            QueueEntryValue::CloseSentinel => self.process_close(ec),
            QueueEntryValue::Chunk(ref chunk) => self.process_write(chunk.clone(), ec),
        }
    }

    fn process_close(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let stream = self.stream_slot(ec)?;
        let _ = self.dequeue_value(ec)?;
        debug_assert!(self.queue.borrow().is_empty());
        let algorithm = self.close_algorithm(ec)?;
        let sink_close_promise = algorithm.call(ec)?;
        // Note: ec_to_ctx — mark_close_request_in_flight and builtin_with_captures need Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        crate::js::js_result_to_completion(stream.mark_close_request_in_flight(), context)?;
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = crate::js::builtin_with_captures(
            context,
            stream_for_fulfilled,
            process_close_on_fulfilled,
            1,
        );
        let on_rejected =
            crate::js::builtin_with_captures(context, stream, process_close_on_rejected, 1);
        let promise = crate::js::js_result_to_completion(
            JsPromise::from_object(sink_close_promise),
            context,
        )?;
        let _ = crate::js::js_result_to_completion(
            promise.then(Some(on_fulfilled), Some(on_rejected), context),
            context,
        )?;
        Ok(())
    }

    fn process_write(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot(ec)?;

        // Step 2: "Perform ! WritableStreamMarkFirstWriteRequestInFlight(stream)."
        // Step 3: "Let sinkWritePromise be the result of performing controller.[[writeAlgorithm]], passing in chunk."
        let controller_object = self.controller_object(ec)?;
        let write_algo = self.write_algorithm(ec)?;
        let sink_write_promise = write_algo.call(&controller_object, chunk, ec)?;
        // Note: ec_to_ctx — mark_first_write_request_in_flight still takes Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        crate::js::js_result_to_completion(stream.mark_first_write_request_in_flight(), context)?;

        let controller_for_fulfilled = self.clone();
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = crate::js::builtin_with_captures(
            context,
            (controller_for_fulfilled, stream_for_fulfilled),
            process_write_on_fulfilled,
            1,
        );
        let on_rejected = crate::js::builtin_with_captures(
            context,
            (self.clone(), stream),
            process_write_on_rejected,
            1,
        );
        let promise = crate::js::js_result_to_completion(
            JsPromise::from_object(sink_write_promise),
            context,
        )?;
        let _ = crate::js::js_result_to_completion(
            promise.then(Some(on_fulfilled), Some(on_rejected), context),
            context,
        )?;
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
        self.queue_total_size
            .set(self.queue_total_size.get() + chunk_size);
        Ok(())
    }

    fn reset_queue(&self) {
        self.queue.borrow_mut().clear();
        self.queue_total_size.set(0.0);
    }

    fn peek_queue_value(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<QueueEntryValue, crate::js::Types> {
        self.queue
            .borrow()
            .first()
            .map(|entry| entry.value.clone())
            .ok_or_else(|| ec.new_type_error("WritableStreamDefaultController queue is empty"))
    }

    fn dequeue_value(
        &self,
        _ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<QueueEntryValue, crate::js::Types> {
        let mut queue = self.queue.borrow_mut();
        let entry = queue.remove(0);
        drop(queue);
        let value = entry.value.clone();

        let mut queue_total_size = self.queue_total_size.get() - entry.size;
        if queue_total_size <= 0.0 {
            queue_total_size = 0.0;
        }
        self.queue_total_size.set(queue_total_size);

        Ok(value)
    }
}

pub(crate) fn create_writable_stream_default_controller(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(WritableStreamDefaultController, JsObject), crate::js::Types> {
    let controller = WritableStreamDefaultController::new();
    let controller_object = create_interface_instance::<
        crate::js::Types,
        WritableStreamDefaultController,
    >(controller.clone(), ec)?
    .into();
    Ok((controller, controller_object))
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

pub(crate) fn with_writable_stream_default_controller_ref_ec<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&WritableStreamDefaultController) -> R,
) -> Completion<R, crate::js::Types> {
    let ctrl_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<WritableStreamDefaultController>());
    let controller = match ctrl_ref {
        Some(c) => c,
        None => return Err(ec.new_type_error("object is not a WritableStreamDefaultController")),
    };
    Ok(f(controller))
}

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller>
pub(crate) fn set_up_writable_stream_default_controller(
    stream: WritableStream,
    controller: WritableStreamDefaultController,
    controller_object: &JsObject,
    start_algorithm: StartAlgorithm,
    write_algorithm: WriteAlgorithm,
    close_algorithm: CloseAlgorithm,
    abort_algorithm: AbortAlgorithm,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream implements WritableStream."
    // Step 2: "Assert: stream.[[controller]] is undefined."

    // Step 3: "Set controller.[[stream]] to stream."
    controller.set_stream_slot(Some(stream.clone()));

    // Step 4: "Set stream.[[controller]] to controller."
    stream.set_controller_slot(Some(WritableStreamController::Default(controller.clone())));
    stream.set_controller_object_slot(Some(controller_object.clone()));

    // Step 5: "Perform ! ResetQueue(controller)."
    reset_controller_queue(&controller);

    // Step 6: "Set controller.[[abortController]] to a new AbortController."
    // The content process stores the exposed [AbortSignal](https://dom.spec.whatwg.org/#interface-AbortSignal) [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) directly because the controller getter
    // only needs the signal object.
    let signal = create_abort_signal(AbortSignal::new(), ec)?;
    controller.set_abort_signal_slot(signal);

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
    let backpressure = controller.get_backpressure(ec)?;

    // Step 14: "Perform ! WritableStreamUpdateBackpressure(stream, backpressure)."
    stream.update_backpressure(backpressure, ec)?;

    // Step 15: "Let startResult be the result of performing startAlgorithm."
    let start_result = start_algorithm.call(controller_object, ec)?;

    // Note: ec_to_ctx — JsPromise::resolve and builtin_with_captures need Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };

    // Step 16: "Let startPromise be a promise resolved with startResult."
    let start_promise =
        crate::js::js_result_to_completion(JsPromise::resolve(start_result, context), context)?;

    // Step 17: "Upon fulfillment of startPromise..."
    let on_fulfilled =
        crate::js::builtin_with_captures(context, controller.clone(), setup_on_fulfilled, 1);

    // Step 18: "Upon rejection of startPromise with reason r..."
    let on_rejected = crate::js::builtin_with_captures(context, controller, setup_on_rejected, 1);
    let _ = crate::js::js_result_to_completion(
        start_promise.then(Some(on_fulfilled), Some(on_rejected), context),
        context,
    )?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller-from-underlying-sink>
pub(crate) fn set_up_writable_stream_default_controller_from_underlying_sink(
    stream: WritableStream,
    underlying_sink_object: Option<JsObject>,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Let controller be a new WritableStreamDefaultController."
    let (controller, controller_object) = create_writable_stream_default_controller(ec)?;

    // Step 2-9: Extract optional methods before ec_to_ctx to avoid borrow conflicts.
    let sink_methods: Option<(
        Option<JsObject>,
        Option<JsObject>,
        Option<JsObject>,
        Option<JsObject>,
    )> = if let Some(ref underlying_sink) = underlying_sink_object {
        let start = get_callable_method(underlying_sink, "start", ec)?;
        let write = get_callable_method(underlying_sink, "write", ec)?;
        let close = get_callable_method(underlying_sink, "close", ec)?;
        let abort = get_callable_method(underlying_sink, "abort", ec)?;
        Some((start, write, close, abort))
    } else {
        None
    };

    // SAFETY: ec is backed by BoaContext repr(transparent) over Context
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };

    // Step 2: "Let startAlgorithm be an algorithm that returns undefined."
    let mut start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 3: "Let writeAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut write_algorithm = WriteAlgorithm::ReturnUndefined;

    // Step 4: "Let closeAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut close_algorithm = CloseAlgorithm::ReturnUndefined;

    // Step 5: "Let abortAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut abort_algorithm = AbortAlgorithm::ReturnUndefined;

    if let (Some((start, write, close, abort)), Some(underlying_sink)) =
        (sink_methods, underlying_sink_object)
    {
        // Step 6: "If underlyingSinkDict['start'] exists, then set startAlgorithm ..."
        if let Some(start) = start {
            start_algorithm = StartAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                crate::webidl::Callback::from_object(start),
            ));
        }

        // Step 7: "If underlyingSinkDict['write'] exists, then set writeAlgorithm ..."
        if let Some(write) = write {
            write_algorithm = WriteAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                crate::webidl::Callback::from_object(write),
            ));
        }

        // Step 8: "If underlyingSinkDict['close'] exists, then set closeAlgorithm ..."
        if let Some(close) = close {
            close_algorithm = CloseAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink.clone(),
                crate::webidl::Callback::from_object(close),
            ));
        }

        // Step 9: "If underlyingSinkDict['abort'] exists, then set abortAlgorithm ..."
        if let Some(abort) = abort {
            abort_algorithm = AbortAlgorithm::JavaScript(SourceMethod::new(
                underlying_sink,
                crate::webidl::Callback::from_object(abort),
            ));
        }
    }

    // Step 10: "Perform ? SetUpWritableStreamDefaultController(...)."
    set_up_writable_stream_default_controller(
        stream,
        controller,
        &controller_object,
        start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        high_water_mark,
        size_algorithm,
        ec,
    )
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-close>
pub(crate) fn writable_stream_default_controller_close(
    controller: WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    controller.close_controller(ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-error-if-needed>
pub(crate) fn writable_stream_default_controller_error_if_needed(
    controller: WritableStreamDefaultController,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    controller.error_if_needed(error, ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-chunk-size>
pub(crate) fn writable_stream_default_controller_get_chunk_size(
    controller: WritableStreamDefaultController,
    chunk: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<f64, crate::js::Types> {
    controller.get_chunk_size(chunk, ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-desired-size>
pub(crate) fn writable_stream_default_controller_get_desired_size(
    controller: WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<f64, crate::js::Types> {
    controller.get_desired_size(ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-write>
pub(crate) fn writable_stream_default_controller_write(
    controller: WritableStreamDefaultController,
    chunk: JsValue,
    chunk_size: f64,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    controller.write_controller(chunk, chunk_size, ec)
}

fn get_callable_method(
    object: &JsObject,
    property: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<JsObject>, crate::js::Types> {
    let pk = ec.property_key_from_str(property);
    let value = ExecutionContext::get(ec, object.clone(), pk)?;
    if <crate::js::Types as JsTypes>::value_is_undefined(&value) {
        return Ok(None);
    }

    let method = <crate::js::Types as JsTypes>::value_as_object(&value).ok_or_else(|| {
        ec.new_type_error(&format!(
            "WritableStream underlyingSink.{property} must be callable when provided"
        ))
    })?;
    let method_val = <crate::js::Types as JsTypes>::value_from_object(method.clone());
    if !ec.is_callable(&method_val) {
        return Err(ec.new_type_error(&format!(
            "WritableStream underlyingSink.{property} must be callable when provided"
        )));
    }

    Ok(Some(method.clone()))
}

fn reset_controller_queue(controller: &WritableStreamDefaultController) {
    controller.reset_queue();
}

fn process_close_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &WritableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let _ = crate::js::completion_to_js_result(
        captures.finish_in_flight_close(js_engine::boa::context_as_ec(ctx)),
    )
    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
    Ok(JsValue::undefined())
}

fn process_close_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &WritableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    crate::js::completion_to_js_result(captures.finish_in_flight_close_with_error(
        args.first().cloned().unwrap_or(JsValue::undefined()),
        js_engine::boa::context_as_ec(ctx),
    ))
    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
    Ok(JsValue::undefined())
}

fn process_write_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(WritableStreamDefaultController, WritableStream),
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (controller, stream) = captures;
    // Do EC operations before ec_to_ctx to avoid borrow conflicts.
    controller.dequeue_value(ec)?;
    let state = stream.state();
    debug_assert!(state == WritableStreamState::Writable || state == WritableStreamState::Erroring);
    if !stream.close_queued_or_in_flight() && state == WritableStreamState::Writable {
        let backpressure = controller.get_backpressure(ec)?;
        stream.update_backpressure(backpressure, ec)?;
    }
    controller.advance_queue_if_needed(ec)?;
    // Note: ec_to_ctx — finish_in_flight_write still takes Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    crate::js::completion_to_js_result(
        stream.finish_in_flight_write(js_engine::boa::context_as_ec(ctx)),
    )
    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
    Ok(JsValue::undefined())
}

fn process_write_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &(WritableStreamDefaultController, WritableStream),
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let (controller, stream) = captures;
    if stream.state() == WritableStreamState::Writable {
        controller.clear_algorithms();
    }
    crate::js::completion_to_js_result(stream.finish_in_flight_write_with_error(
        args.first().cloned().unwrap_or(JsValue::undefined()),
        js_engine::boa::context_as_ec(ctx),
    ))
    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
    Ok(JsValue::undefined())
}

fn setup_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    captures.set_started(true);
    crate::js::completion_to_js_result(
        captures.advance_queue_if_needed(js_engine::boa::context_as_ec(ctx)),
    )
    .map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))?;
    Ok(JsValue::undefined())
}

fn setup_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.set_started(true);
    let stream = captures.stream_slot(ec)?;
    stream.deal_with_rejection(
        args.first().cloned().unwrap_or(JsValue::undefined()),
        ec,
    )?;
    Ok(JsValue::undefined())
}
