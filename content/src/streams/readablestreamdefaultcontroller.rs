use std::{cell::Cell, collections::VecDeque, rc::Rc};

use boa_engine::{
    JsArgs, JsData, JsNativeError, JsResult, JsString, JsValue,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::streams::SizeAlgorithm;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{mark_promise_as_handled, promise_from_completion, resolved_promise};

use super::readablestream::{
    ByteTeeState, ReadableStreamFromIterableState, TeeState,
    readable_byte_stream_tee_cancel1_algorithm, readable_byte_stream_tee_cancel2_algorithm,
    readable_byte_stream_tee_pull1_algorithm, readable_byte_stream_tee_pull2_algorithm,
    readable_stream_add_read_request, readable_stream_close,
    readable_stream_default_tee_cancel1_algorithm, readable_stream_default_tee_cancel2_algorithm,
    readable_stream_default_tee_pull_algorithm, readable_stream_error,
    readable_stream_from_iterable_cancel_algorithm, readable_stream_from_iterable_pull_algorithm,
    readable_stream_fulfill_read_request, readable_stream_get_num_read_requests,
};
use super::transformstream::{
    transform_stream_default_source_cancel_algorithm,
    transform_stream_default_source_pull_algorithm,
};
use super::{
    ReadRequest, ReadableStream, ReadableStreamController, ReadableStreamState, SourceMethod,
    TransformStream, range_error_value,
};

use js_engine::{Completion, ExecutionContext};

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum PullAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
    ReadableStreamFromIterable(ReadableStreamFromIterableState),
    ReadableStreamDefaultTee {
        tee_state: Gc<GcRefCell<TeeState>>,
        clone_for_branch2: bool,
    },
    ReadableByteStreamTeeBranch1(Gc<GcRefCell<ByteTeeState>>),
    ReadableByteStreamTeeBranch2(Gc<GcRefCell<ByteTeeState>>),
    TransformStreamDefaultSourcePull(TransformStream),
}

impl PullAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    pub(crate) fn call(
        &self,
        controller_object: &JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> JsPromise {
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        // readable_stream_from_iterable_pull_algorithm and tee algorithms still take Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        match self {
            Self::ReturnUndefined => promise_from_completion(Ok(JsValue::undefined()), ec),
            Self::JavaScript(callback) => {
                let arg = JsValue::from(controller_object.clone());
                promise_from_completion(
                    crate::js::completion_to_js_result(callback.call(&[arg], ec)),
                    ec,
                )
            }
            Self::ReadableStreamFromIterable(state) => promise_from_completion(
                readable_stream_from_iterable_pull_algorithm(state.clone(), context)
                    .map(JsValue::from),
                ec,
            ),
            Self::ReadableStreamDefaultTee {
                tee_state,
                clone_for_branch2,
            } => promise_from_completion(
                readable_stream_default_tee_pull_algorithm(
                    tee_state.clone(),
                    *clone_for_branch2,
                    context,
                ),
                ec,
            ),
            Self::ReadableByteStreamTeeBranch1(tee_state) => promise_from_completion(
                readable_byte_stream_tee_pull1_algorithm(tee_state.clone(), context),
                ec,
            ),
            Self::ReadableByteStreamTeeBranch2(tee_state) => promise_from_completion(
                readable_byte_stream_tee_pull2_algorithm(tee_state.clone(), context),
                ec,
            ),
            Self::TransformStreamDefaultSourcePull(stream) => promise_from_completion(
                transform_stream_default_source_pull_algorithm(stream.clone(), context)
                    .map(JsValue::from),
                ec,
            ),
        }
    }
}

/// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
#[derive(Clone, Trace, Finalize)]
pub(crate) enum CancelAlgorithm {
    ReturnUndefined,
    JavaScript(SourceMethod),
    ReadableStreamFromIterable(ReadableStreamFromIterableState),
    ReadableStreamDefaultTeeBranch1(Gc<GcRefCell<TeeState>>),
    ReadableStreamDefaultTeeBranch2(Gc<GcRefCell<TeeState>>),
    ReadableByteStreamTeeBranch1(Gc<GcRefCell<ByteTeeState>>),
    ReadableByteStreamTeeBranch2(Gc<GcRefCell<ByteTeeState>>),
    TransformStreamDefaultSourceCancel(TransformStream),
}

impl CancelAlgorithm {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    pub(crate) fn call(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> JsPromise {
        // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
        // readable_stream_from_iterable_cancel_algorithm and tee algorithms still take Context.
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        match self {
            Self::ReturnUndefined => promise_from_completion(Ok(JsValue::undefined()), ec),
            Self::JavaScript(callback) => promise_from_completion(
                crate::js::completion_to_js_result(callback.call(&[reason], ec)),
                ec,
            ),
            Self::ReadableStreamFromIterable(state) => promise_from_completion(
                readable_stream_from_iterable_cancel_algorithm(state.clone(), reason, context)
                    .map(JsValue::from),
                ec,
            ),
            Self::ReadableStreamDefaultTeeBranch1(tee_state) => promise_from_completion(
                readable_stream_default_tee_cancel1_algorithm(tee_state.clone(), reason, context),
                ec,
            ),
            Self::ReadableStreamDefaultTeeBranch2(tee_state) => promise_from_completion(
                readable_stream_default_tee_cancel2_algorithm(tee_state.clone(), reason, context),
                ec,
            ),
            Self::ReadableByteStreamTeeBranch1(tee_state) => promise_from_completion(
                readable_byte_stream_tee_cancel1_algorithm(tee_state.clone(), reason, context)
                    .map(JsValue::from),
                ec,
            ),
            Self::ReadableByteStreamTeeBranch2(tee_state) => promise_from_completion(
                readable_byte_stream_tee_cancel2_algorithm(tee_state.clone(), reason, context)
                    .map(JsValue::from),
                ec,
            ),
            Self::TransformStreamDefaultSourceCancel(stream) => promise_from_completion(
                transform_stream_default_source_cancel_algorithm(stream.clone(), reason, context)
                    .map(JsValue::from),
                ec,
            ),
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
/// `EnqueueValueWithSize` computes for it.
#[derive(Clone, Trace, Finalize)]
struct QueueEntry {
    chunk: JsValue,
    #[unsafe_ignore_trace]
    size: f64,
}

/// <https://streams.spec.whatwg.org/#rs-default-controller-class>
js_engine::impl_gc_traits! {
    #[derive(Clone)]
    pub struct ReadableStreamDefaultController {
    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-stream>
    stream: Gc<GcRefCell<Option<ReadableStream>>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-queue>
    queue: Gc<GcRefCell<VecDeque<QueueEntry>>>,

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
    strategy_size_algorithm: Gc<GcRefCell<Option<SizeAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-strategyhwm>
    #[unsafe_ignore_trace]
    strategy_high_water_mark: Rc<Cell<f64>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-pullalgorithm>
    pull_algorithm: Gc<GcRefCell<Option<PullAlgorithm>>>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultcontroller-cancelalgorithm>
    cancel_algorithm: Gc<GcRefCell<Option<CancelAlgorithm>>>,
}
}

impl ReadableStreamDefaultController {
    pub(crate) fn new() -> Self {
        Self {
            stream: Gc::new(GcRefCell::new(None)),
            queue: Gc::new(GcRefCell::new(VecDeque::new())),
            queue_total_size: Rc::new(Cell::new(0.0)),
            started: Rc::new(Cell::new(false)),
            close_requested: Rc::new(Cell::new(false)),
            pull_again: Rc::new(Cell::new(false)),
            pulling: Rc::new(Cell::new(false)),
            strategy_size_algorithm: Gc::new(GcRefCell::new(None)),
            strategy_high_water_mark: Rc::new(Cell::new(0.0)),
            pull_algorithm: Gc::new(GcRefCell::new(None)),
            cancel_algorithm: Gc::new(GcRefCell::new(None)),
        }
    }

    fn stream_slot(&self) -> JsResult<ReadableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its stream")
                .into()
        })
    }

    fn controller_object(&self) -> JsResult<JsObject> {
        self.stream_slot()?.controller_object_slot().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController is missing its JavaScript object")
                .into()
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

    /// <https://streams.spec.whatwg.org/#rs-default-controller-close>
    pub(crate) fn close(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !crate::js::js_result_to_completion(self.can_close_or_enqueue(), context)? {
            let error: JsNativeError = JsNativeError::typ()
                .with_message("The stream is not in a state that permits close");
            return Err(crate::js::native_error_to_js_value(error, context));
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
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(this) is false, throw a TypeError exception."
        if !crate::js::js_result_to_completion(self.can_close_or_enqueue(), context)? {
            let error: JsNativeError = JsNativeError::typ()
                .with_message("The stream is not in a state that permits enqueue");
            return Err(crate::js::native_error_to_js_value(error, context));
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
            Some(cancel_algorithm) => {
                let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
                JsObject::from(
                    cancel_algorithm.call(reason, js_engine::boa::context_as_ec(context)),
                )
            }
            None => resolved_promise(JsValue::undefined(), ec)?,
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
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "Let stream be this.[[stream]]."
        let stream = crate::js::js_result_to_completion(self.stream_slot(), context)?;

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
                readable_stream_close(stream, context).map_err(|e| {
                    e.into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined())
                })?;
            } else {
                // Step 2.3: "Otherwise, perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
                self.call_pull_if_needed(js_engine::boa::context_as_ec(context))?;
            }

            // Step 2.4: "Perform readRequest's chunk steps, given chunk."
            return read_request.chunk_steps(chunk, ec);
        }

        // Step 3.1: "Perform ! ReadableStreamAddReadRequest(stream, readRequest)."
        crate::js::js_result_to_completion(
            readable_stream_add_read_request(stream.clone(), read_request),
            context,
        )?;

        // Step 3.2: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(this)."
        self.call_pull_if_needed(js_engine::boa::context_as_ec(context))
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
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "Let shouldPull be ! ReadableStreamDefaultControllerShouldCallPull(controller)."
        let should_pull = crate::js::js_result_to_completion(self.should_call_pull(), context)?;

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
        let controller_object =
            crate::js::js_result_to_completion(self.controller_object(), context)?;
        let pull_algorithm = self.pull_algorithm.borrow().clone();
        let pull_promise = match pull_algorithm {
            Some(pull_algorithm) => {
                pull_algorithm.call(&controller_object, js_engine::boa::context_as_ec(context))
            }
            None => promise_from_completion(
                Ok(JsValue::undefined()),
                js_engine::boa::context_as_ec(context),
            ),
        };

        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, controller: &ReadableStreamDefaultController, context| {
                // Step 7.1: "Set controller.[[pulling]] to false."
                controller.pulling.set(false);

                let should_pull_again = controller.pull_again.get();
                if should_pull_again {
                    // Step 7.2.1: "Set controller.[[pullAgain]] to false."
                    controller.pull_again.set(false);

                    // Step 7.2.2: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
                    crate::js::completion_to_js_result(
                        controller.call_pull_if_needed(js_engine::boa::context_as_ec(context)),
                    )?;
                }

                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, controller: &ReadableStreamDefaultController, context| {
                // Step 8.1: "Perform ! ReadableStreamDefaultControllerError(controller, e)."
                crate::js::completion_to_js_result(controller.error_steps(
                    args.get_or_undefined(0).clone(),
                    js_engine::boa::context_as_ec(context),
                ))?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let pull_reaction: JsObject = pull_promise
            .then(Some(on_fulfilled), Some(on_rejected), context)
            .map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })?
            .into();
        mark_promise_as_handled(&pull_reaction, js_engine::boa::context_as_ec(context))?;
        mark_promise_as_handled(
            &JsObject::from(pull_promise),
            js_engine::boa::context_as_ec(context),
        )?;
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

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-close>
    pub(crate) fn close_steps(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
        if !crate::js::js_result_to_completion(self.can_close_or_enqueue(), context)? {
            return Ok(());
        }

        // Step 2: "Let stream be controller.[[stream]]."
        let stream = crate::js::js_result_to_completion(self.stream_slot(), context)?;

        // Step 3: "Set controller.[[closeRequested]] to true."
        self.close_requested.set(true);

        // Step 4: "If controller.[[queue]] is empty,"
        if self.queue_is_empty() {
            // Step 4.1: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
            self.clear_algorithms();

            // Step 4.2: "Perform ! ReadableStreamClose(stream)."
            readable_stream_close(stream, context).map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-enqueue>
    pub(crate) fn enqueue_steps(
        &self,
        chunk: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "If ! ReadableStreamDefaultControllerCanCloseOrEnqueue(controller) is false, return."
        if !crate::js::js_result_to_completion(self.can_close_or_enqueue(), context)? {
            return Ok(());
        }

        // Step 2: "Let stream be controller.[[stream]]."
        let stream = crate::js::js_result_to_completion(self.stream_slot(), context)?;

        // Step 3: "If ! IsReadableStreamLocked(stream) is true and ! ReadableStreamGetNumReadRequests(stream) > 0, perform ! ReadableStreamFulfillReadRequest(stream, chunk, false)."
        if stream.is_readable_stream_locked()
            && readable_stream_get_num_read_requests(stream.clone()) > 0
        {
            readable_stream_fulfill_read_request(stream, chunk, false, context).map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })?;
        } else {
            // Step 4.1: "Let result be the result of performing controller.[[strategySizeAlgorithm]], passing in chunk, and interpreting the result as a completion record."
            let size_algorithm = crate::js::js_result_to_completion(
                self.strategy_size_algorithm
                    .borrow()
                    .clone()
                    .ok_or_else(|| {
                        JsNativeError::typ()
                            .with_message(
                                "ReadableStreamDefaultController is missing its size algorithm",
                            )
                            .into()
                    }),
                context,
            )?;
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
        let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
        // Step 1: "Let stream be controller.[[stream]]."
        let stream = crate::js::js_result_to_completion(self.stream_slot(), context)?;

        // Step 2: "If stream.[[state]] is not \"readable\", return."
        if stream.state() != ReadableStreamState::Readable {
            return Ok(());
        }

        // Step 3: "Perform ! ResetQueue(controller)."
        self.reset_queue();

        // Step 4: "Perform ! ReadableStreamDefaultControllerClearAlgorithms(controller)."
        self.clear_algorithms();

        // Step 5: "Perform ! ReadableStreamError(stream, e)."
        readable_stream_error(stream, error, context).map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })
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

    /// <https://streams.spec.whatwg.org/#readable-stream-default-controller-has-backpressure>
    pub(crate) fn has_backpressure(&self) -> JsResult<bool> {
        Ok(!self.should_call_pull()?)
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
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
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
    let start_result =
        start_algorithm.call(controller_object, js_engine::boa::context_as_ec(context))?;

    // Step 10: "Let startPromise be a promise resolved with startResult."
    let start_promise = JsPromise::resolve(start_result, context).map_err(|e| {
        e.into_opaque(context)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, controller: &ReadableStreamDefaultController, context| {
            // Step 11.1: "Set controller.[[started]] to true."
            controller.started.set(true);

            // Step 11.2: "Assert: controller.[[pulling]] is false."
            debug_assert!(!controller.pulling.get());

            // Step 11.3: "Assert: controller.[[pullAgain]] is false."
            debug_assert!(!controller.pull_again.get());

            // Step 11.4: "Perform ! ReadableStreamDefaultControllerCallPullIfNeeded(controller)."
            crate::js::completion_to_js_result(
                controller.call_pull_if_needed(js_engine::boa::context_as_ec(context)),
            )?;
            Ok(JsValue::undefined())
        },
        controller.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, controller: &ReadableStreamDefaultController, context| {
            // Step 12.1: "Perform ! ReadableStreamDefaultControllerError(controller, r)."
            crate::js::completion_to_js_result(controller.error_steps(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(context),
            ))?;
            Ok(JsValue::undefined())
        },
        controller,
    )
    .to_js_function(context.realm());
    let start_reaction: JsObject = start_promise
        .then(Some(on_fulfilled), Some(on_rejected), context)
        .map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?
        .into();
    mark_promise_as_handled(&start_reaction, js_engine::boa::context_as_ec(context))?;
    mark_promise_as_handled(
        &JsObject::from(start_promise),
        js_engine::boa::context_as_ec(context),
    )?;
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
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 1: "Let controller be a new ReadableStreamDefaultController."
    let controller = ReadableStreamDefaultController::new();
    let controller_object = create_interface_instance::<
        crate::js::Types,
        ReadableStreamDefaultController,
    >(controller.clone(), js_engine::boa::context_as_ec(context))?;

    // Step 2: "Let startAlgorithm be an algorithm that returns undefined."
    let mut start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 3: "Let pullAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut pull_algorithm = PullAlgorithm::ReturnUndefined;

    // Step 4: "Let cancelAlgorithm be an algorithm that returns a promise resolved with undefined."
    let mut cancel_algorithm = CancelAlgorithm::ReturnUndefined;

    // Step 5: "If underlyingSourceDict[\"start\"] exists, then set startAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"start\"] with argument list « controller » and callback this value underlyingSource."
    let ec_ref: &mut dyn ExecutionContext<crate::js::Types> =
        js_engine::boa::context_as_ec(context);
    if let Some(start_method) =
        extract_source_method(underlying_source_object.as_ref(), "start", ec_ref)?
    {
        start_algorithm = StartAlgorithm::JavaScript(start_method);
    }

    // Step 6: "If underlyingSourceDict[\"pull\"] exists, then set pullAlgorithm to an algorithm which returns the result of invoking underlyingSourceDict[\"pull\"] with argument list « controller » and callback this value underlyingSource."
    let ec_ref: &mut dyn ExecutionContext<crate::js::Types> =
        js_engine::boa::context_as_ec(context);
    if let Some(pull_method) =
        extract_source_method(underlying_source_object.as_ref(), "pull", ec_ref)?
    {
        pull_algorithm = PullAlgorithm::JavaScript(pull_method);
    }

    // Step 7: "If underlyingSourceDict[\"cancel\"] exists, then set cancelAlgorithm to an algorithm which takes an argument reason and returns the result of invoking underlyingSourceDict[\"cancel\"] with argument list « reason » and callback this value underlyingSource."
    let ec_ref: &mut dyn ExecutionContext<crate::js::Types> =
        js_engine::boa::context_as_ec(context);
    if let Some(cancel_method) =
        extract_source_method(underlying_source_object.as_ref(), "cancel", ec_ref)?
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
        js_engine::boa::context_as_ec(context),
    )
}
/// underlying source object as the callback this value required by the Streams setup algorithm.
pub(crate) fn extract_source_method(
    source_object: Option<&JsObject>,
    name: &str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<SourceMethod>, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let property_name = JsString::from(name);
    let value =
        crate::js::js_result_to_completion(source_object.get(property_name, context), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    let callback = value
        .as_object()
        .filter(|object| object.is_callable())
        .ok_or_else(|| {
            crate::js::native_error_to_js_value(
                JsNativeError::typ()
                    .with_message(format!("underlying source {name} must be callable")),
                context,
            )
        })?;

    Ok(Some(SourceMethod::new(
        source_object.clone(),
        crate::webidl::Callback::from_object(callback),
    )))
}
