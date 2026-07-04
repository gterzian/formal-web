use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use std::{cell::Cell, rc::Rc};

use crate::js::Types;

use crate::{
    dom::{AbortSignal, create_abort_signal, signal_abort},
    js::bindings::dom::EcDispatchHost,
    streams::SizeAlgorithm,
    webidl::bindings::create_interface_instance,
    webidl::{promise_from_value, rejected_promise, resolved_promise},
};

use super::{SourceMethod, WritableStream, WritableStreamController, WritableStreamState};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller-from-underlying-sink>
#[gc_struct]
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        match self {
            Self::ReturnUndefined => Ok(ec.value_undefined()),
            Self::ReturnValue(value) => Ok(value.clone()),
            Self::JavaScript(callback) => {
                let arg = Types::value_from_object(controller_object.clone());
                callback.call(&[arg], ec)
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-writealgorithm>
#[gc_struct]
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(ec.value_undefined(), ec),
            Self::JavaScript(callback) => {
                let controller_value = Types::value_from_object(controller_object.clone());
                let call_result = callback.call(&[chunk, controller_value], ec);
                match call_result {
                    Ok(value) => promise_from_value(value, ec),
                    Err(error_value) => {
                        // Propagate synchronous throws directly so the caller
                        // (process_write) can invoke FinishInFlightWriteWithError
                        // and error the stream synchronously.  Converting to a
                        // rejected promise postpones the error handling to a
                        // microtask, which the pipe-to pump cannot rely on.
                        return Err(error_value);
                    }
                }
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
#[gc_struct]
pub(crate) enum CloseAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl CloseAlgorithm {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-closealgorithm>
    fn call(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(ec.value_undefined(), ec),
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
#[gc_struct]
pub(crate) enum AbortAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
}

impl AbortAlgorithm {
    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-abortalgorithm>
    fn call(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(ec.value_undefined(), ec),
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
#[gc_struct]
struct QueueEntry {
    value: QueueEntryValue,

    #[ignore_trace]
    size: f64,
}
#[gc_struct]
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
    #[ignore_trace]
    queue_total_size: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-started>
    #[ignore_trace]
    started: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategysizealgorithm>
    strategy_size_algorithm: GcCell<Option<SizeAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#writablestreamdefaultcontroller-strategyhwm>
    #[ignore_trace]
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<WritableStream, Types> {
        self.stream.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is not attached to a stream")
        })
    }

    fn controller_object(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.stream_slot(ec)?
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

    pub(crate) fn signal(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<AbortSignal, Types> {
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
    pub(crate) fn signal_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        self.signal(ec)?
            .object()
            .ok_or_else(|| ec.new_type_error("AbortSignal is missing its JavaScript object"))
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-error>
    pub(crate) fn error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let state = self.stream_slot(ec)?.state();
        if state != WritableStreamState::Writable {
            return Ok(());
        }
        self.error_controller(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#ws-default-controller-private-abort>
    pub(crate) fn abort_steps(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let signal = self.signal(ec)?;
        let mut host = EcDispatchHost::new(ec);
        signal_abort(&mut host, &signal, reason)
    }

    fn write_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<WriteAlgorithm, Types> {
        self.write_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its write algorithm")
        })
    }

    fn close_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<CloseAlgorithm, Types> {
        self.close_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its close algorithm")
        })
    }

    fn abort_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<AbortAlgorithm, Types> {
        self.abort_algorithm.borrow().clone().ok_or_else(|| {
            ec.new_type_error("WritableStreamDefaultController is missing its abort algorithm")
        })
    }

    fn get_desired_size(&self, _ec: &mut dyn ExecutionContext<Types>) -> Completion<f64, Types> {
        Ok(self.strategy_high_water_mark.get() - self.queue_total_size.get())
    }

    fn get_backpressure(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<bool, Types> {
        Ok(self.get_desired_size(ec)? <= 0.0)
    }

    fn get_chunk_size(
        &self,
        chunk: &JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<f64, Types> {
        let Some(strategy_size_algorithm) = self.strategy_size_algorithm.borrow().clone() else {
            debug_assert_ne!(self.stream_slot(ec)?.state(), WritableStreamState::Writable,);
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let stream = self.stream_slot(ec)?;
        debug_assert_eq!(stream.state(), WritableStreamState::Writable);

        self.clear_algorithms();
        stream.start_erroring(error, ec)
    }

    fn error_if_needed(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if self.stream_slot(ec)?.state() == WritableStreamState::Writable {
            self.error_controller(error, ec)?;
        }
        Ok(())
    }

    fn close_controller(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        self.enqueue_value_with_size(QueueEntryValue::CloseSentinel, 0.0, ec)?;
        self.advance_queue_if_needed(ec)
    }

    fn write_controller(
        &self,
        chunk: JsValue,
        chunk_size: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let stream = self.stream_slot(ec)?;
        if let Err(error) =
            self.enqueue_value_with_size(QueueEntryValue::Chunk(chunk), chunk_size, ec)
        {
            self.error_if_needed(error, ec)?;
            return Ok(());
        }

        if !stream.close_queued_or_in_flight() && stream.state() == WritableStreamState::Writable {
            let backpressure = self.get_backpressure(ec)?;
            stream.update_backpressure(backpressure, ec)?;
        }

        self.advance_queue_if_needed(ec)
    }

    fn advance_queue_if_needed(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
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

    fn process_close(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        let stream = self.stream_slot(ec)?;

        // Step 1: "Perform ! WritableStreamMarkCloseRequestInFlight(stream)."
        stream.mark_close_request_in_flight(ec)?;

        // Step 2: "Perform ! DequeueValue(controller)."
        let _ = self.dequeue_value(ec)?;

        // Step 3: "Assert: controller.[[queue]] is empty."
        debug_assert!(self.queue.borrow().is_empty());

        // Step 4: "Let sinkClosePromise be the result of performing controller.[[closeAlgorithm]]."
        let algorithm = self.close_algorithm(ec)?;
        let sink_close_promise = match algorithm.call(ec) {
            Ok(promise) => promise,
            Err(error) => {
                // If the close algorithm throws synchronously, error the stream
                // via FinishInFlightCloseWithError.
                stream.finish_in_flight_close_with_error(error, ec)?;
                return Ok(());
            }
        };

        // Step 5: "Perform ! WritableStreamDefaultControllerClearAlgorithms(controller)."
        self.clear_algorithms();
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = crate::js::builtin_with_captures(
            ec,
            stream_for_fulfilled,
            process_close_on_fulfilled,
            1,
        );
        let on_rejected =
            crate::js::builtin_with_captures(ec, stream, process_close_on_rejected, 1);
        let promise = Types::object_as_promise(&sink_close_promise)
            .ok_or_else(|| ec.new_type_error("not a Promise"))?;
        ec.perform_promise_then(promise, Some(on_fulfilled), Some(on_rejected), None)?;
        Ok(())
    }

    fn process_write(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot(ec)?;

        // Step 2: "Perform ! WritableStreamMarkFirstWriteRequestInFlight(stream)."
        stream.mark_first_write_request_in_flight(ec)?;

        // Step 3: "Let sinkWritePromise be the result of performing controller.[[writeAlgorithm]], passing in chunk."
        let controller_object = self.controller_object(ec)?;
        let write_algo = self.write_algorithm(ec)?;
        let sink_write_promise = match write_algo.call(&controller_object, chunk, ec) {
            Ok(promise) => promise,
            Err(error) => {
                // If the write algorithm throws synchronously, error the stream
                // via FinishInFlightWriteWithError.
                stream.finish_in_flight_write_with_error(error, ec)?;
                return Ok(());
            }
        };

        let controller_for_fulfilled = self.clone();
        let stream_for_fulfilled = stream.clone();
        let on_fulfilled = crate::js::builtin_with_captures(
            ec,
            (controller_for_fulfilled, stream_for_fulfilled),
            process_write_on_fulfilled,
            1,
        );
        let on_rejected = crate::js::builtin_with_captures(
            ec,
            (self.clone(), stream),
            process_write_on_rejected,
            1,
        );
        let promise = Types::object_as_promise(&sink_write_promise)
            .ok_or_else(|| ec.new_type_error("not a Promise"))?;
        ec.perform_promise_then(promise, Some(on_fulfilled), Some(on_rejected), None)?;
        Ok(())
    }

    fn enqueue_value_with_size(
        &self,
        value: QueueEntryValue,
        chunk_size: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if chunk_size.is_nan() || chunk_size < 0.0 {
            return Err(ec.new_range_error("queue size must be a non-negative number"));
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<QueueEntryValue, Types> {
        self.queue
            .borrow()
            .first()
            .map(|entry| entry.value.clone())
            .ok_or_else(|| ec.new_type_error("WritableStreamDefaultController queue is empty"))
    }

    fn dequeue_value(
        &self,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<QueueEntryValue, Types> {
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(WritableStreamDefaultController, JsObject), Types> {
    let controller = WritableStreamDefaultController::new();
    let controller_object = create_interface_instance::<Types, WritableStreamDefaultController>(
        controller.clone(),
        ec,
    )?
    .into();
    Ok((controller, controller_object))
}

pub(crate) fn with_writable_stream_default_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&WritableStreamDefaultController) -> R,
) -> Completion<R, Types> {
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
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
    let signal = create_abort_signal(AbortSignal::new(ec), ec)?;
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

    // Step 16: "Let startPromise be a promise resolved with startResult."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let start_promise = ec.promise_resolve(intrinsics.promise.clone(), start_result)?;

    // Step 17: "Upon fulfillment of startPromise..."
    let on_fulfilled =
        crate::js::builtin_with_captures(ec, controller.clone(), setup_on_fulfilled, 1);

    // Step 18: "Upon rejection of startPromise with reason r..."
    let on_rejected = crate::js::builtin_with_captures(ec, controller, setup_on_rejected, 1);
    ec.perform_promise_then(start_promise, Some(on_fulfilled), Some(on_rejected), None)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#set-up-writable-stream-default-controller-from-underlying-sink>
pub(crate) fn set_up_writable_stream_default_controller_from_underlying_sink(
    stream: WritableStream,
    underlying_sink_object: Option<JsObject>,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: "Let controller be a new WritableStreamDefaultController."
    let (controller, controller_object) = create_writable_stream_default_controller(ec)?;

    // Step 2-9: Extract optional methods.
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    controller.close_controller(ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-error-if-needed>
pub(crate) fn writable_stream_default_controller_error_if_needed(
    controller: WritableStreamDefaultController,
    error: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    controller.error_if_needed(error, ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-chunk-size>
pub(crate) fn writable_stream_default_controller_get_chunk_size(
    controller: WritableStreamDefaultController,
    chunk: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<f64, Types> {
    controller.get_chunk_size(chunk, ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-get-desired-size>
pub(crate) fn writable_stream_default_controller_get_desired_size(
    controller: WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<f64, Types> {
    controller.get_desired_size(ec)
}

/// <https://streams.spec.whatwg.org/#writable-stream-default-controller-write>
pub(crate) fn writable_stream_default_controller_write(
    controller: WritableStreamDefaultController,
    chunk: JsValue,
    chunk_size: f64,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    controller.write_controller(chunk, chunk_size, ec)
}

fn get_callable_method(
    object: &JsObject,
    property: &'static str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<JsObject>, Types> {
    let pk = ec.property_key_from_str(property);
    let value = ExecutionContext::get(ec, object.clone(), pk)?;
    if <Types as JsTypes>::value_is_undefined(&value) {
        return Ok(None);
    }

    let method = <Types as JsTypes>::value_as_object(&value).ok_or_else(|| {
        ec.new_type_error(&format!(
            "WritableStream underlyingSink.{property} must be callable when provided"
        ))
    })?;
    let method_val = <Types as JsTypes>::value_from_object(method.clone());
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    captures.finish_in_flight_close(ec)?;
    Ok(ec.value_undefined())
}

fn process_close_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &WritableStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    captures.finish_in_flight_close_with_error(
        args.first().cloned().unwrap_or(ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn process_write_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(WritableStreamDefaultController, WritableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, stream) = captures;
    controller.dequeue_value(ec)?;
    // Note: finish_in_flight_write must be called before advance_queue_if_needed,
    // so the in-flight slot is available for the next queued write.
    stream.finish_in_flight_write(ec)?;
    let state = stream.state();
    debug_assert!(state == WritableStreamState::Writable || state == WritableStreamState::Erroring);
    if !stream.close_queued_or_in_flight() && state == WritableStreamState::Writable {
        let backpressure = controller.get_backpressure(ec)?;
        stream.update_backpressure(backpressure, ec)?;
    }
    controller.advance_queue_if_needed(ec)?;
    Ok(ec.value_undefined())
}

fn process_write_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &(WritableStreamDefaultController, WritableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (controller, stream) = captures;
    if stream.state() == WritableStreamState::Writable {
        controller.clear_algorithms();
    }
    stream.finish_in_flight_write_with_error(
        args.first().cloned().unwrap_or(ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn setup_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    captures.set_started(true);
    captures.advance_queue_if_needed(ec)?;
    Ok(ec.value_undefined())
}

fn setup_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &WritableStreamDefaultController,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    captures.set_started(true);
    let stream = captures.stream_slot(ec)?;
    stream.deal_with_rejection(args.first().cloned().unwrap_or(ec.value_undefined()), ec)?;
    Ok(ec.value_undefined())
}
