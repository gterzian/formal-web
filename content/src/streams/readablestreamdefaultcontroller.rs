use std::{cell::Cell, collections::VecDeque, rc::Rc};

use boa_engine::{
    JsArgs, JsError, JsNativeError, JsResult, JsValue,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Trace};

use crate::streams::SizeAlgorithm;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{mark_promise_as_handled, resolved_promise};
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::readablestream::{
    ByteTeeState, ReadableStreamFromIterableState, TeeState,
    readable_byte_stream_tee_cancel1_algorithm_ec,
    readable_byte_stream_tee_cancel2_algorithm_ec,
    readable_byte_stream_tee_pull1_algorithm_ec,
    readable_byte_stream_tee_pull2_algorithm_ec,
    readable_stream_add_read_request, readable_stream_close, readable_stream_close_ec,
    readable_stream_default_tee_cancel1_algorithm, readable_stream_default_tee_cancel2_algorithm,
    readable_stream_default_tee_pull_algorithm, readable_stream_error,
    readable_stream_from_iterable_cancel_algorithm_ec,
    readable_stream_from_iterable_pull_algorithm_ec,
    readable_stream_fulfill_read_request, readable_stream_get_num_read_requests,
};
use super::transformstream::{
    transform_stream_default_source_cancel_algorithm_ec,
    transform_stream_default_source_pull_algorithm,
};
use super::{
    ReadRequest, ReadableStream, ReadableStreamController, ReadableStreamState, SourceMethod,
    TransformStream, range_error_value,
};

use js_engine::{Completion, ExecutionContext, JsEngine, JsTypes};

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum PullAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
    ReadableStreamFromIterable(ReadableStreamFromIterableState),
    ReadableStreamDefaultTee {
        tee_state: GcCell<TeeState>,
        clone_for_branch2: bool,
    },
    ReadableByteStreamTeeBranch1(GcCell<ByteTeeState>),
    ReadableByteStreamTeeBranch2(GcCell<ByteTeeState>),
    TransformStreamDefaultSourcePull(TransformStream),
}

impl PullAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    pub(crate) fn call(
        &self,
        controller_object: &JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::ReturnUndefined => {
                resolved_promise(ec.value_undefined(), ec)
            }
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller_object.clone());
                let result = callback.call(&[arg], ec)?;
                resolved_promise(result, ec)
            }
            Self::ReadableStreamFromIterable(state) => {
                readable_stream_from_iterable_pull_algorithm_ec(state.clone(), ec)
            }
            Self::ReadableStreamDefaultTee {
                tee_state,
                clone_for_branch2,
            } => {
                let value = readable_stream_default_tee_pull_algorithm(
                    tee_state.clone(),
                    *clone_for_branch2,
                    ec,
                )?;
                resolved_promise(value, ec)
            }
            Self::ReadableByteStreamTeeBranch1(tee_state) => {
                let value = readable_byte_stream_tee_pull1_algorithm_ec(
                    tee_state.clone(), ec,
                )?;
                resolved_promise(value, ec)
            }
            Self::ReadableByteStreamTeeBranch2(tee_state) => {
                let value = readable_byte_stream_tee_pull2_algorithm_ec(
                    tee_state.clone(), ec,
                )?;
                resolved_promise(value, ec)
            }
            Self::TransformStreamDefaultSourcePull(stream) => {
                transform_stream_default_source_pull_algorithm(stream.clone(), ec)
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum CancelAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
    ReadableStreamFromIterable(ReadableStreamFromIterableState),
    ReadableStreamDefaultTeeBranch1(GcCell<TeeState>),
    ReadableStreamDefaultTeeBranch2(GcCell<TeeState>),
    ReadableByteStreamTeeBranch1(GcCell<ByteTeeState>),
    ReadableByteStreamTeeBranch2(GcCell<ByteTeeState>),
    TransformStreamDefaultSourceCancel(TransformStream),
}

