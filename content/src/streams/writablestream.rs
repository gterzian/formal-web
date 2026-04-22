use std::{cell::{Cell, RefCell}, rc::Rc};

use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    class::Class,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::resolved_promise;

use super::{
    AbortAlgorithm, CloseAlgorithm, PendingAbortRequest, WritableStartAlgorithm, WriteAlgorithm,
    WritableStreamController, WritableStreamState, WritableStreamWriter, WriteRequest,
    acquire_writable_stream_default_writer, create_writable_stream_default_controller,
    rejected_type_error_promise, set_up_writable_stream_default_controller,
    set_up_writable_stream_default_controller_from_underlying_sink,
    writable_stream_default_controller_close,
};

/// <https://streams.spec.whatwg.org/#ws-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct WritableStream {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-controller>
    controller: Gc<GcRefCell<Option<WritableStreamController>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-writer>
    writer: Gc<GcRefCell<Option<WritableStreamWriter>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-state>
    #[unsafe_ignore_trace]
    state: Rc<RefCell<WritableStreamState>>,

    /// <https://streams.spec.whatwg.org/#writablestream-storederror>
    stored_error: Gc<GcRefCell<JsValue>>,

    /// <https://streams.spec.whatwg.org/#writablestream-writerequests>
    write_requests: Gc<GcRefCell<Vec<WriteRequest>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-inflightwriterequest>
    in_flight_write_request: Gc<GcRefCell<Option<WriteRequest>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-closerequest>
    close_request: Gc<GcRefCell<Option<WriteRequest>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-inflightcloserequest>
    in_flight_close_request: Gc<GcRefCell<Option<WriteRequest>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-pendingabortrequest>
    pending_abort_request: Gc<GcRefCell<Option<PendingAbortRequest>>>,

    /// <https://streams.spec.whatwg.org/#writablestream-backpressure>
    #[unsafe_ignore_trace]
    backpressure: Rc<Cell<bool>>,
}

