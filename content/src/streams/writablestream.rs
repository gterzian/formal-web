use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::{Types, create_builtin_fn_with_traced_captures};

use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{resolved_promise, upon_settlement};
use js_engine::gc::{GcCell, JsObjectCell, JsValueCell, gc_cell_new};
use js_engine::gc_struct;

use super::{
    AbortAlgorithm, CloseAlgorithm, PendingAbortRequest, WritableStartAlgorithm,
    WritableStreamController, WritableStreamState, WritableStreamWriter, WriteAlgorithm,
    WriteRequest, acquire_writable_stream_default_writer,
    create_writable_stream_default_controller, rejected_type_error_promise,
    set_up_writable_stream_default_controller,
    set_up_writable_stream_default_controller_from_underlying_sink,
    writable_stream_default_controller_close,
};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://streams.spec.whatwg.org/#ws-class>
#[gc_struct]
pub struct WritableStream {
    /// <https://streams.spec.whatwg.org/#writablestream-controller>
    controller: GcCell<Option<WritableStreamController>>,
    controller_object: JsObjectCell,

    /// <https://streams.spec.whatwg.org/#writablestream-writer>
    writer: GcCell<Option<WritableStreamWriter>>,

    /// <https://streams.spec.whatwg.org/#writablestream-state>
    #[ignore_trace]
    state: Rc<RefCell<WritableStreamState>>,

    /// <https://streams.spec.whatwg.org/#writablestream-storederror>
    stored_error: JsValueCell,

    /// <https://streams.spec.whatwg.org/#writablestream-writerequests>
    write_requests: GcCell<Vec<WriteRequest>>,

    /// <https://streams.spec.whatwg.org/#writablestream-inflightwriterequest>
    in_flight_write_request: GcCell<Option<WriteRequest>>,

    /// <https://streams.spec.whatwg.org/#writablestream-closerequest>
    close_request: GcCell<Option<WriteRequest>>,

    /// <https://streams.spec.whatwg.org/#writablestream-inflightcloserequest>
    in_flight_close_request: GcCell<Option<WriteRequest>>,

    /// <https://streams.spec.whatwg.org/#writablestream-pendingabortrequest>
    pending_abort_request: GcCell<Option<PendingAbortRequest>>,

    /// <https://streams.spec.whatwg.org/#writablestream-backpressure>
    #[ignore_trace]
    backpressure: Rc<Cell<bool>>,
}