impl CancelAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    pub(crate) fn call(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        match self {
            Self::ReturnUndefined => resolved_promise(ec.value_undefined(), ec),
            Self::JavaScript(callback) => {
                let result = callback.call(&[reason], ec)?;
                resolved_promise(result, ec)
            }
            Self::ReadableStreamFromIterable(state) => {
                readable_stream_from_iterable_cancel_algorithm_ec(
                    state.clone(), reason, ec,
                )
            }
            Self::ReadableStreamDefaultTeeBranch1(tee_state) => {
                readable_stream_default_tee_cancel1_algorithm(
                    tee_state.clone(), reason, ec,
                )
            }
            Self::ReadableStreamDefaultTeeBranch2(tee_state) => {
                readable_stream_default_tee_cancel2_algorithm(
                    tee_state.clone(), reason, ec,
                )
            }
            Self::ReadableByteStreamTeeBranch1(tee_state) => {
                readable_byte_stream_tee_cancel1_algorithm_ec(
                    tee_state.clone(), reason, ec,
                )
            }
            Self::ReadableByteStreamTeeBranch2(tee_state) => {
                readable_byte_stream_tee_cancel2_algorithm_ec(
                    tee_state.clone(), reason, ec,
                )
            }
            Self::TransformStreamDefaultSourceCancel(stream) => {
                transform_stream_default_source_cancel_algorithm_ec(
                    stream.clone(), reason, ec,
                )
            }
        }
    }
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller-from-underlying-source>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum StartAlgorithm {
    ReturnUndefined,
    ReturnValue(JsValue),
    JavaScript(SourceMethod),
}

impl StartAlgorithm {
    /// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller>
    pub(crate) fn call(
        &self,
        controller_object: &JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        match self {
            Self::ReturnUndefined => Ok(ec.value_undefined()),
            Self::ReturnValue(value) => Ok(value.clone()),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller_object.clone());
                callback.call(&[arg], ec)
            }
        }
    }
}
/// `EnqueueValueWithSize` computes for it.
#[derive(Clone, Trace, Finalize)]
struct QueueEntry {
    chunk: JsValue,
    #[unsafe_ignore_trace]
    size: f64,
}

/// <https://streams.spec.whatwg.org/#rs-default-controller-class>
#[gc_struct]
pub struct ReadableStreamDefaultController {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-stream>
    stream: GcCell<Option<ReadableStream>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-queue>
    queue: GcCell<VecDeque<QueueEntry>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-queuetotalsize>
    #[unsafe_ignore_trace]
    queue_total_size: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-started>
    #[unsafe_ignore_trace]
    started: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-closerequested>
    #[unsafe_ignore_trace]
    close_requested: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullagain>
    #[unsafe_ignore_trace]
    pull_again: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pulling>
    #[unsafe_ignore_trace]
    pulling: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-strategysizealgorithm>
    strategy_size_algorithm: GcCell<Option<SizeAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    pull_algorithm: GcCell<Option<PullAlgorithm>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    cancel_algorithm: GcCell<Option<CancelAlgorithm>>,
}