impl WritableStream {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            controller: Gc::new(GcRefCell::new(None)),
            writer: Gc::new(GcRefCell::new(None)),
            state: Rc::new(RefCell::new(WritableStreamState::Writable)),
            stored_error: Gc::new(GcRefCell::new(JsValue::undefined())),
            write_requests: Gc::new(GcRefCell::new(Vec::new())),
            in_flight_write_request: Gc::new(GcRefCell::new(None)),
            close_request: Gc::new(GcRefCell::new(None)),
            in_flight_close_request: Gc::new(GcRefCell::new(None)),
            pending_abort_request: Gc::new(GcRefCell::new(None)),
            backpressure: Rc::new(Cell::new(false)),
        }
    }
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("WritableStream is missing its JavaScript object")
                .into()
        })
    }
    pub(crate) fn controller_slot(&self) -> Option<WritableStreamController> {
        self.controller.borrow().clone()
    }
    pub(crate) fn set_controller_slot(&self, controller: Option<WritableStreamController>) {
        *self.controller.borrow_mut() = controller;
    }
    pub(crate) fn writer_slot(&self) -> Option<WritableStreamWriter> {
        self.writer.borrow().clone()
    }
    pub(crate) fn set_writer_slot(&self, writer: Option<WritableStreamWriter>) {
        *self.writer.borrow_mut() = writer;
    }
    pub(crate) fn state(&self) -> WritableStreamState {
        self.state.borrow().clone()
    }
    pub(crate) fn set_state(&self, state: WritableStreamState) {
        *self.state.borrow_mut() = state;
    }
    pub(crate) fn stored_error(&self) -> JsValue {
        self.stored_error.borrow().clone()
    }
    pub(crate) fn set_stored_error(&self, error: JsValue) {
        *self.stored_error.borrow_mut() = error;
    }
    pub(crate) fn backpressure(&self) -> bool {
        self.backpressure.get()
    }
    pub(crate) fn set_backpressure(&self, backpressure: bool) {
        self.backpressure.set(backpressure);
    }
    pub(crate) fn close_request_slot(&self) -> Option<WriteRequest> {
        self.close_request.borrow().clone()
    }
    pub(crate) fn set_close_request_slot(&self, request: Option<WriteRequest>) {
        *self.close_request.borrow_mut() = request;
    }
    pub(crate) fn take_close_request_slot(&self) -> Option<WriteRequest> {
        self.close_request.borrow_mut().take()
    }
    pub(crate) fn in_flight_write_request_slot(&self) -> Option<WriteRequest> {
        self.in_flight_write_request.borrow().clone()
    }
    pub(crate) fn set_in_flight_write_request_slot(&self, request: Option<WriteRequest>) {
        *self.in_flight_write_request.borrow_mut() = request;
    }
    pub(crate) fn take_in_flight_write_request_slot(&self) -> Option<WriteRequest> {
        self.in_flight_write_request.borrow_mut().take()
    }
    pub(crate) fn in_flight_close_request_slot(&self) -> Option<WriteRequest> {
        self.in_flight_close_request.borrow().clone()
    }
    pub(crate) fn set_in_flight_close_request_slot(&self, request: Option<WriteRequest>) {
        *self.in_flight_close_request.borrow_mut() = request;
    }
    pub(crate) fn take_in_flight_close_request_slot(&self) -> Option<WriteRequest> {
        self.in_flight_close_request.borrow_mut().take()
    }
    pub(crate) fn pending_abort_request_slot(&self) -> Option<PendingAbortRequest> {
        self.pending_abort_request.borrow().clone()
    }
    pub(crate) fn set_pending_abort_request_slot(&self, request: Option<PendingAbortRequest>) {
        *self.pending_abort_request.borrow_mut() = request;
    }
    pub(crate) fn take_pending_abort_request_slot(&self) -> Option<PendingAbortRequest> {
        self.pending_abort_request.borrow_mut().take()
    }
    pub(crate) fn push_write_request(&self, request: WriteRequest) {
        self.write_requests.borrow_mut().push(request);
    }
    pub(crate) fn shift_write_request(&self) -> Option<WriteRequest> {
        let mut write_requests = self.write_requests.borrow_mut();
        if write_requests.is_empty() {
            None
        } else {
            Some(write_requests.remove(0))
        }
    }
    pub(crate) fn take_write_requests(&self) -> Vec<WriteRequest> {
        std::mem::take(&mut self.write_requests.borrow_mut())
    }

    /// <https://streams.spec.whatwg.org/#initialize-writable-stream>
    fn initialize_writable_stream(&mut self) {
        *self.state.borrow_mut() = WritableStreamState::Writable;
        *self.stored_error.borrow_mut() = JsValue::undefined();
        *self.writer.borrow_mut() = None;
        *self.controller.borrow_mut() = None;
        *self.in_flight_write_request.borrow_mut() = None;
        *self.close_request.borrow_mut() = None;
        *self.in_flight_close_request.borrow_mut() = None;
        *self.pending_abort_request.borrow_mut() = None;
        self.write_requests.borrow_mut().clear();
        self.backpressure.set(false);
    }

    /// <https://streams.spec.whatwg.org/#is-writable-stream-locked>
    pub(crate) fn is_writable_stream_locked(&self) -> bool {
        self.writer_slot().is_some()
    }

    /// <https://streams.spec.whatwg.org/#ws-locked>
    pub(crate) fn locked(&self) -> bool {
        self.is_writable_stream_locked()
    }

    /// <https://streams.spec.whatwg.org/#ws-abort>
    pub(crate) fn abort(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        if self.is_writable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot abort a WritableStream that already has a writer",
                context,
            );
        }

        self.abort_stream(reason, context)
    }

    /// <https://streams.spec.whatwg.org/#ws-close>
    pub(crate) fn close(&self, context: &mut Context) -> JsResult<JsObject> {
        if self.is_writable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that already has a writer",
                context,
            );
        }

        if self.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that is already closing",
                context,
            );
        }

        self.close_stream(context)
    }

    /// <https://streams.spec.whatwg.org/#ws-get-writer>
    pub(crate) fn get_writer(&self, context: &mut Context) -> JsResult<JsObject> {
        acquire_writable_stream_default_writer(self.clone(), context)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-abort>
    pub(crate) fn abort_stream(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        if matches!(self.state(), WritableStreamState::Closed | WritableStreamState::Errored) {
            return resolved_promise(JsValue::undefined(), context);
        }

        let controller = self.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its controller")
        })?;
        controller.signal_abort(reason.clone(), context)?;

        if matches!(self.state(), WritableStreamState::Closed | WritableStreamState::Errored) {
            return resolved_promise(JsValue::undefined(), context);
        }

        if let Some(abort_request) = self.pending_abort_request_slot() {
            return Ok(abort_request.promise());
        }

        let mut was_already_erroring = false;
        let mut abort_reason = reason;
        if self.state() == WritableStreamState::Erroring {
            was_already_erroring = true;
            abort_reason = JsValue::undefined();
        }

        let abort_request =
            PendingAbortRequest::new(abort_reason.clone(), was_already_erroring, context);
        let promise = abort_request.promise();
        self.set_pending_abort_request_slot(Some(abort_request));

        if !was_already_erroring {
            self.start_erroring(abort_reason, context)?;
        }

        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-close>
    pub(crate) fn close_stream(&self, context: &mut Context) -> JsResult<JsObject> {
        match self.state() {
            WritableStreamState::Closed | WritableStreamState::Errored => {
                return rejected_type_error_promise(
                    "Cannot close a WritableStream that is already closed or errored",
                    context,
                );
            }
            _ => {}
        }

        debug_assert!(!self.close_queued_or_in_flight());

        let (close_request, promise) = WriteRequest::new(context);
        self.set_close_request_slot(Some(close_request));

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                if self.backpressure() && self.state() == WritableStreamState::Writable {
                    writer.resolve_ready_promise(context)?;
                }
            }
        }

        let controller = self.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its controller")
        })?;
        writable_stream_default_controller_close(controller.as_default_controller(), context)?;
        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-add-write-request>
    pub(crate) fn add_write_request(&self, context: &mut Context) -> JsResult<JsObject> {
        debug_assert!(self.is_writable_stream_locked());
        debug_assert_eq!(self.state(), WritableStreamState::Writable);

        let (write_request, promise) = WriteRequest::new(context);
        self.push_write_request(write_request);
        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-close-queued-or-in-flight>
    pub(crate) fn close_queued_or_in_flight(&self) -> bool {
        self.close_request_slot().is_some() || self.in_flight_close_request_slot().is_some()
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-deal-with-rejection>
    pub(crate) fn deal_with_rejection(&self, error: JsValue, context: &mut Context) -> JsResult<()> {
        if self.state() == WritableStreamState::Writable {
            self.start_erroring(error, context)?;
            return Ok(());
        }

        debug_assert_eq!(self.state(), WritableStreamState::Erroring);
        self.finish_erroring(context)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-erroring>
    pub(crate) fn finish_erroring(&self, context: &mut Context) -> JsResult<()> {
        debug_assert_eq!(self.state(), WritableStreamState::Erroring);
        debug_assert!(!self.has_operation_marked_in_flight());

        self.set_state(WritableStreamState::Errored);
        let controller = self.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its controller")
        })?;
        controller.error_steps();

        let stored_error = self.stored_error();
        for write_request in self.take_write_requests().into_iter() {
            write_request.reject(stored_error.clone(), context)?;
        }

        let Some(abort_request) = self.take_pending_abort_request_slot() else {
            self.reject_close_and_closed_promise_if_needed(context)?;
            return Ok(());
        };

        if abort_request.was_already_erroring() {
            abort_request.reject(stored_error.clone(), context)?;
            self.reject_close_and_closed_promise_if_needed(context)?;
            return Ok(());
        }

        let promise = controller.abort_steps(abort_request.reason(), context)?;
        let abort_request_for_fulfilled = abort_request.clone();
        let stream_for_fulfilled = self.clone();
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, captures: &(PendingAbortRequest, WritableStream), context| {
                let (abort_request, stream) = captures;
                abort_request.resolve(context)?;
                stream.reject_close_and_closed_promise_if_needed(context)?;
                Ok(JsValue::undefined())
            },
            (abort_request_for_fulfilled, stream_for_fulfilled),
        )
        .to_js_function(context.realm());
        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args: &[JsValue], captures: &(PendingAbortRequest, WritableStream), context| {
                let (abort_request, stream) = captures;
                abort_request.reject(args.get_or_undefined(0).clone(), context)?;
                stream.reject_close_and_closed_promise_if_needed(context)?;
                Ok(JsValue::undefined())
            },
            (abort_request, self.clone()),
        )
        .to_js_function(context.realm());
        let _ =
            JsPromise::from_object(promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-close>
    pub(crate) fn finish_in_flight_close(&self, context: &mut Context) -> JsResult<()> {
        let close_request = self.take_in_flight_close_request_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its in-flight close request")
        })?;
        close_request.resolve(context)?;

        let state = self.state();
        debug_assert!(
            state == WritableStreamState::Writable || state == WritableStreamState::Erroring
        );
        if state == WritableStreamState::Erroring {
            self.set_stored_error(JsValue::undefined());
            if let Some(abort_request) = self.take_pending_abort_request_slot() {
                abort_request.resolve(context)?;
            }
        }

        self.set_state(WritableStreamState::Closed);
        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.resolve_closed_promise(context)?;
            }
        }

        debug_assert!(self.pending_abort_request_slot().is_none());
        debug_assert!(self.stored_error().is_undefined());
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-close-with-error>
    pub(crate) fn finish_in_flight_close_with_error(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        let close_request = self.take_in_flight_close_request_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its in-flight close request")
        })?;
        close_request.reject(error.clone(), context)?;

        if let Some(abort_request) = self.take_pending_abort_request_slot() {
            abort_request.reject(error.clone(), context)?;
        }

        self.deal_with_rejection(error, context)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-write>
    pub(crate) fn finish_in_flight_write(&self, context: &mut Context) -> JsResult<()> {
        let write_request = self.take_in_flight_write_request_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its in-flight write request")
        })?;
        write_request.resolve(context)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-write-with-error>
    pub(crate) fn finish_in_flight_write_with_error(
        &self,
        error: JsValue,
        context: &mut Context,
    ) -> JsResult<()> {
        let write_request = self.take_in_flight_write_request_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its in-flight write request")
        })?;
        write_request.reject(error.clone(), context)?;
        self.deal_with_rejection(error, context)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-has-operation-marked-in-flight>
    pub(crate) fn has_operation_marked_in_flight(&self) -> bool {
        self.in_flight_write_request_slot().is_some() || self.in_flight_close_request_slot().is_some()
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-mark-close-request-in-flight>
    pub(crate) fn mark_close_request_in_flight(&self) -> JsResult<()> {
        debug_assert!(self.in_flight_close_request_slot().is_none());
        let close_request = self.take_close_request_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its close request")
        })?;
        self.set_in_flight_close_request_slot(Some(close_request));
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-mark-first-write-request-in-flight>
    pub(crate) fn mark_first_write_request_in_flight(&self) -> JsResult<()> {
        debug_assert!(self.in_flight_write_request_slot().is_none());
        let write_request = self.shift_write_request().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream has no pending write request")
        })?;
        self.set_in_flight_write_request_slot(Some(write_request));
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-reject-close-and-closed-promise-if-needed>
    pub(crate) fn reject_close_and_closed_promise_if_needed(
        &self,
        context: &mut Context,
    ) -> JsResult<()> {
        debug_assert_eq!(self.state(), WritableStreamState::Errored);

        if let Some(close_request) = self.take_close_request_slot() {
            debug_assert!(self.in_flight_close_request_slot().is_none());
            close_request.reject(self.stored_error(), context)?;
        }

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.ensure_closed_promise_rejected(self.stored_error(), context)?;
            }
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-start-erroring>
    pub(crate) fn start_erroring(&self, reason: JsValue, context: &mut Context) -> JsResult<()> {
        debug_assert!(self.stored_error().is_undefined());
        debug_assert_eq!(self.state(), WritableStreamState::Writable);

        let controller = self.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream is missing its controller")
        })?;
        self.set_state(WritableStreamState::Erroring);
        self.set_stored_error(reason.clone());

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.ensure_ready_promise_rejected(reason, context)?;
            }
        }

        if !self.has_operation_marked_in_flight() && controller.as_default_controller().started() {
            self.finish_erroring(context)?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-update-backpressure>
    pub(crate) fn update_backpressure(
        &self,
        backpressure: bool,
        context: &mut Context,
    ) -> JsResult<()> {
        debug_assert_eq!(self.state(), WritableStreamState::Writable);
        debug_assert!(!self.close_queued_or_in_flight());

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                if backpressure != self.backpressure() {
                    if backpressure {
                        writer.reset_ready_promise(context)?;
                    } else {
                        writer.resolve_ready_promise(context)?;
                    }
                }
            }
        }

        self.set_backpressure(backpressure);
        Ok(())
    }
}
/// <https://streams.spec.whatwg.org/#ws-constructor>
pub(crate) fn construct_writable_stream(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<WritableStream> {
    let mut stream = WritableStream::new(None);

    let underlying_sink = if args.is_empty() {
        JsValue::null()
    } else {
        args[0].clone()
    };
    let strategy = args.get_or_undefined(1).clone();

    let size_algorithm = extract_size_algorithm(&strategy, context)?;
    let high_water_mark = extract_high_water_mark(&strategy, 1.0, context)?;

    let underlying_sink_object = if underlying_sink.is_null() || underlying_sink.is_undefined() {
        None
    } else {
        Some(underlying_sink.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("WritableStream underlyingSink must be an object")
        })?)
    };

    if let Some(sink_type) = underlying_sink_type(underlying_sink_object.as_ref(), context)? {
        return Err(JsNativeError::range()
            .with_message(format!(
                "WritableStream underlyingSink.type must be undefined, got {sink_type}"
            ))
            .into());
    }

    // Step 5: "Perform ! InitializeWritableStream(this)."
    // Note: `data_constructor` initializes the native carrier before Boa allocates the wrapping
    // object, so `object_constructor` stores the wrapper after construction.
    stream.initialize_writable_stream();

    // Step 6: "Perform ? SetUpWritableStreamDefaultControllerFromUnderlyingSink(this, underlyingSink, underlyingSinkDict, highWaterMark, sizeAlgorithm)."
    set_up_writable_stream_default_controller_from_underlying_sink(
        stream.clone(),
        underlying_sink_object,
        high_water_mark,
        size_algorithm,
        context,
    )?;
    Ok(stream)
}