impl WritableStream {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        let undefined = ec.value_undefined();
        Self {
            controller: gc_cell_new(None),
            controller_object: JsObjectCell::new(None),
            writer: gc_cell_new(None),
            state: Rc::new(RefCell::new(WritableStreamState::Writable)),
            stored_error: JsValueCell::new(undefined),
            write_requests: gc_cell_new(Vec::new()),
            in_flight_write_request: gc_cell_new(None),
            close_request: gc_cell_new(None),
            in_flight_close_request: gc_cell_new(None),
            pending_abort_request: gc_cell_new(None),
            backpressure: Rc::new(Cell::new(false)),
        }
    }
    pub(crate) fn controller_slot(&self) -> Option<WritableStreamController> {
        self.controller.borrow().clone()
    }
    pub(crate) fn set_controller_slot(&self, controller: Option<WritableStreamController>) {
        *self.controller.borrow_mut() = controller;
    }
    pub(crate) fn controller_object_slot(&self) -> Option<JsObject> {
        self.controller_object.borrow().clone()
    }
    pub(crate) fn set_controller_object_slot(&self, controller_object: Option<JsObject>) {
        self.controller_object.set(controller_object);
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
        self.stored_error.set(error);
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

    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    /// <https://streams.spec.whatwg.org/#initialize-writable-stream>
    fn initialize_writable_stream(&mut self, ec: &mut dyn ExecutionContext<Types>) {
        *self.state.borrow_mut() = WritableStreamState::Writable;
        self.stored_error.set(ec.value_undefined());
        *self.writer.borrow_mut() = None;
        *self.controller.borrow_mut() = None;
        self.controller_object.set(None);
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
    pub(crate) fn abort(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        if self.is_writable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot abort a WritableStream that already has a writer",
                ec,
            );
        }

        self.abort_stream(reason, ec)
    }

    /// <https://streams.spec.whatwg.org/#ws-close>
    pub(crate) fn close(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        if self.is_writable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that already has a writer",
                ec,
            );
        }

        if self.close_queued_or_in_flight() {
            return rejected_type_error_promise(
                "Cannot close a WritableStream that is already closing",
                ec,
            );
        }

        self.close_stream(ec)
    }

    /// <https://streams.spec.whatwg.org/#ws-get-writer>
    pub(crate) fn get_writer(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        acquire_writable_stream_default_writer(self.clone(), ec)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-abort>
    pub(crate) fn abort_stream(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        if matches!(
            self.state(),
            WritableStreamState::Closed | WritableStreamState::Errored
        ) {
            return resolved_promise(ec.value_undefined(), ec);
        }

        let controller = self
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
        controller.signal_abort(reason.clone(), ec)?;

        if matches!(
            self.state(),
            WritableStreamState::Closed | WritableStreamState::Errored
        ) {
            return resolved_promise(ec.value_undefined(), ec);
        }

        if let Some(abort_request) = self.pending_abort_request_slot() {
            return Ok(abort_request.promise());
        }

        let mut was_already_erroring = false;
        let mut abort_reason = reason;
        if self.state() == WritableStreamState::Erroring {
            was_already_erroring = true;
            abort_reason = ec.value_undefined();
        }

        let abort_request =
            PendingAbortRequest::new(abort_reason.clone(), was_already_erroring, ec)?;
        let promise = abort_request.promise();
        self.set_pending_abort_request_slot(Some(abort_request));

        if !was_already_erroring {
            self.start_erroring(abort_reason, ec)?;
        }

        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-close>
    pub(crate) fn close_stream(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        match self.state() {
            WritableStreamState::Closed | WritableStreamState::Errored => {
                return rejected_type_error_promise(
                    "Cannot close a WritableStream that is already closed or errored",
                    ec,
                );
            }
            _ => {}
        }

        debug_assert!(!self.close_queued_or_in_flight());

        let (close_request, promise) = WriteRequest::new(ec)?;
        self.set_close_request_slot(Some(close_request));

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                if self.backpressure() && self.state() == WritableStreamState::Writable {
                    writer.resolve_ready_promise(ec)?;
                }
            }
        }

        let controller = self
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
        writable_stream_default_controller_close(controller.as_default_controller(), ec)?;
        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-add-write-request>
    pub(crate) fn add_write_request(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        debug_assert!(self.is_writable_stream_locked());
        debug_assert_eq!(self.state(), WritableStreamState::Writable);

        let (write_request, promise) = WriteRequest::new(ec)?;
        self.push_write_request(write_request);
        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-close-queued-or-in-flight>
    pub(crate) fn close_queued_or_in_flight(&self) -> bool {
        self.close_request_slot().is_some() || self.in_flight_close_request_slot().is_some()
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-deal-with-rejection>
    pub(crate) fn deal_with_rejection(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if self.state() == WritableStreamState::Writable {
            self.start_erroring(error, ec)?;
            return Ok(());
        }

        debug_assert_eq!(self.state(), WritableStreamState::Erroring);
        self.finish_erroring(ec)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-erroring>
    pub(crate) fn finish_erroring(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert_eq!(self.state(), WritableStreamState::Erroring);
        debug_assert!(!self.has_operation_marked_in_flight());

        self.set_state(WritableStreamState::Errored);
        let controller = self
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
        controller.error_steps();

        let stored_error = self.stored_error();
        for write_request in self.take_write_requests().into_iter() {
            write_request.reject(stored_error.clone(), ec)?;
        }

        let Some(abort_request) = self.take_pending_abort_request_slot() else {
            self.reject_close_and_closed_promise_if_needed(ec)?;
            return Ok(());
        };

        if abort_request.was_already_erroring() {
            abort_request.reject(stored_error.clone(), ec)?;
            self.reject_close_and_closed_promise_if_needed(ec)?;
            return Ok(());
        }

        let promise = controller.abort_steps(abort_request.reason(), ec)?;
        let abort_request_for_fulfilled = abort_request.clone();
        let stream_for_fulfilled = self.clone();
        let stream_for_rejected = self.clone();

        let name_key = ec.property_key_from_str("");
        let on_fulfilled = create_builtin_fn_with_traced_captures(
            ec,
            (abort_request_for_fulfilled, stream_for_fulfilled),
            writable_stream_finish_erroring_on_fulfilled_fn,
            1,
            name_key.clone(),
            false,
        );
        let on_rejected = create_builtin_fn_with_traced_captures(
            ec,
            (abort_request, stream_for_rejected),
            writable_stream_finish_erroring_on_rejected_fn,
            1,
            name_key,
            false,
        );
        let _ = upon_settlement(promise, Some(on_fulfilled), Some(on_rejected), ec)?;
        Ok(())
    }
}

/// Handler for `finish_erroring` on_fulfilled closure.
/// Resolves the abort request and rejects close/closed promise if needed.
fn writable_stream_finish_erroring_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    captures: &(PendingAbortRequest, WritableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (abort_request, stream) = captures;
    abort_request.clone().resolve(ec)?;
    stream.reject_close_and_closed_promise_if_needed(ec)?;
    Ok(ec.value_undefined())
}

/// Handler for `finish_erroring` on_rejected closure.
/// Rejects the abort request with the reason and rejects close/closed promise if needed.
fn writable_stream_finish_erroring_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(PendingAbortRequest, WritableStream),
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (abort_request, stream) = captures;
    let reason = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined());
    abort_request.clone().reject(reason, ec)?;
    stream.reject_close_and_closed_promise_if_needed(ec)?;
    Ok(ec.value_undefined())
}

impl WritableStream {
    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-close>
    pub(crate) fn finish_in_flight_close(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let close_request = self.take_in_flight_close_request_slot().ok_or_else(|| {
            ec.new_type_error("WritableStream is missing its in-flight close request")
        })?;
        close_request.resolve(ec)?;

        let state = self.state();
        debug_assert!(
            state == WritableStreamState::Writable || state == WritableStreamState::Erroring
        );
        if state == WritableStreamState::Erroring {
            self.set_stored_error(ec.value_undefined());
            if let Some(abort_request) = self.take_pending_abort_request_slot() {
                abort_request.resolve(ec)?;
            }
        }

        self.set_state(WritableStreamState::Closed);
        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.resolve_closed_promise(ec)?;
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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let close_request = self.take_in_flight_close_request_slot().ok_or_else(|| {
            ec.new_type_error("WritableStream is missing its in-flight close request")
        })?;
        close_request.reject(error.clone(), ec)?;

        if let Some(abort_request) = self.take_pending_abort_request_slot() {
            abort_request.reject(error.clone(), ec)?;
        }

        self.deal_with_rejection(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-write>
    pub(crate) fn finish_in_flight_write(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let write_request = self.take_in_flight_write_request_slot().ok_or_else(|| {
            ec.new_type_error("WritableStream is missing its in-flight write request")
        })?;
        write_request.resolve(ec)?;
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-finish-in-flight-write-with-error>
    pub(crate) fn finish_in_flight_write_with_error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let write_request = self.take_in_flight_write_request_slot().ok_or_else(|| {
            ec.new_type_error("WritableStream is missing its in-flight write request")
        })?;
        write_request.reject(error.clone(), ec)?;
        self.deal_with_rejection(error, ec)
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-has-operation-marked-in-flight>
    pub(crate) fn has_operation_marked_in_flight(&self) -> bool {
        self.in_flight_write_request_slot().is_some()
            || self.in_flight_close_request_slot().is_some()
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-mark-close-request-in-flight>
    pub(crate) fn mark_close_request_in_flight(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert!(self.in_flight_close_request_slot().is_none());
        let close_request = self
            .take_close_request_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its close request"))?;
        self.set_in_flight_close_request_slot(Some(close_request));
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-mark-first-write-request-in-flight>
    pub(crate) fn mark_first_write_request_in_flight(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert!(self.in_flight_write_request_slot().is_none());
        let write_request = self
            .shift_write_request()
            .ok_or_else(|| ec.new_type_error("WritableStream has no pending write request"))?;
        self.set_in_flight_write_request_slot(Some(write_request));
        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-reject-close-and-closed-promise-if-needed>
    pub(crate) fn reject_close_and_closed_promise_if_needed(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert_eq!(self.state(), WritableStreamState::Errored);

        if let Some(close_request) = self.take_close_request_slot() {
            debug_assert!(self.in_flight_close_request_slot().is_none());
            close_request.reject(self.stored_error(), ec)?;
        }

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.ensure_closed_promise_rejected(self.stored_error(), ec)?;
            }
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-start-erroring>
    pub(crate) fn start_erroring(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert!(self.stored_error().is_undefined());
        debug_assert_eq!(self.state(), WritableStreamState::Writable);

        let controller = self
            .controller_slot()
            .ok_or_else(|| ec.new_type_error("WritableStream is missing its controller"))?;
        self.set_state(WritableStreamState::Erroring);
        self.set_stored_error(reason.clone());

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                writer.ensure_ready_promise_rejected(reason, ec)?;
            }
        }

        if !self.has_operation_marked_in_flight() && controller.as_default_controller().started() {
            self.finish_erroring(ec)?;
        }

        Ok(())
    }

    /// <https://streams.spec.whatwg.org/#writable-stream-update-backpressure>
    pub(crate) fn update_backpressure(
        &self,
        backpressure: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        debug_assert_eq!(self.state(), WritableStreamState::Writable);
        debug_assert!(!self.close_queued_or_in_flight());

        if let Some(writer_slot) = self.writer_slot() {
            if let Some(writer) = writer_slot.as_default_writer() {
                if backpressure != self.backpressure() {
                    if backpressure {
                        writer.reset_ready_promise(ec)?;
                    } else {
                        writer.resolve_ready_promise(ec)?;
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<WritableStream, Types> {
    let mut stream = WritableStream::new(ec);

    let underlying_sink = if args.is_empty() {
        ec.value_null()
    } else {
        args[0].clone()
    };
    let strategy = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());

    let size_algorithm = extract_size_algorithm(&strategy, ec)?;
    let high_water_mark = extract_high_water_mark(&strategy, 1.0, ec)?;

    let underlying_sink_object =
        if underlying_sink.is_null() || underlying_sink.is_undefined() {
            None
        } else {
            Some(underlying_sink.as_object().ok_or_else(|| {
                ec.new_type_error("WritableStream underlyingSink must be an object")
            })?)
        };

    if let Some(sink_type) = underlying_sink_type(underlying_sink_object.as_ref(), ec)? {
        return Err(ec.new_range_error(&format!(
            "WritableStream underlyingSink.type must be undefined, got {sink_type}"
        )));
    }

    // Step 5: "Perform ! InitializeWritableStream(this)."
    // Note: The backing struct is returned from the data constructor, after which Boa wraps it
    // in the newly created JsObject.

    stream.initialize_writable_stream(ec);

    // Step 6: "Perform ? SetUpWritableStreamDefaultControllerFromUnderlyingSink(this, underlyingSink, underlyingSinkDict, highWaterMark, sizeAlgorithm)."

    set_up_writable_stream_default_controller_from_underlying_sink(
        stream.clone(),
        underlying_sink_object,
        high_water_mark,
        size_algorithm,
        ec,
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(WritableStream, JsObject), Types> {
    let high_water_mark = high_water_mark.unwrap_or(1.0);
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    let (mut stream, stream_object) = create_writable_stream_object(ec)?;
    stream.initialize_writable_stream(ec);
    let (controller, controller_object) = create_writable_stream_default_controller(ec)?;
    set_up_writable_stream_default_controller(
        stream.clone(),
        controller,
        &controller_object,
        start_algorithm,
        write_algorithm,
        close_algorithm,
        abort_algorithm,
        high_water_mark,
        size_algorithm,
        ec,
    )?;
    Ok((stream, stream_object))
}
fn create_writable_stream_object(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(WritableStream, JsObject), Types> {
    let mut stream = WritableStream::new(ec);
    stream.initialize_writable_stream(ec);
    let stream_object: JsObject =
        create_interface_instance::<Types, WritableStream>(stream.clone(), ec)?.into();
    Ok((stream, stream_object))
}

pub(crate) fn with_writable_stream_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&WritableStream) -> R,
) -> Completion<R, Types> {
    let stream_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<WritableStream>());
    let stream = match stream_ref {
        Some(s) => s,
        None => return Err(ec.new_type_error("object is not a WritableStream")),
    };
    Ok(f(stream))
}

fn underlying_sink_type(
    underlying_sink: Option<&JsObject>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    let Some(underlying_sink) = underlying_sink else {
        return Ok(None);
    };

    use js_engine::EcmascriptHost;
    let type_key = ec.property_key_from_str("type");
    if !ec.has_property(underlying_sink.clone(), type_key)? {
        return Ok(None);
    }

    let sink_type = EcmascriptHost::get(ec, underlying_sink, "type")?;
    let undefined_value = ec.value_undefined();
    if ec.same_value(&sink_type, &undefined_value) {
        return Ok(None);
    }

    Ok(Some(ec.to_rust_string(sink_type)?))
}