impl ReadableStreamDefaultController {
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            queue: gc_cell_new(VecDeque::new()),
            queue_total_size: Rc::new(Cell::new(0.0)),
            started: Rc::new(Cell::new(false)),
            close_requested: Rc::new(Cell::new(false)),
            pull_again: Rc::new(Cell::new(false)),
            pulling: Rc::new(Cell::new(false)),
            strategy_size_algorithm: gc_cell_new(None),
            strategy_high_water_mark: Rc::new(Cell::new(0.0)),
            pull_algorithm: gc_cell_new(None),
            cancel_algorithm: gc_cell_new(None),
        }
    }

    fn stream_slot(&self) -> JsResult<ReadableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its stream")
                .into()
        })
    }

    fn stream_slot_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<ReadableStream, crate::js::Types> {
        self.stream.borrow().clone().ok_or_else(|| {
            ec.new_type_error("ReadableStreamDefaultController is missing its stream")
        })
    }

    fn controller_object(&self) -> JsResult<JsObject> {
        self.stream_slot()?.controller_object_slot().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its JavaScript object")
                .into()
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
                    "ReadableStreamDefaultController is missing its JavaScript object",
                )
            })
    }

    fn queue_is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-desired-size>
    pub(crate) fn desired_size(&self) -> JsResult<Option<f64>> {
        // Step 1: "Return ! ReadableStreamDefaultControllerGetDesiredSize(this)."
        self.get_desired_size()
    }

    pub(crate) fn desired_size_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<f64>, crate::js::Types> {
        // Step 1: "Return ! ReadableStreamDefaultControllerGetDesiredSize(this)."
        self.get_desired_size_ec(ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-close>
    pub(crate) fn close(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !self.can_close_or_enqueue_ec(ec)? {
            return Err(ec.new_type_error("The stream is not in a state that permits close"));
        }

        // Step 2: "Perform ! ReadableStreamDefaultControllerClose(this)."
        self.close_steps(ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-enqueue>
    pub(crate) fn enqueue(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !self.can_close_or_enqueue_ec(ec)? {
            return Err(ec.new_type_error("The stream is not in a state that permits enqueue"));
        }

        // Step 2: "Perform ? ReadableStreamDefaultControllerEnqueue(this, chunk)."
        self.enqueue_steps(chunk, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-error>
    pub(crate) fn error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Perform ! ReadableStreamDefaultControllerError(this, e)."
        self.error_steps(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-private-cancel>
    pub(crate) fn cancel_steps(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        // Step 1: "Perform ! ResetQueue(this)."
        self.reset_queue();

        let cancel_algorithm = self.cancel_algorithm.borrow().clone();

        // Step 2: "Let result be the result of performing this.[[cancelAlgorithm]], passing reason."
        let result = match cancel_algorithm {
            Some(cancel_algorithm) => cancel_algorithm.call(reason, ec)?,
            None => resolved_promise(ec.value_undefined(), ec)?,
        };

        // Step 3: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(this)."
        self.clear_algorithms();

        // Step 4: "Return result."
        Ok(result)
    }

    /// <https://streams.spec.whatwg.org/#rs-default-controller-private-pull>
    pub(crate) fn pull_steps(
        &self,
        read_request: ReadRequest,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Let stream be this.[[stream]]."
        let stream = self.stream_slot_ec(ec)?;

        // Step 2: "If this.[[queue]] is not empty,"
        if !self.queue_is_empty() {
            let (chunk, should_close_stream) = {
                let mut queue = self.queue.borrow_mut();

                // Step 2.1: "Let chunk be ! DequeueValue(this)."
                let entry = queue
                    .pop_front()
                    .expect("queue was checked to be non-empty");
                {
                    let mut new_size = self.queue_total_size.get() - entry.size;
                    if new_size <= 0.0 {
                        new_size = 0.0;
                    }
                    self.queue_total_size.set(new_size);
                }

                // Step 2.2: "If this.[[closeRequested]] is true and this.[[queue]] is empty,"
                let should_close_stream = self.close_requested.get() && queue.is_empty();
                (entry.chunk.clone(), should_close_stream)
            };

            if should_close_stream {
                // Step 2.2.1: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(this)."
                self.clear_algorithms();

                // Step 2.2.2: "Perform ! ReadableStreamClose(stream)."
                readable_stream_close_ec(stream, ec)?;
            } else {
                // Step 2.3: "Otherwise, perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
                self.call_pull_if_needed(ec)?;
            }

            // Step 2.4: "Perform readRequest's chunk steps, given chunk."
            return read_request.chunk_steps(chunk, ec);
        }

        // Step 3.1: "Perform ! ReadableStreamAddReadRequest(stream, readRequest)."
        readable_stream_add_read_request(stream.clone(), read_request, ec)?;

        // Step 3.2: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaultcontroller-releasesteps>
    pub(crate) fn release_steps(
        &self,
        _ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Return."
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-call-pull-if-needed>
    pub(crate) fn call_pull_if_needed(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Let shouldPull be ! ReadableStreamDefaultControllerShouldCallPull(controller)."
        let should_pull = self.should_call_pull_ec(ec)?;

        // Step 2: "If shouldPull is false, return."
        if !should_pull {
            return Ok(());
        }

        // Step 3: "If controller.[[pulling]] is true,"
        if self.pulling.get() {
            // Step 3.1: "Set controller.[[pullAgain]] to true."
            self.pull_again.set(true);

            // Step 3.2: "Return."
            return Ok(());
        }

        // Step 4: "Assert: controller.[[pullAgain]] is false."
        debug_assert!(!self.pull_again.get());

        // Step 5: "Set controller.[[pulling]] to true."
        self.pulling.set(true);

        // Step 6: "Let pullPromise be the result of performing controller.[[pullAlgorithm]]."
        let controller_object = self.controller_object_ec(ec)?;
        let pull_algorithm = self.pull_algorithm.borrow().clone();
        let pull_promise: JsObject = match pull_algorithm {
            Some(pull_algorithm) => pull_algorithm.call(&controller_object, ec)?,
            None => resolved_promise(ec.value_undefined(), ec)?,
        };

        let pull_js_promise =
            crate::js::Types::object_as_promise(&pull_promise)
                .ok_or_else(|| JsValue::undefined())?;
        let on_fulfilled =
            crate::js::builtin_with_captures(ec, self.clone(), pull_steps_on_fulfilled, 1);
        let on_rejected =
            crate::js::builtin_with_captures(ec, self.clone(), pull_steps_on_rejected, 1);
        let pull_reaction_val =
            ec.perform_promise_then(pull_js_promise, Some(on_fulfilled), Some(on_rejected), None)?;
        let pull_reaction: JsObject =
            crate::js::Types::value_as_object(&pull_reaction_val)
                .ok_or_else(|| ec.new_type_error("promise.then returned non-object"))?;
        mark_promise_as_handled(&pull_reaction, ec)?;
        mark_promise_as_handled(&pull_promise, ec)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-can-close-or-enqueue>
    pub(crate) fn can_close_or_enqueue(&self) -> JsResult<bool> {
        // Step 1: "Let state be controller.[[stream]].[[state]]."
        let state = self.stream_slot()?.state();

        // Step 2: "If controller.[[closeRequested]] is false and state is \"readable\", return true."
        if !self.close_requested.get() && state == ReadableStreamState::Readable {
            return Ok(true);
        }

        // Step 3: "Otherwise, return false."
        Ok(false)
    }

    pub(crate) fn can_close_or_enqueue_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        // Step 1: "Let state be controller.[[stream]].[[state]]."
        let state = self.stream_slot_ec(ec)?.state();

        // Step 2: "If controller.[[closeRequested]] is false and state is \"readable\", return true."
        if !self.close_requested.get() && state == ReadableStreamState::Readable {
            return Ok(true);
        }

        // Step 3: "Otherwise, return false."
        Ok(false)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-close>
    pub(crate) fn close_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
        if !self.can_close_or_enqueue_ec(ec)? {
            return Ok(());
        }

        // Step 2: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot_ec(ec)?;

        // Step 3: "Set controller.[[closeRequested]] to true."
        self.close_requested.set(true);

        // Step 4: "If controller.[[queue]] is empty,"
        if self.queue_is_empty() {
            // Step 4.1: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
            self.clear_algorithms();

            // Step 4.2: "Perform ! ReadableStreamClose(stream)."
            readable_stream_close_ec(stream, ec)?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-enqueue>
    pub(crate) fn enqueue_steps(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
        if !self.can_close_or_enqueue_ec(ec)? {
            return Ok(());
        }

        // Step 2: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot_ec(ec)?;

        // Step 3: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, perform ! ReadableStreamFulfillReadRequest(stream, chunk, false)."
        if stream.is_readable_stream_locked()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            readable_stream_fulfill_read_request(stream, chunk, false, ec)?;
        } else {
            // Step 4.1: "Let result be the result of performing controller.[[strategySizeAlgorithm]], passing in chunk, and interpreting the result as a completion record."
            let size_algorithm =
                self.strategy_size_algorithm
                    .borrow()
                    .clone()
                    .ok_or_else(|| {
                        ec.new_type_error(
                            "ReadableStreamDefaultController is missing its size algorithm",
                        )
                    })?;
            let chunk_size = match size_algorithm.size(&chunk, ec) {
                Ok(chunk_size) => chunk_size,
                Err(error) => {
                    // Step 4.2.1: "Perform ! ReadableStreamDefaultControllerError(controller, result.[[Value]])."
                    self.error_steps(error.clone(), ec)?;

                    // Step 4.2.2: "Return result."
                    return Err(error);
                }
            };

            // Step 4.3: "Let chunkSize be result.[[Value]]."

            // Step 4.4: "Let enqueueResult be EnqueueValueWithSize(controller, chunk, chunkSize)."
            if !chunk_size.is_finite() || chunk_size < 0.0 {
                let error = range_error_value(
                    "queue strategy size must be a finite, non-negative number",
                    ec,
                )?;

                // Step 4.5.1: "Perform ! ReadableStreamDefaultControllerError(controller, enqueueResult.[[Value]])."
                self.error_steps(error.clone(), ec)?;

                // Step 4.5.2: "Return enqueueResult."
                return Err(error);
            }

            self.enqueue_value_with_size(chunk, chunk_size);
        }

        // Step 5: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
        self.call_pull_if_needed(ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-error>
    pub(crate) fn error_steps(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot_ec(ec)?;

        // Step 2: "If stream.[[state]] is not \"readable\", return."
        if stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        // Step 3: "Perform ! ResetQueue(controller)."
        self.reset_queue();

        // Step 4: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
        self.clear_algorithms();

        // Step 5: "Perform ! ReadableStreamError(stream, e)."
        readable_stream_error(stream, error, ec)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-get-desired-size>
    pub(crate) fn get_desired_size(&self) -> JsResult<Option<f64>> {
        // Step 1: "Let state be controller.[[stream]].[[state]]."
        let state = self.stream_slot()?.state();

        // Step 2: "If state is \"errored\", return null."
        if state == ReadableStreamState::Errored {
            return Ok(None);
        }

        // Step 3: "If state is \"closed\", return 0."
        if state == ReadableStreamState::Closed {
            return Ok(Some(0.0));
        }

        // Step 4: "Return controller.[[strategyHWM]] - controller.[[queueTotalSize]]."
        Ok(Some(
            self.strategy_high_water_mark.get() - self.queue_total_size.get(),
        ))
    }

    pub(crate) fn get_desired_size_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<f64>, crate::js::Types> {
        // Step 1: "Let state be controller.[[stream]].[[state]]."
        let state = self.stream_slot_ec(ec)?.state();

        // Step 2: "If state is \"errored\", return null."
        if state == ReadableStreamState::Errored {
            return Ok(None);
        }

        // Step 3: "If state is \"closed\", return 0."
        if state == ReadableStreamState::Closed {
            return Ok(Some(0.0));
        }

        // Step 4: "Return controller.[[strategyHWM]] - controller.[[queueTotalSize]]."
        Ok(Some(
            self.strategy_high_water_mark.get() - self.queue_total_size.get(),
        ))
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-has-backpressure>
    pub(crate) fn has_backpressure(&self) -> JsResult<bool> {
        Ok(!self.should_call_pull()?)
    }

    pub(crate) fn has_backpressure_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        Ok(!self.should_call_pull_ec(ec)?)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-should-call-pull>
    fn should_call_pull(&self) -> JsResult<bool> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot()?;

        // Step 2: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return false."
        if !self.can_close_or_enqueue()? {
            return Ok(false);
        }

        // Step 3: "If controller.[[started]] is false, return false."
        if !self.started.get() {
            return Ok(false);
        }

        // Step 4: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, return true."
        if stream.is_readable_stream_locked()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            return Ok(true);
        }

        // Step 5: "Let desiredSize be ! ReadableStreamDefaultControllerGetDesiredSize(controller)."
        let desired_size = self.get_desired_size()?;

        // Step 6: "Assert: desiredSize is not null."
        debug_assert!(desired_size.is_some());

        // Step 7: "If desiredSize > 0, return true."
        if desired_size.unwrap_or(0.0) > 0.0 {
            return Ok(true);
        }

        // Step 8: "Return false."
        Ok(false)
    }

    fn should_call_pull_ec(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = self.stream_slot_ec(ec)?;

        // Step 2: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return false."
        if !self.can_close_or_enqueue_ec(ec)? {
            return Ok(false);
        }

        // Step 3: "If controller.[[started]] is false, return false."
        if !self.started.get() {
            return Ok(false);
        }

        // Step 4: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, return true."
        if stream.is_readable_stream_locked()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            return Ok(true);
        }

        // Step 5: "Let desiredSize be ! ReadableStreamDefaultControllerGetDesiredSize(controller)."
        let desired_size = self.get_desired_size_ec(ec)?;

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
    fn clear_algorithms(&self) {
        *self.pull_algorithm.borrow_mut() = None;
        *self.cancel_algorithm.borrow_mut() = None;
        *self.strategy_size_algorithm.borrow_mut() = None;
    }

    /// <https://streams.spec.whatwg.org/#enqueue-value-with-size>
    fn enqueue_value_with_size(&self, chunk: JsValue, chunk_size: f64) {
        self.queue.borrow_mut().push_back(QueueEntry {
            chunk,
            size: chunk_size,
        });
        self.queue_total_size
            .set(self.queue_total_size.get() + chunk_size);
    }

    /// <https://streams.spec.whatwg.org/#reset-queue>
    fn reset_queue(&self) {
        self.queue.borrow_mut().clear();
        self.queue_total_size.set(0.0);
    }
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller>
pub(crate) fn set_up_readable_stream_default_controller(
    stream: ReadableStream,
    controller: ReadableStreamDefaultController,
    controller_object: &JsObject,
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream.[[controller]] is undefined."
    debug_assert!(stream.controller_slot().is_none());

    // Step 2: "Set controller.[[stream]] to stream."
    *controller.stream.borrow_mut() = Some(stream.clone());

    // Step 3: "Perform ! ResetQueue(controller)."
    controller.reset_queue();

    // Step 4: "Set controller.[[started]], controller.[[closeRequested]], controller.[[pullAgain]], and controller.[[pulling]] to false."
    controller.started.set(false);
    controller.close_requested.set(false);
    controller.pull_again.set(false);
    controller.pulling.set(false);

    // Step 5: "Set controller.[[strategySizeAlgorithm]] to sizeAlgorithm and controller.[[strategyHWM]] to highWaterMark."
    *controller.strategy_size_algorithm.borrow_mut() = Some(size_algorithm);
    controller.strategy_high_water_mark.set(high_water_mark);

    // Step 6: "Set controller.[[pullAlgorithm]] to pullAlgorithm."
    *controller.pull_algorithm.borrow_mut() = Some(pull_algorithm);

    // Step 7: "Set controller.[[cancelAlgorithm]] to cancelAlgorithm."
    *controller.cancel_algorithm.borrow_mut() = Some(cancel_algorithm);

    // Step 8: "Set stream.[[controller]] to controller."
    stream.set_controller_slot(Some(ReadableStreamController::Default(controller.clone())));
    stream.set_controller_object_slot(Some(controller_object.clone()));

    // Step 9: "Let startResult be the result of performing startAlgorithm. (This might throw an exception.)"
    let start_result = start_algorithm.call(controller_object, ec)?;

    // Step 10: "Let startPromise be a promise resolved with startResult."
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let start_promise = ec.promise_resolve(intrinsics.promise.clone(), start_result)?;
    let on_fulfilled =
        crate::js::builtin_with_captures(ec, controller.clone(), setup_on_fulfilled, 1);
    let on_rejected = crate::js::builtin_with_captures(ec, controller, setup_on_rejected, 1);
    let start_reaction_val =
        ec.perform_promise_then(start_promise.clone(), Some(on_fulfilled), Some(on_rejected), None)?;
    let start_reaction: JsObject =
        crate::js::Types::value_as_object(&start_reaction_val)
            .ok_or_else(|| ec.new_type_error("promise.then returned non-object"))?;
    mark_promise_as_handled(&start_reaction, ec)?;
    mark_promise_as_handled(&JsObject::from(start_promise), ec)?;
    Ok(())
}

/// <https://streams.spec.whatwg.org/#set-up-readable-stream-default-controller-from-underlying-source>
pub(crate) fn set_up_readable_stream_default_controller_from_underlying_source(
    stream: ReadableStream,
    underlying_source_object: Option<JsObject>,
    high_water_mark: f64,
    size_algorithm: SizeAlgorithm,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Let controller be a new ReadableStreamDefaultController."
    let controller = ReadableStreamDefaultController::new();
    let controller_object = create_interface_instance::<
        crate::js::Types,
        ReadableStreamDefaultController,
    >(controller.clone(), ec)?;

    // Step 2: "Let startAlgorithm be an algorithm that returns undefined."
    let mut start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 3: "Let pullAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut pull_algorithm = PullAlgorithm::ReturnUndefined;

    // Step 4: "Let cancelAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut cancel_algorithm = CancelAlgorithm::ReturnUndefined;

    // Step 5: "If underlyingSourceDict[\"start\"] exists, then set startAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"start\"] with argument list « controller » and callback this value underlyingSource."
    if let Some(start_method) =
        extract_source_method(underlying_source_object.as_ref(), "start", ec)?
    {
        start_algorithm = StartAlgorithm::JavaScript(start_method);
    }

    // Step 6: "If underlyingSourceDict[\"pull\"] exists, then set pullAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"pull\"] with argument list « controller » and callback this value underlyingSource."
    if let Some(pull_method) =
        extract_source_method(underlying_source_object.as_ref(), "pull", ec)?
    {
        pull_algorithm = PullAlgorithm::JavaScript(pull_method);
    }

    // Step 7: "If underlyingSourceDict[\"cancel\"] exists, then set cancelAlgorithm to an algorithm which takes an argument reason and returns the result of invoking underlyingSourceDict[\"cancel\"] with argument list « reason » and callback this value underlyingSource."
    if let Some(cancel_method) =
        extract_source_method(underlying_source_object.as_ref(), "cancel", ec)?
    {
        cancel_algorithm = CancelAlgorithm::JavaScript(cancel_method);
    }

    // Step 8: "Perform ? SetUpReadableStreamDefaultController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller(
        stream,
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        size_algorithm,
        ec,
    )
}
/// underlying source object as the callback this value required by the Streams setup algorithm.
pub(crate) fn extract_source_method(
    source_object: Option<&JsObject>,
    name: &str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<SourceMethod>, crate::js::Types> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let pk = ec.property_key_from_str(name);
    let value = ExecutionContext::get(ec, source_object.clone(), pk)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let callback = value
        .as_object()
        .ok_or_else(|| ec.new_type_error(&format!("underlying source {name} must be callable")))?;

    let callback_val = <crate::js::Types as JsTypes>::value_from_object(callback.clone());
    if !ec.is_callable(&callback_val) {
        return Err(ec.new_type_error(&format!("underlying source {name} must be callable")));
    }

    Ok(Some(SourceMethod::new(
        source_object.clone(),
        crate::webidl::Callback::from_object(callback),
    )))
}

fn pull_steps_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &ReadableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.pulling.set(false);
    let should_pull_again = captures.pull_again.get();
    if should_pull_again {
        captures.pull_again.set(false);
        captures.call_pull_if_needed(ec)?;
    }
    Ok(JsValue::undefined())
}

fn pull_steps_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(
        args.first().cloned().unwrap_or(JsValue::undefined()),
        ec,
    )?;
    Ok(JsValue::undefined())
}

fn setup_on_fulfilled(
    _args: &[JsValue],
    _this: JsValue,
    captures: &ReadableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.started.set(true);
    debug_assert!(!captures.pulling.get());
    debug_assert!(!captures.pull_again.get());
    captures.call_pull_if_needed(ec)?;
    Ok(JsValue::undefined())
}

fn setup_on_rejected(
    args: &[JsValue],
    _this: JsValue,
    captures: &ReadableStreamDefaultController,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    captures.error_steps(
        args.first().cloned().unwrap_or(JsValue::undefined()),
        ec,
    )?;
    Ok(JsValue::undefined())
}