/// <https://streams.spec.whatwg.org/#acquire-writable-stream-default-writer>
pub(crate) fn create_writable_stream(
    start_algorithm: WritableStartAlgorithm,
    write_algorithm: WriteAlgorithm,
    close_algorithm: CloseAlgorithm,
    abort_algorithm: AbortAlgorithm,
    high_water_mark: Option<f64>,
    size_algorithm: Option<SizeAlgorithm>,
    context: &mut Context,
) -> JsResult<WritableStream> {
    let high_water_mark = high_water_mark.unwrap_or(1.0);
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    let mut stream = create_writable_stream_object(context)?;
    stream.initialize_writable_stream();
    let controller = create_writable_stream_default_controller(context)?;
    set_up_writable_stream_default_controller(
        stream.clone(),
        controller,
        start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        high_water_mark,
        size_algorithm,
        context,
    )?;
    Ok(stream)
}
fn create_writable_stream_object(context: &mut Context) -> JsResult<WritableStream> {
    let stream = WritableStream::new(None);
    let stream_object = WritableStream::from_data(stream.clone(), context)?;
    stream.set_reflector(stream_object);
    Ok(stream)
}

pub(crate) fn with_writable_stream_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&WritableStream) -> R,
) -> JsResult<R> {
    let stream = object
        .downcast_ref::<WritableStream>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a WritableStream"))?;
    Ok(f(&stream))
}

fn underlying_sink_type(
    underlying_sink: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<String>> {
    let Some(underlying_sink) = underlying_sink else {
        return Ok(None);
    };

    if !underlying_sink.has_property(js_string!("type"), context)? {
        return Ok(None);
    }

    let sink_type = underlying_sink.get(js_string!("type"), context)?;
    if sink_type.is_undefined() {
        return Ok(None);
    }

    Ok(Some(sink_type.to_string(context)?.to_std_string_escaped()))
}
