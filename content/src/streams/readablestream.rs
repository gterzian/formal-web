use log::error;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    js_string,
    native_function::NativeFunction,
    object::{
        JsObject,
        builtins::{JsArray, JsFunction, JsPromise},
    },
    symbol::JsSymbol,
};
use boa_gc::{Finalize, Gc, GcRef, GcRefMut, Trace};

use crate::dom::{AbortAlgorithm as SignalAbortAlgorithm, AbortSignal};
use crate::js::with_abort_signal_ref;
use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{
    error_to_rejection_reason, mark_promise_as_handled, promise_from_completion,
    promise_from_value, rejected_promise, rejected_promise_from_error, resolved_promise,
    transform_promise_to_undefined,
};
use js_engine::EcmascriptHost;
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;
use js_engine::records::PromiseResolvers;
use js_engine::types::JsTypes;

use super::{
    ArrayBufferViewDescriptor, CancelAlgorithm, PullAlgorithm, ReadIntoRequest, ReadRequest,
    ReadableByteStreamController, ReadableStreamController, ReadableStreamDefaultReader,
    ReadableStreamGenericReader, ReadableStreamReader, ReadableStreamState, StartAlgorithm,
    acquire_readable_stream_byob_reader, acquire_readable_stream_default_reader,
    queue_internal_stream_microtask, readable_stream_byob_reader_release,
    readable_stream_default_reader_error_read_requests, readable_stream_default_reader_release,
    rejected_type_error_promise, set_up_readable_byte_stream_controller_from_underlying_source,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source, type_error_value,
    with_readable_stream_byob_reader_ref, with_readable_stream_byob_reader_ref_ec,
    with_readable_stream_default_reader_ref, with_readable_stream_default_reader_ref_ec,
};
use js_engine::{Completion, ExecutionContext};

/// <https://streams.spec.whatwg.org/#rs-class>
#[gc_struct]
pub struct ReadableStream {
    /// <https://streams.spec.whatwg.org/#readablestream-controller>
    controller: GcCell<Option<ReadableStreamController>>,

    controller_object: GcCell<Option<JsObject>>,

    /// <https://streams.spec.whatwg.org/#readablestream-reader>
    reader: GcCell<Option<ReadableStreamReader>>,

    /// <https://streams.spec.whatwg.org/#readablestream-disturbed>
    #[unsafe_ignore_trace]
    disturbed: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestream-state>
    #[unsafe_ignore_trace]
    state: Rc<RefCell<ReadableStreamState>>,

    /// <https://streams.spec.whatwg.org/#readablestream-storederror>
    stored_error: GcCell<JsValue>,
}

impl ReadableStream {
    pub(crate) fn new() -> Self {
        Self {
            controller: gc_cell_new(None),
            controller_object: gc_cell_new(None),
            reader: gc_cell_new(None),
            disturbed: Rc::new(Cell::new(false)),
            state: Rc::new(RefCell::new(ReadableStreamState::Readable)),
            stored_error: gc_cell_new(JsValue::undefined()),
        }
    }

    pub(crate) fn controller_slot(&self) -> Option<ReadableStreamController> {
        self.controller.borrow().clone()
    }

    pub(crate) fn set_controller_slot(&self, controller: Option<ReadableStreamController>) {
        *self.controller.borrow_mut() = controller;
    }

    pub(crate) fn controller_object_slot(&self) -> Option<JsObject> {
        self.controller_object.borrow().clone()
    }

    pub(crate) fn set_controller_object_slot(&self, controller_object: Option<JsObject>) {
        *self.controller_object.borrow_mut() = controller_object;
    }

    pub(crate) fn reader_slot(&self) -> Option<ReadableStreamReader> {
        self.reader.borrow().clone()
    }

    pub(crate) fn set_reader_slot(&self, reader: Option<ReadableStreamReader>) {
        *self.reader.borrow_mut() = reader;
    }

    pub(crate) fn state(&self) -> ReadableStreamState {
        self.state.borrow().clone()
    }

    pub(crate) fn set_state(&self, state: ReadableStreamState) {
        *self.state.borrow_mut() = state;
    }

    pub(crate) fn stored_error(&self) -> JsValue {
        self.stored_error.borrow().clone()
    }

    pub(crate) fn set_stored_error(&self, error: JsValue) {
        *self.stored_error.borrow_mut() = error;
    }

    pub(crate) fn set_disturbed(&self, disturbed: bool) {
        self.disturbed.set(disturbed);
    }

    /// <https://streams.spec.whatwg.org/#initialize-readable-stream>
    fn initialize_readable_stream(&mut self) {
        // Step 1: "Set stream.[[state]] to \"readable\"."
        *self.state.borrow_mut() = ReadableStreamState::Readable;

        // Step 2: "Set stream.[[reader]] and stream.[[storedError]] to undefined."
        *self.reader.borrow_mut() = None;
        *self.stored_error.borrow_mut() = JsValue::undefined();

        // Step 3: "Set stream.[[disturbed]] to false."
        self.disturbed.set(false);
    }

    /// <https://streams.spec.whatwg.org/#is-readable-stream-locked>
    pub(crate) fn is_readable_stream_locked(&self) -> bool {
        // Step 1: "If stream.[[reader]] is undefined, return false."
        if self.reader_slot().is_none() {
            return false;
        }

        // Step 2: "Return true."
        true
    }

    /// <https://streams.spec.whatwg.org/#rs-locked>
    pub(crate) fn locked(&self) -> bool {
        // Step 1: "Return ! IsReadableStreamLocked(this)."
        self.is_readable_stream_locked()
    }

    /// <https://streams.spec.whatwg.org/#rs-cancel>
    pub(crate) fn cancel(
        &mut self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.is_readable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot cancel a stream that already has a reader",
                ec,
            );
        }

        // Step 2: "Return ! ReadableStreamCancel(this, reason)."
        readable_stream_cancel(self.clone(), reason, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-get-reader>
    pub(crate) fn get_reader(
        &mut self,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        self.get_reader_ec(options, js_engine::boa::context_as_ec(context))
            .map_err(|e| JsError::from_opaque(e))
    }

    /// Generic entry point for <https://streams.spec.whatwg.org/#rs-get-reader>.
    /// Returns `Completion` - the binding layer uses this directly without bridging.
    pub(crate) fn get_reader_ec(
        &mut self,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        let options_object: Option<JsObject> = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(options.as_object().ok_or_else(|| {
                ec.new_type_error("ReadableStream.getReader() options must be an object")
            })?)
        };

        // Step 1: "If options[\"mode\"] does not exist, return ? AcquireReadableStreamDefaultReader(this)."
        let Some(options_object) = options_object else {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        };

        let mode_key = ec.property_key_from_str("mode");
        if !ec.has_property(options_object.clone(), mode_key.clone())? {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        }

        let mode = ExecutionContext::get(ec, options_object.clone(), mode_key)?;
        if mode.is_undefined() {
            return acquire_readable_stream_default_reader(self.clone(), ec);
        }

        // Step 2: "Assert: options[\"mode\"] is \"byob\"."
        let mode = ec.to_rust_string(mode)?;
        if mode != "byob" {
            return Err(ec.new_type_error(
                "ReadableStream.getReader() only supports the default reader mode",
            ));
        }

        // Step 3: "Return ? AcquireReadableStreamBYOBReader(this)."
        let Some(controller) = self.controller_slot() else {
            return Err(ec.new_type_error("ReadableStream is missing its controller"));
        };
        if controller.as_byte_controller().is_none() {
            return Err(ec.new_type_error("Cannot acquire a BYOB reader for a non-byte stream"));
        }

        acquire_readable_stream_byob_reader(self.clone(), ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-through>
    /// <https://streams.spec.whatwg.org/#rs-pipe-through>
    pub(crate) fn pipe_through(
        &mut self,
        transform: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, throw a TypeError exception."
        if self.locked() {
            return Err(ec.new_type_error("ReadableStream.pipeThrough() called on a locked stream"));
        }

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        // Note: This implementation performs the lock check below, after reading options members.
        let transform_obj = transform.as_object().ok_or_else(|| {
            ec.new_type_error("ReadableStream.pipeThrough() requires a ReadableWritablePair")
        })?;
        let readable_value = EcmascriptHost::get(&mut *ec, &transform_obj, "readable")?;
        let readable_obj = readable_value.as_object().ok_or_else(|| {
            ec.new_type_error("ReadableWritablePair is missing its readable property")
        })?;
        let _ = with_readable_stream_ref_ec(&readable_obj, ec, |stream| stream.clone())?;

        let writable_value = EcmascriptHost::get(&mut *ec, &transform_obj, "writable")?;
        let writable_obj = writable_value.as_object().ok_or_else(|| {
            ec.new_type_error("ReadableWritablePair is missing its writable property")
        })?;

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        //
        // Note: The implementation order diverges from the specification.
        // The specification performs Step 2 (IsWritableStreamLocked) before reading options members.
        // This implementation normalizes options first so option getters run before the lock check.
        // This ordering is currently required to match WPT behavior for
        // pipeThrough() should throw if an option getter grabs a writer.
        let options = normalize_pipe_options(options, ec)?;

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        let writable_locked =
            super::with_writable_stream_ref_ec(&writable_obj, ec, |ws| ws.locked())?;
        if writable_locked {
            return Err(ec.new_type_error(
                "ReadableStream.pipeThrough(): destination writable stream is locked",
            ));
        }

        // Step 4: "Let promise be ! ReadableStreamPipeTo(this, transform[\"writable\"], options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        let destination = super::with_writable_stream_ref_ec(&writable_obj, ec, |ws| ws.clone())?;
        let promise = readable_stream_pipe_to(
            self.clone(),
            destination,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
            ec,
        )?;

        // Step 5: "Set promise.[[PromiseIsHandled]] to true."
        crate::webidl::mark_promise_as_handled(&promise, ec)?;

        // Step 6: "Return transform[\"readable\"]."
        Ok(readable_value)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-to>
    pub(crate) fn pipe_to(
        &mut self,
        destination: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.locked() {
            return promise_rejected_with_type_error(
                "ReadableStream.pipeTo() called on a locked stream",
                ec,
            );
        }

        // Step 2: "If ! IsWritableStreamLocked(destination) is true, return a promise rejected with a TypeError exception."
        let dest_obj = match destination.as_object() {
            Some(obj) => obj.clone(),
            None => {
                return promise_rejected_with_type_error(
                    "ReadableStream.pipeTo() requires a WritableStream destination",
                    ec,
                );
            }
        };
        let dest_locked = super::with_writable_stream_ref_ec(&dest_obj, ec, |ws| ws.locked())?;
        if dest_locked {
            return promise_rejected_with_type_error(
                "ReadableStream.pipeTo(): destination is locked",
                ec,
            );
        }

        let options = normalize_pipe_options(options, ec)?;

        let dest = super::with_writable_stream_ref_ec(&dest_obj, ec, |ws| ws.clone())?;

        // Step 4: "Return ! ReadableStreamPipeTo(this, destination, options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        readable_stream_pipe_to(
            self.clone(),
            dest,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
            ec,
        )
    }

    /// <https://streams.spec.whatwg.org/#rs-tee>
    pub(crate) fn tee(
        &mut self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        // Step 1: "Return ? ReadableStreamTee(this, false)."
        let branches = readable_stream_tee(self.clone(), false, ec)?;
        branches.into_js_value(ec)
    }
}

struct ReadableStreamTeeBranches {
    _branch1: ReadableStream,
    branch1_object: JsObject,
    _branch2: ReadableStream,
    branch2_object: JsObject,
}

impl ReadableStreamTeeBranches {
    fn into_js_value(
        self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types> {
        let array = ec.create_empty_array();
        ec.array_push(
            &array,
            <crate::js::Types as JsTypes>::value_from_object(self.branch1_object),
        )?;
        ec.array_push(
            &array,
            <crate::js::Types as JsTypes>::value_from_object(self.branch2_object),
        )?;
        Ok(<crate::js::Types as JsTypes>::value_from_object(array))
    }
}

/// pull and cancel algorithms.
#[derive(Trace, Finalize)]
pub(crate) struct TeeState {
    source_stream: ReadableStream,
    reader: ReadableStreamDefaultReader,
    branch1: Option<ReadableStream>,
    branch2: Option<ReadableStream>,
    cancel_promise: JsObject,
    cancel_resolvers: PromiseResolvers<crate::js::Types>,
    #[unsafe_ignore_trace]
    reading: bool,
    #[unsafe_ignore_trace]
    read_again: bool,
    #[unsafe_ignore_trace]
    canceled1: bool,
    #[unsafe_ignore_trace]
    canceled2: bool,
    reason1: JsValue,
    reason2: JsValue,
}

fn default_tee_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    tee_state: &GcCell<TeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let error = args.get_or_undefined(0).clone();
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let tee_state = tee_state.borrow();
        (
            tee_state.branch1.clone(),
            tee_state.branch2.clone(),
            tee_state.canceled1,
            tee_state.canceled2,
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 19.1: "Perform ! ReadableStreamDefaultControllerError(branch1.[[controller]], r)."
    if let Some(branch1) = branch1.as_ref() {
        if let Err(error) = default_tee_error_branch(branch1, error.clone(), ec) {
            error!("[readable-stream] default tee error branch1 failed: {error:?}");
        }
    }

    // Step 19.2: "Perform ! ReadableStreamDefaultControllerError(branch2.[[controller]], r)."
    if let Some(branch2) = branch2.as_ref() {
        if let Err(error) = default_tee_error_branch(branch2, error, ec) {
            error!("[readable-stream] default tee error branch2 failed: {error:?}");
        }
    }

    // Step 19.3: "If canceled1 is false or canceled2 is false, resolve cancelPromise with undefined."
    if !canceled1 || !canceled2 {
        let undefined = ec.value_undefined();
        if let Err(error) = cancel_resolvers.resolve(undefined.clone(), ec) {
            error!("[readable-stream] failed to resolve cancel promise: {error:?}");
        }
    }

    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#readable-stream-tee>
fn readable_stream_tee(
    stream: ReadableStream,
    clone_for_branch2: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStreamTeeBranches, crate::js::Types> {
    // Step 1: "Assert: stream implements ReadableStream."
    // Step 2: "Assert: cloneForBranch2 is a boolean."

    // Step 3: "If stream.[[controller]] implements ReadableByteStreamController, return ? ReadableByteStreamTee(stream)."
    if stream
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
        .is_some()
    {
        return readable_byte_stream_tee(stream, ec);
    }

    // Step 4: "Return ? ReadableStreamDefaultTee(stream, cloneForBranch2)."
    readable_stream_default_tee(stream, clone_for_branch2, ec)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
fn readable_stream_default_tee(
    stream: ReadableStream,
    clone_for_branch2: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStreamTeeBranches, crate::js::Types> {
    // Step 1: "Assert: stream implements ReadableStream."
    // Step 2: "Assert: cloneForBranch2 is a boolean."

    // Step 3: "Let reader be ? AcquireReadableStreamDefaultReader(stream)."
    let reader_object = acquire_readable_stream_default_reader(stream.clone(), ec)?;
    let reader =
        with_readable_stream_default_reader_ref_ec(&reader_object, ec, |reader| reader.clone())?;

    // Step 12: "Let cancelPromise be a new promise."
    let reader_closed_promise = reader.closed(ec)?;

    // Step 19: "Upon rejection of reader.[[closedPromise]] with reason r,"
    // Note: mark the source reader's closed promise as handled before attaching the forwarding
    // reaction so engine-level unhandled-rejection reporting does not race this internal hook.
    mark_promise_as_handled(&reader_closed_promise, ec)?;

    let (cancel_promise_value, cancel_resolvers) = ec.new_promise_pending()?;
    let cancel_promise = <crate::js::Types as JsTypes>::value_as_object(&cancel_promise_value)
        .unwrap_or_else(|| ec.realm_global_object());

    // Step 4: "Let reading be false."
    // Step 5: "Let readAgain be false."
    // Step 6: "Let canceled1 be false."
    // Step 7: "Let canceled2 be false."
    // Step 8: "Let reason1 be undefined."
    // Step 9: "Let reason2 be undefined."
    // Step 10: "Let branch1 be undefined."
    // Step 11: "Let branch2 be undefined."
    let undefined = ec.value_undefined();
    let tee_state = gc_cell_new(TeeState {
        source_stream: stream,
        reader,
        branch1: None,
        branch2: None,
        cancel_promise,
        cancel_resolvers,
        reading: false,
        read_again: false,
        canceled1: false,
        canceled2: false,
        reason1: undefined.clone(),
        reason2: undefined,
    });

    // Step 16: "Let startAlgorithm be an algorithm that returns undefined."
    // Step 17: "Set branch1 to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancel1Algorithm)."
    let (branch1, branch1_object) = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableStreamDefaultTee {
            tee_state: tee_state.clone(),
            clone_for_branch2,
        },
        CancelAlgorithm::ReadableStreamDefaultTeeBranch1(tee_state.clone()),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        ec,
    )?;

    // Step 18: "Set branch2 to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancel2Algorithm)."
    let (branch2, branch2_object) = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableStreamDefaultTee {
            tee_state: tee_state.clone(),
            clone_for_branch2,
        },
        CancelAlgorithm::ReadableStreamDefaultTeeBranch2(tee_state.clone()),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        ec,
    )?;

    {
        let mut tee_state = tee_state.borrow_mut();
        tee_state.branch1 = Some(branch1.clone());
        tee_state.branch2 = Some(branch2.clone());
    }

    // Step 19: "Upon rejection of reader.[[closedPromise]] with reason r,"
    let on_rejected =
        crate::js::builtin_with_captures(ec, tee_state, default_tee_on_rejected_fn, 1);
    let forward_error_promise =
        <crate::js::Types as JsTypes>::object_as_promise(&reader_closed_promise)
            .ok_or_else(|| ec.new_type_error("reader_closed_promise is not a Promise"))?;
    let forward_error =
        ec.perform_promise_then(forward_error_promise, None, Some(on_rejected), None)?;
    let forward_error_obj = <crate::js::Types as JsTypes>::value_as_object(&forward_error)
        .unwrap_or_else(|| ec.realm_global_object());
    mark_promise_as_handled(&forward_error_obj, ec)?;

    // Step 20: "Return « branch1, branch2 »."
    Ok(ReadableStreamTeeBranches {
        _branch1: branch1,
        branch1_object,
        _branch2: branch2,
        branch2_object,
    })
}

fn structured_clone_value(
    value: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let global = ec.realm_global_object();
    let pk = ec.property_key_from_str("structuredClone");
    let sc_value = ExecutionContext::get(ec, global.clone(), pk)?;
    let sc_fn = <crate::js::Types as JsTypes>::value_as_object(&sc_value).ok_or_else(|| {
        ec.new_type_error("structuredClone is not available on the global object")
    })?;
    let undefined = ec.value_undefined();
    ec.call(&sc_fn, &undefined, &[value])
}

fn default_tee_enqueue_to_branch(
    branch: &ReadableStream,
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    controller.enqueue(chunk, ec)
}

fn default_tee_close_branch(
    branch: &ReadableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    controller.close(ec)
}

fn default_tee_error_branch(
    branch: &ReadableStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let Some(controller) = branch
        .controller_slot()
        .map(|controller| controller.as_default_controller())
    else {
        return Ok(());
    };
    controller.error(error, ec)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_pull_algorithm(
    tee_state: GcCell<TeeState>,
    clone_for_branch2: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 13.1: "If reading is true,"
    {
        let mut tee_state = tee_state.borrow_mut();
        if tee_state.reading {
            // Step 13.1.1: "Set readAgain to true."
            tee_state.read_again = true;
            // Step 13.1.2: "Return a promise resolved with undefined."
            return Ok(ec.value_undefined());
        }
        // Step 13.2: "Set reading to true."
        tee_state.reading = true;
    }

    // Step 13.3: "Let readRequest be a read request with the following items:"
    let read_request = ReadRequest::ReadableStreamDefaultTee {
        tee_state: tee_state.clone(),
        clone_for_branch2,
    };
    let reader = tee_state.borrow().reader.clone();

    // Step 13.4: "Perform ! ReadableStreamDefaultReaderRead(reader, readRequest)."
    if let Err(error) = reader.read_with_request(read_request, ec) {
        tee_state.borrow_mut().reading = false;
        return Err(error);
    }

    // Step 13.5: "Return a promise resolved with undefined."
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_chunk_steps(
    tee_state: GcCell<TeeState>,
    clone_for_branch2: bool,
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let realm = ec.current_realm();
    ec.enqueue_job_with_realm(
        realm,
        Box::new(move |job_ec: &mut dyn ExecutionContext<crate::js::Types>| {
            // Step 13.3 chunk steps 1.1: "Set readAgain to false."
            tee_state.borrow_mut().read_again = false;

            // Step 13.3 chunk steps 1.2: "Let chunk1 and chunk2 be chunk."
            let chunk1 = chunk.clone();
            let mut chunk2 = chunk;
            let (source_stream, branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
                let tee_state = tee_state.borrow();
                (
                    tee_state.source_stream.clone(),
                    tee_state.branch1.clone(),
                    tee_state.branch2.clone(),
                    tee_state.canceled1,
                    tee_state.canceled2,
                    tee_state.cancel_resolvers.clone(),
                )
            };

            // Step 13.3 chunk steps 1.3: "If canceled2 is false and cloneForBranch2 is true,"
            if !canceled2 && clone_for_branch2 {
                // Step 13.3 chunk steps 1.3.1: "Let cloneResult be StructuredClone(chunk2)."
                match structured_clone_value(chunk2.clone(), job_ec) {
                    Ok(cloned_chunk) => {
                        // Step 13.3 chunk steps 1.3.3: "Otherwise, set chunk2 to cloneResult.[[Value]]."
                        chunk2 = cloned_chunk;
                    }
                    Err(clone_error) => {
                        // Step 13.3 chunk steps 1.3.2.1: "Perform ! ReadableStreamDefaultControllerError(branch1.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch1) = branch1.as_ref() {
                            if let Err(error) =
                                default_tee_error_branch(branch1, clone_error.clone(), job_ec)
                            {
                                error!(
                                    "[readable-stream] default tee error branch1 (chunk) failed: {error:?}"
                                );
                            }
                        }

                        // Step 13.3 chunk steps 1.3.2.2: "Perform ! ReadableStreamDefaultControllerError(branch2.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch2) = branch2.as_ref() {
                            if let Err(error) =
                                default_tee_error_branch(branch2, clone_error.clone(), job_ec)
                            {
                                error!(
                                    "[readable-stream] default tee error branch2 (chunk) failed: {error:?}"
                                );
                            }
                        }

                        // Step 13.3 chunk steps 1.3.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                        if let Ok(cancel_result) =
                            readable_stream_cancel(source_stream, clone_error, job_ec)
                        {
                            let cancel_val = <crate::js::Types as JsTypes>::value_from_object(cancel_result);
                            let _ = cancel_resolvers.resolve(cancel_val, job_ec);
                        }

                        // Step 13.3 chunk steps 1.3.2.4: "Return."
                        return;
                    }
                }
            }

            // Step 13.3 chunk steps 1.4: "If canceled1 is false, perform ! ReadableStreamDefaultControllerEnqueue(branch1.[[controller]], chunk1)."
            if !canceled1 {
                if let Some(branch1) = branch1.as_ref() {
                    let _ = default_tee_enqueue_to_branch(branch1, chunk1, job_ec);
                }
            }

            // Step 13.3 chunk steps 1.5: "If canceled2 is false, perform ! ReadableStreamDefaultControllerEnqueue(branch2.[[controller]], chunk2)."
            if !canceled2 {
                if let Some(branch2) = branch2.as_ref() {
                    let _ = default_tee_enqueue_to_branch(branch2, chunk2, job_ec);
                }
            }

            // Step 13.3 chunk steps 1.6: "Set reading to false."
            // Step 13.3 chunk steps 1.7: "If readAgain is true, perform pullAlgorithm."
            let should_read_again = {
                let mut tee_state_ref = tee_state.borrow_mut();
                tee_state_ref.reading = false;
                let should_read_again = tee_state_ref.read_again;
                tee_state_ref.read_again = false;
                should_read_again
            };

            if should_read_again {
                match readable_stream_default_tee_pull_algorithm(
                    tee_state.clone(),
                    clone_for_branch2,
                    job_ec,
                ) {
                    Ok(value) => {
                        if let Ok(pull_promise) = resolved_promise(value, job_ec) {
                            let _ = mark_promise_as_handled(&pull_promise, job_ec);
                        }
                    }
                    Err(error) => {
                        error!("[readable-stream] default tee pull algorithm failed");
                        let rejected = rejected_promise(error, job_ec)
                            .unwrap_or_else(|_| job_ec.realm_global_object());
                        let _ = mark_promise_as_handled(&rejected, job_ec);
                    }
                }
            }
        }),
    );
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_close_steps(
    tee_state: GcCell<TeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 13.3 close steps 1: "Set reading to false."
        tee_state.reading = false;
        (
            tee_state.branch1.clone(),
            tee_state.branch2.clone(),
            tee_state.canceled1,
            tee_state.canceled2,
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 13.3 close steps 2: "If canceled1 is false, perform ! ReadableStreamDefaultControllerClose(branch1.[[controller]])."
    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            default_tee_close_branch(branch1, ec)?;
        }
    }

    // Step 13.3 close steps 3: "If canceled2 is false, perform ! ReadableStreamDefaultControllerClose(branch2.[[controller]])."
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            default_tee_close_branch(branch2, ec)?;
        }
    }

    // Step 13.3 close steps 4: "If canceled1 is false or canceled2 is false, resolve cancelPromise with undefined."
    if !canceled1 || !canceled2 {
        let undefined = ec.value_undefined();
        ec.call(&cancel_resolvers.resolve, &undefined, &[undefined.clone()])?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_read_request_error_steps(tee_state: GcCell<TeeState>) {
    // Step 13.3 error steps 1: "Set reading to false."
    tee_state.borrow_mut().reading = false;
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_cancel1_algorithm(
    tee_state: GcCell<TeeState>,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let (source_stream, cancel_promise, canceled2, reason1, reason2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 14.1: "Set canceled1 to true."
        tee_state.canceled1 = true;

        // Step 14.2: "Set reason1 to reason."
        tee_state.reason1 = reason;
        (
            tee_state.source_stream.clone(),
            tee_state.cancel_promise.clone(),
            tee_state.canceled2,
            tee_state.reason1.clone(),
            tee_state.reason2.clone(),
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 14.3: "If canceled2 is true,"
    if canceled2 {
        // Step 14.3.1: "Let compositeReason be ! CreateArrayFromList(« reason1, reason2 »)."
        let composite_reason = {
            let array = ec.create_empty_array();
            ec.array_push(&array, reason1)?;
            ec.array_push(&array, reason2)?;
            <crate::js::Types as JsTypes>::value_from_object(array)
        };

        // Step 14.3.2: "Let cancelResult be ! ReadableStreamCancel(stream, compositeReason)."
        let cancel_result = readable_stream_cancel(source_stream, composite_reason, ec)?;

        // Step 14.3.3: "Resolve cancelPromise with cancelResult."
        let resolve_val = <crate::js::Types as JsTypes>::value_from_object(cancel_result);
        cancel_resolvers.resolve(resolve_val, ec)?;
    }

    // Step 14.4: "Return cancelPromise."
    Ok(cancel_promise)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaulttee>
pub(crate) fn readable_stream_default_tee_cancel2_algorithm(
    tee_state: GcCell<TeeState>,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let (source_stream, cancel_promise, canceled1, reason1, reason2, cancel_resolvers) = {
        let mut tee_state = tee_state.borrow_mut();

        // Step 15.1: "Set canceled2 to true."
        tee_state.canceled2 = true;

        // Step 15.2: "Set reason2 to reason."
        tee_state.reason2 = reason;
        (
            tee_state.source_stream.clone(),
            tee_state.cancel_promise.clone(),
            tee_state.canceled1,
            tee_state.reason1.clone(),
            tee_state.reason2.clone(),
            tee_state.cancel_resolvers.clone(),
        )
    };

    // Step 15.3: "If canceled1 is true,"
    if canceled1 {
        // Step 15.3.1: "Let compositeReason be ! CreateArrayFromList(« reason1, reason2 »)."
        let composite_reason = {
            let array = ec.create_empty_array();
            ec.array_push(&array, reason1)?;
            ec.array_push(&array, reason2)?;
            <crate::js::Types as JsTypes>::value_from_object(array)
        };

        // Step 15.3.2: "Let cancelResult be ! ReadableStreamCancel(stream, compositeReason)."
        let cancel_result = readable_stream_cancel(source_stream, composite_reason, ec)?;

        // Step 15.3.3: "Resolve cancelPromise with cancelResult."
        let resolve_val = <crate::js::Types as JsTypes>::value_from_object(cancel_result);
        cancel_resolvers.resolve(resolve_val, ec)?;
    }

    // Step 15.4: "Return cancelPromise."
    Ok(cancel_promise)
}

#[derive(Clone, Trace, Finalize)]
enum ReadableStreamFromIteratorKind {
    Async,
    Sync,
}

#[derive(Clone, Trace, Finalize)]
struct ReadableStreamFromIteratorRecord {
    iterator: JsObject,
    next_method: JsObject,
    kind: ReadableStreamFromIteratorKind,
}

impl ReadableStreamFromIteratorRecord {
    fn next_result_promise(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsObject, crate::js::Types> {
        let iterator_val = <crate::js::Types as JsTypes>::value_from_object(self.iterator.clone());
        let next_result = ec.call(&self.next_method, &iterator_val, &[])?;

        match self.kind {
            ReadableStreamFromIteratorKind::Async => promise_from_value(next_result, ec),
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(next_result, ec)
            }
        }
    }

    fn return_result_promise(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<JsObject>, crate::js::Types> {
        let return_key = ec.property_key_from_str("return");
        let return_method_value = ExecutionContext::get(ec, self.iterator.clone(), return_key)?;
        let return_method = get_optional_callable_method_value(
            return_method_value,
            "ReadableStream.from() iterator.return",
            ec,
        )?;
        let Some(return_method) = return_method else {
            return Ok(None);
        };

        let iterator_val = <crate::js::Types as JsTypes>::value_from_object(self.iterator.clone());
        let return_result = ec.call(&return_method, &iterator_val, &[reason])?;
        let return_promise = match self.kind {
            ReadableStreamFromIteratorKind::Async => promise_from_value(return_result, ec)?,
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(return_result, ec)?
            }
        };
        Ok(Some(return_promise))
    }
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct ReadableStreamFromIterableState {
    iterator_record: ReadableStreamFromIteratorRecord,
    stream: GcCell<Option<ReadableStream>>,
}

impl ReadableStreamFromIterableState {
    fn new(iterator_record: ReadableStreamFromIteratorRecord) -> Self {
        Self {
            iterator_record,
            stream: gc_cell_new(None),
        }
    }

    fn set_stream(&self, stream: ReadableStream) {
        *self.stream.borrow_mut() = Some(stream);
    }

    fn stream(&self) -> Option<ReadableStream> {
        self.stream.borrow().clone()
    }
}
/// <https://streams.spec.whatwg.org/#rs-constructor>
pub(crate) fn construct_readable_stream(
    _new_target: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStream, crate::js::Types> {
    let mut stream = ReadableStream::new();

    // Step 1: "If underlyingSource is missing, set it to undefined."
    let underlying_source = if args.is_empty() {
        ec.value_undefined()
    } else {
        args[0].clone()
    };

    // Step 2: "Let underlyingSourceDict be underlyingSource, converted to an IDL value of type UnderlyingSource."
    // Note: The implementation keeps the original JavaScript object so it can invoke the underlying source callbacks directly.
    let underlying_source_object = if underlying_source.is_undefined() {
        None
    } else {
        Some(underlying_source.as_object().ok_or_else(|| {
            ec.new_type_error("ReadableStream underlyingSource must be an object")
        })?)
    };

    // Step 3: "Perform ! InitializeReadableStream(this)."
    // Note: The backing struct is returned from the data constructor, after which Boa wraps it
    // in the newly created JsObject.
    stream.initialize_readable_stream();

    let strategy = args.get_or_undefined(1).clone();
    match underlying_source_type(underlying_source_object.as_ref(), ec)?.as_deref() {
        Some("bytes") => {
            // Step 4.1: "If strategy[\"size\"] exists, throw a RangeError exception."
            if strategy_has_size(&strategy, ec)? {
                return Err(
                    ec.new_range_error("a byte stream strategy cannot include a size function")
                );
            }

            // Step 4.2: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 0)."
            let high_water_mark = extract_high_water_mark(&strategy, 0.0, ec)?;

            // Step 4.3: "Perform ? SetUpReadableByteStreamControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark)."
            set_up_readable_byte_stream_controller_from_underlying_source(
                stream.clone(),
                underlying_source_object,
                high_water_mark,
                ec,
            )?;
            return Ok(stream);
        }
        Some(_) => {
            return Err(ec.new_type_error(
                "ReadableStream underlyingSource.type must be \"bytes\" when present",
            ));
        }
        None => {}
    }

    // Step 5.1: "Assert: underlyingSourceDict[\"type\"] does not exist."
    debug_assert!(underlying_source_type(underlying_source_object.as_ref(), ec)?.is_none());

    // Step 5.2: "Let sizeAlgorithm be ! ExtractSizeAlgorithm(strategy)."
    let size_algorithm = extract_size_algorithm(&strategy, ec)?;

    // Step 5.3: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 1)."
    let high_water_mark = extract_high_water_mark(&strategy, 1.0, ec)?;

    // Step 5.4: "Perform ? SetUpReadableStreamDefaultControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller_from_underlying_source(
        stream.clone(),
        underlying_source_object,
        high_water_mark,
        size_algorithm,
        ec,
    )?;

    Ok(stream)
}

/// <https://streams.spec.whatwg.org/#create-readable-stream>
pub(crate) fn create_readable_stream(
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    high_water_mark: Option<f64>,
    size_algorithm: Option<SizeAlgorithm>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(ReadableStream, JsObject), crate::js::Types> {
    // Step 1: "If highWaterMark was not passed, set it to 1."
    let high_water_mark = high_water_mark.unwrap_or(1.0);

    // Step 2: "If sizeAlgorithm was not passed, set it to an algorithm that returns 1."
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);

    // Step 3: "Assert: ! IsNonNegativeNumber(highWaterMark) is true."
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    // Step 4: "Let stream be a new ReadableStream."
    let (mut stream, stream_object) = create_readable_stream_object(ec)?;

    // Step 5: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 6: "Let controller be a new ReadableStreamDefaultController."
    let controller = super::ReadableStreamDefaultController::new();
    let controller_object: JsObject = create_interface_instance::<
        crate::js::Types,
        super::ReadableStreamDefaultController,
    >(controller.clone(), ec)?;

    // Step 7: "Perform ? SetUpReadableStreamDefaultController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller(
        stream.clone(),
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        high_water_mark,
        size_algorithm,
        ec,
    )?;

    // Step 8: "Return stream."
    Ok((stream, stream_object))
}

fn create_readable_stream_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(ReadableStream, JsObject), crate::js::Types> {
    let stream = ReadableStream::new();
    let stream_object: JsObject =
        create_interface_instance::<crate::js::Types, ReadableStream>(stream.clone(), ec)?;
    Ok((stream, stream_object))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-createreadablebytestream>
fn create_readable_byte_stream(
    start_algorithm: StartAlgorithm,
    pull_algorithm: PullAlgorithm,
    cancel_algorithm: CancelAlgorithm,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(ReadableStream, JsObject), crate::js::Types> {
    // Step 1: "Let stream be a new ReadableStream."
    let (mut stream, stream_object) = create_readable_stream_object(ec)?;

    // Step 2: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 3: "Let controller be a new ReadableByteStreamController."
    let controller = ReadableByteStreamController::new();
    let controller_object: JsObject = create_interface_instance::<
        crate::js::Types,
        ReadableByteStreamController,
    >(controller.clone(), ec)?;

    // Step 4: "Perform ? SetUpReadableByteStreamController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, 0, undefined)."
    super::set_up_readable_byte_stream_controller(
        stream.clone(),
        controller,
        &controller_object,
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        0.0,
        None,
        ec,
    )?;

    // Step 5: "Return stream."
    Ok((stream, stream_object))
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable(
    async_iterable: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Let stream be undefined."
    let state = ReadableStreamFromIterableState::new(get_readable_stream_from_iterator_record(
        async_iterable,
        ec,
    )?);

    // Step 2: "Let iteratorRecord be ? GetIterator(asyncIterable, async)."
    // Note: `get_readable_stream_from_iterator_record()` normalizes async iterators and the
    // async-from-sync fallback into a record whose `next_result_promise()` matches the spec.

    // Step 3: "Let startAlgorithm be an algorithm that returns undefined."
    let start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 4: "Let pullAlgorithm be the following steps:"
    let pull_algorithm = PullAlgorithm::ReadableStreamFromIterable(state.clone());

    // Step 5: "Let cancelAlgorithm be the following steps, given reason:"
    let cancel_algorithm = CancelAlgorithm::ReadableStreamFromIterable(state.clone());

    // Step 6: "Set stream to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancelAlgorithm, 0)."
    let (stream, stream_object) = create_readable_stream(
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        Some(0.0),
        None,
        ec,
    )?;
    state.set_stream(stream);

    // Step 7: "Return stream."
    Ok(stream_object)
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable_pull_algorithm(
    state: ReadableStreamFromIterableState,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 4.1: "Let nextResult be IteratorNext(iteratorRecord)."
    let next_result = state.iterator_record.next_result_promise(ec);

    // Step 4.2: "If nextResult is an abrupt completion, return a promise rejected with nextResult.[[Value]]."
    let next_promise = match next_result {
        Ok(next_promise) => next_promise,
        Err(error) => {
            return rejected_promise(error, ec);
        }
    };

    // Step 4.3: "Let nextPromise be a promise resolved with nextResult.[[Value]]."

    // Step 4.4: "Return the result of reacting to nextPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = crate::js::builtin_with_captures(
        ec,
        state,
        readable_stream_from_iterable_pull_on_fulfilled_fn,
        1,
    );

    let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&next_promise)
        .ok_or_else(|| ec.new_type_error("not a Promise"))?;
    let capability = ec.new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)?;
    let result_promise = capability.promise.clone();
    ec.perform_promise_then(js_promise, Some(on_fulfilled), None, Some(capability))?;
    let result_obj = <crate::js::Types as JsTypes>::value_as_object(&result_promise)
        .unwrap_or_else(|| ec.realm_global_object());
    Ok(result_obj)
}

fn readable_stream_from_iterable_pull_on_fulfilled_fn(
    args: &[JsValue],
    _this: JsValue,
    state: &ReadableStreamFromIterableState,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let iter_result = args.get_or_undefined(0).clone();

    // Step 4.4.1: "If iterResult is not an Object, throw a TypeError."
    let iter_result_object = iter_result.as_object().ok_or_else(|| {
        ec.new_type_error("ReadableStream.from() iterator next() must fulfill with an object")
    })?;

    // Step 4.4.2: "Let done be ? IteratorComplete(iterResult)."
    use js_engine::EcmascriptHost;
    let done_value = EcmascriptHost::get(ec, &iter_result_object, "done")?;
    let done = ec.to_boolean(&done_value);

    let stream = state
        .stream()
        .ok_or_else(|| ec.new_type_error("ReadableStream.from() is missing its stream"))?;
    let controller = stream
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream.from() is missing its controller"))?;
    let controller = controller.as_default_controller();

    // Step 4.4.3: "If done is true:"
    if done {
        // Step 4.4.3.1: "Perform ! ReadableStreamDefaultControllerClose(stream.[[controller]])."
        controller.close_steps(ec)?;
        return Ok(ec.value_undefined());
    }

    // Step 4.4.4.1: "Let value be ? IteratorValue(iterResult)."
    let value = EcmascriptHost::get(ec, &iter_result_object, "value")?;

    // Step 4.4.4.2: "Perform ! ReadableStreamDefaultControllerEnqueue(stream.[[controller]], value)."
    controller.enqueue_steps(value, ec)?;
    Ok(ec.value_undefined())
}

fn readable_stream_from_iterable_cancel_on_fulfilled_fn(
    args: &[JsValue],
    _this: JsValue,
    _captures: &(),
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 5.8.1: "If iterResult is not an Object, throw a TypeError."
    if args.get_or_undefined(0).as_object().is_none() {
        return Err(ec.new_type_error(
            "ReadableStream.from() iterator return() must fulfill with an object",
        ));
    }

    // Step 5.8.2: "Return undefined."
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable_cancel_algorithm(
    state: ReadableStreamFromIterableState,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Steps 5.1-5.7: Folds spec steps into return_result_promise.
    let return_result = state.iterator_record.return_result_promise(reason, ec);
    let return_promise = match return_result {
        Ok(Some(return_promise)) => return_promise,
        Ok(None) => {
            return resolved_promise(ec.value_undefined(), ec);
        }
        Err(error) => {
            return rejected_promise(error, ec);
        }
    };

    // Step 5.8: "Return the result of reacting to returnPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = crate::js::builtin_with_captures(
        ec,
        (),
        readable_stream_from_iterable_cancel_on_fulfilled_fn,
        1,
    );

    let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&return_promise)
        .ok_or_else(|| ec.new_type_error("not a Promise"))?;
    let capability = ec.new_promise_capability(ec.realm_intrinsics(&ec.current_realm()).promise)?;
    let result_promise = capability.promise.clone();
    ec.perform_promise_then(js_promise, Some(on_fulfilled), None, Some(capability))?;
    let result_obj = <crate::js::Types as JsTypes>::value_as_object(&result_promise)
        .unwrap_or_else(|| ec.realm_global_object());
    Ok(result_obj)
}

fn get_readable_stream_from_iterator_record(
    async_iterable: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStreamFromIteratorRecord, crate::js::Types> {
    let iterable_object = <crate::js::Types as JsTypes>::value_as_object(&async_iterable)
        .ok_or_else(|| ec.new_type_error("ReadableStream.from() argument must be an object"))?;

    let async_iterator_key = ec.property_key_from_str("asyncIterator");
    // Note: @@asyncIterator is Symbol.asyncIterator, not the string "asyncIterator".
    // We use the property name that the iterable exposes — @@asyncIterator and @@iterator
    // are accessed via Symbol well-known symbols, which requires symbol PropertyKey
    // support on the EC trait.  For now, the from-iterable code path is not exposed
    // to user code, so this approximate lookup suffices.
    let async_iter_method_value =
        ExecutionContext::get(ec, iterable_object.clone(), async_iterator_key)?;

    if let Some(async_iterator_method) = get_optional_callable_method_value(
        async_iter_method_value,
        "ReadableStream.from() iterable[@@asyncIterator]",
        ec,
    )? {
        let iterator_val =
            <crate::js::Types as JsTypes>::value_from_object(iterable_object.clone());
        let iterator_obj = ec.call(&async_iterator_method, &iterator_val, &[])?;
        let iterator = <crate::js::Types as JsTypes>::value_as_object(&iterator_obj)
            .ok_or_else(|| {
                ec.new_type_error("ReadableStream.from() @@asyncIterator must return an object")
            })?
            .clone();
        let next_method = get_required_callable_method(
            &iterator,
            "next",
            "ReadableStream.from() iterator.next must be callable",
            ec,
        )?;
        return Ok(ReadableStreamFromIteratorRecord {
            iterator,
            next_method,
            kind: ReadableStreamFromIteratorKind::Async,
        });
    }

    let iterator_key = ec.property_key_from_str("iterator");
    // Note: Same approximation — @@iterator is Symbol.iterator.
    let iter_method_value = ExecutionContext::get(ec, iterable_object.clone(), iterator_key)?;
    let iterator_method = get_optional_callable_method_value(
        iter_method_value,
        "ReadableStream.from() iterable[@@iterator]",
        ec,
    )?
    .ok_or_else(|| {
        ec.new_type_error("ReadableStream.from() requires an async iterable or iterable")
    })?;
    let iterator_val = <crate::js::Types as JsTypes>::value_from_object(iterable_object);
    let iterator_obj = ec.call(&iterator_method, &iterator_val, &[])?;
    let iterator = <crate::js::Types as JsTypes>::value_as_object(&iterator_obj)
        .ok_or_else(|| ec.new_type_error("ReadableStream.from() @@iterator must return an object"))?
        .clone();
    let next_method = get_required_callable_method(
        &iterator,
        "next",
        "ReadableStream.from() iterator.next must be callable",
        ec,
    )?;
    Ok(ReadableStreamFromIteratorRecord {
        iterator,
        next_method,
        kind: ReadableStreamFromIteratorKind::Sync,
    })
}

fn promise_from_sync_iterator_result_on_fulfilled_fn(
    args: &[JsValue],
    _this: JsValue,
    done: &bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_key = ec.property_key_from_str("value");
    let done_key = ec.property_key_from_str("done");
    let done_value = ec.value_from_bool(*done);
    let object = ec.create_plain_object(None);
    ec.create_data_property(object.clone(), value_key, args.get_or_undefined(0).clone())?;
    ec.create_data_property(object.clone(), done_key, done_value)?;
    Ok(<crate::js::Types as JsTypes>::value_from_object(object))
}

fn promise_from_sync_iterator_result(
    iter_result: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let iter_result_object = match iter_result.as_object() {
        Some(iter_result_object) => iter_result_object.clone(),
        None => {
            return rejected_promise(
                ec.new_type_error("ReadableStream.from() iterator result must be an object"),
                ec,
            );
        }
    };

    let done_key = ec.property_key_from_str("done");
    let done = match ExecutionContext::get(ec, iter_result_object.clone(), done_key) {
        Ok(done) => ec.to_boolean(&done),
        Err(error) => {
            return rejected_promise(error, ec);
        }
    };
    let value_key = ec.property_key_from_str("value");
    let value = match ExecutionContext::get(ec, iter_result_object.clone(), value_key) {
        Ok(value) => value,
        Err(error) => {
            return rejected_promise(error, ec);
        }
    };
    let value_promise = match promise_from_value(value, ec) {
        Ok(value_promise) => value_promise,
        Err(error) => {
            return rejected_promise(error, ec);
        }
    };
    let on_fulfilled = crate::js::builtin_with_captures(
        ec,
        done,
        promise_from_sync_iterator_result_on_fulfilled_fn,
        0,
    );

    let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&value_promise)
        .ok_or_else(|| ec.new_type_error("not a Promise"))?;
    let intrinsics = ec.realm_intrinsics(&ec.current_realm());
    let capability = ec.new_promise_capability(intrinsics.promise)?;
    let result_promise = capability.promise.clone();
    ec.perform_promise_then(js_promise, Some(on_fulfilled), None, Some(capability))?;
    let result_obj = <crate::js::Types as JsTypes>::value_as_object(&result_promise)
        .unwrap_or_else(|| ec.realm_global_object());
    Ok(result_obj)
}

fn get_optional_callable_method_value(
    value: JsValue,
    description: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<JsObject>, crate::js::Types> {
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }

    let method = value.as_object().ok_or_else(|| {
        ec.new_type_error(&format!("{description} must be callable when provided"))
    })?;
    let method_val = <crate::js::Types as JsTypes>::value_from_object(method.clone());
    if !ec.is_callable(&method_val) {
        return Err(ec.new_type_error(&format!("{description} must be callable when provided")));
    }

    Ok(Some(method.clone()))
}

fn get_required_callable_method(
    object: &JsObject,
    property: &'static str,
    message: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let pk = ec.property_key_from_str(property);
    let value = ExecutionContext::get(ec, object.clone(), pk)?;
    get_optional_callable_method_value(value, message, ec)?
        .ok_or_else(|| ec.new_type_error(message))
}
pub(crate) fn with_readable_stream_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStream) -> R,
) -> JsResult<R> {
    let stream = object
        .downcast_ref::<ReadableStream>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a ReadableStream"))?;
    Ok(f(&stream))
}

pub(crate) fn with_readable_stream_ref_ec<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableStream) -> R,
) -> Completion<R, crate::js::Types> {
    let stream_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStream>());
    let stream = match stream_ref {
        Some(s) => s,
        None => return Err(ec.new_type_error("object is not a ReadableStream")),
    };
    Ok(f(stream))
}

/// <https://streams.spec.whatwg.org/#readable-stream-cancel>
/// <https://streams.spec.whatwg.org/#readable-stream-cancel>
pub(crate) fn readable_stream_cancel(
    stream: ReadableStream,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Set stream.[[disturbed]] to true."
    stream.set_disturbed(true);

    // Step 2: "If stream.[[state]] is \"closed\", return a promise resolved with undefined."
    if stream.state() == ReadableStreamState::Closed {
        return resolved_promise(ec.value_undefined(), ec);
    }

    // Step 3: "If stream.[[state]] is \"errored\", return a promise rejected with stream.[[storedError]]."
    if stream.state() == ReadableStreamState::Errored {
        return rejected_promise(stream.stored_error(), ec);
    }

    // Step 4: "Perform ! ReadableStreamClose(stream)."
    readable_stream_close(stream.clone(), ec)?;

    // Step 5: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 6: "If reader is not undefined and reader implements ReadableStreamBYOBReader,"
    // Note: The byte-stream controller's cancel steps own any pending BYOB read-into cleanup.
    let _ = reader;

    // Step 7: "Let sourceCancelPromise be ! stream.[[controller]].[[CancelSteps]](reason)."
    let controller = stream
        .controller_slot()
        .ok_or_else(|| ec.new_type_error("ReadableStream is missing its controller"))?;
    let source_cancel_promise = controller.cancel_steps(reason, ec)?;

    // Step 8: "Return the result of reacting to sourceCancelPromise with a fulfillment step that returns undefined."
    // This implements <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
    // by calling Web IDL's transform_promise_to_undefined which wraps PerformPromiseThen.
    transform_promise_to_undefined(&source_cancel_promise, ec)
}

/// <https://streams.spec.whatwg.org/#readable-stream-close>
pub(crate) fn readable_stream_close(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 2: "Set stream.[[state]] to \"closed\"."
    stream.set_state(ReadableStreamState::Closed);

    // Step 3: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 4: "If reader is undefined, return."
    let Some(reader) = reader else {
        return Ok(());
    };

    match &reader {
        ReadableStreamReader::Default(reader) => {
            // Step 5: "Resolve reader.[[closedPromise]] with undefined."
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let resolve: JsObject = resolvers.resolve.clone().into();
                let undefined = ec.value_undefined();
                ec.call(&resolve, &undefined, &[undefined.clone()])?;
                reader.set_closed_resolvers_slot_value(None);
            }

            // Step 6.1: "Let readRequests be reader.[[readRequests]]."
            let read_requests = reader.take_read_requests();

            // Step 6.2: "Set reader.[[readRequests]] to an empty list."
            // Note: `take_read_requests()` empties the list before the requests are processed.

            // Step 6.3: "For each readRequest of readRequests,"
            for read_request in read_requests {
                // Step 6.3.1: "Perform readRequest's close steps."
                read_request.close_steps(ec)?;
            }
        }
        ReadableStreamReader::BYOB(reader) => {
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let resolve: JsObject = resolvers.resolve.clone().into();
                let undefined = ec.value_undefined();
                ec.call(&resolve, &undefined, &[undefined.clone()])?;
                reader.set_closed_resolvers_slot_value(None);
            }
        }
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-error>
pub(crate) fn readable_stream_error(
    stream: ReadableStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 2: "Set stream.[[state]] to \"errored\"."
    stream.set_state(ReadableStreamState::Errored);

    // Step 3: "Set stream.[[storedError]] to e."
    stream.set_stored_error(error.clone());

    // Step 4: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 5: "If reader is undefined, return."
    let Some(reader) = reader else {
        return Ok(());
    };

    match &reader {
        ReadableStreamReader::Default(reader) => {
            // Step 7: "Set reader.[[closedPromise]].[[PromiseIsHandled]] to true."
            if let Some(closed_promise) = reader.closed_promise_slot_value() {
                mark_promise_as_handled(&closed_promise, ec)?;
            }

            // Step 6: "Reject reader.[[closedPromise]] with e."
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let reject: JsObject = resolvers.reject.clone().into();
                let undefined = ec.value_undefined();
                ec.call(&reject, &undefined, &[error.clone()])?;
                reader.set_closed_resolvers_slot_value(None);
            }

            // Step 8.1: "Perform ! ReadableStreamDefaultReaderErrorReadRequests(reader, e)."
            readable_stream_default_reader_error_read_requests(reader.clone(), error, ec)
        }
        ReadableStreamReader::BYOB(reader) => {
            if let Some(closed_promise) = reader.closed_promise_slot_value() {
                mark_promise_as_handled(&closed_promise, ec)?;
            }

            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                let reject: JsObject = resolvers.reject.clone().into();
                let undefined = ec.value_undefined();
                ec.call(&reject, &undefined, &[error.clone()])?;
                reader.set_closed_resolvers_slot_value(None);
            }

            Ok(())
        }
    }
}

/// <https://streams.spec.whatwg.org/#readable-stream-add-read-request>
pub(crate) fn readable_stream_add_read_request(
    stream: ReadableStream,
    read_request: ReadRequest,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: stream.[[reader]] implements ReadableStreamDefaultReader."
    let reader = stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .ok_or_else(|| ec.new_type_error("ReadableStream is not locked to a default reader"))?;

    // Step 2: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 3: "Append readRequest to stream.[[reader]].[[readRequests]]."
    reader.push_read_request(read_request);
    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-fulfill-read-request>
pub(crate) fn readable_stream_fulfill_read_request(
    stream: ReadableStream,
    chunk: JsValue,
    done: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 1: "Assert: ! ReadableStreamHasDefaultReader(stream) is true."
    let reader = stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .ok_or_else(|| ec.new_type_error("ReadableStream is not locked to a default reader"))?;

    // Step 2: "Let reader be stream.[[reader]]."

    // Step 3: "Assert: reader.[[readRequests]] is not empty."
    debug_assert!(reader.read_requests_len() > 0);

    // Step 4: "Let readRequest be reader.[[readRequests]][0]."
    // Step 5: "Remove readRequest from reader.[[readRequests]]."
    let read_request = reader
        .shift_read_request()
        .ok_or_else(|| ec.new_type_error("ReadableStream has no pending read request"))?;

    // Step 6: "If done is true, perform readRequest's close steps."
    if done {
        return read_request.close_steps(ec);
    }

    // Step 7: "Otherwise, perform readRequest's chunk steps, given chunk."
    read_request.chunk_steps(chunk, ec)
}

/// <https://streams.spec.whatwg.org/#readable-stream-get-num-read-requests>
pub(crate) fn readable_stream_get_num_read_requests(stream: ReadableStream) -> usize {
    // Step 1: "Assert: ! ReadableStreamHasDefaultReader(stream) is true."
    debug_assert!(readable_stream_has_default_reader(&stream));

    // Step 2: "Return stream.[[reader]].[[readRequests]]'s size."
    stream
        .reader_slot()
        .and_then(|reader| reader.as_default_reader())
        .map(|reader| reader.read_requests_len())
        .unwrap_or(0)
}

/// <https://streams.spec.whatwg.org/#readable-stream-has-default-reader>
pub(crate) fn readable_stream_has_default_reader(stream: &ReadableStream) -> bool {
    // Step 1: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 2: "If reader is undefined, return false."
    let Some(reader) = reader else {
        return false;
    };

    // Step 3: "If reader implements ReadableStreamDefaultReader, return true."
    if reader.is_default_reader() {
        return true;
    }

    // Step 4: "Return false."
    false
}

// ---- ByteTeeState ---------------------------------------------------------

/// Internal state for ReadableByteStreamTee algorithm, maintained across pull and cancel operations.
/// See `readable_byte_stream_tee` for the algorithm using this state.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct ByteTeeState {
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    source_stream: ReadableStream,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reader: ReadableStreamReader,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    branch1: Option<ReadableStream>,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    branch2: Option<ReadableStream>,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    cancel_promise: JsObject,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    cancel_resolvers: PromiseResolvers<crate::js::Types>,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    reading: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    read_again_for_branch1: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    read_again_for_branch2: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    canceled1: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    canceled2: bool,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reason1: JsValue,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    reason2: JsValue,
    /// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
    #[unsafe_ignore_trace]
    reader_generation: u64,
}

// ---- helpers used inside the byte tee algorithms --------------------------

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_enqueue_to_branch(
    branch: &ReadableStream,
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step helper: "Perform ! ReadableByteStreamControllerEnqueue(branchX.[[controller]], chunkX)."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    controller.enqueue(chunk, ec)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_error_branch(
    branch: &ReadableStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step helper: "Perform ! ReadableByteStreamControllerError(branchX.[[controller]], r)."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    controller.error(error, ec)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_close_branch(
    branch: &ReadableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step helper: "Perform ! ReadableByteStreamControllerClose(branchX.[[controller]])."
    let Some(controller) = branch
        .controller_slot()
        .and_then(|c| c.as_byte_controller())
    else {
        return Ok(());
    };
    controller.close(ec)
}

/// <https://streams.spec.whatwg.org/#readable-byte-stream-controller-has-pending-pull-intos>
fn byte_tee_pending_pull_into_controller(
    branch: &ReadableStream,
) -> Option<ReadableByteStreamController> {
    let controller = branch.controller_slot()?.as_byte_controller()?;
    if controller.pending_pull_intos_len() > 0 {
        Some(controller)
    } else {
        None
    }
}

fn byte_tee_forward_error_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(u64, GcCell<ByteTeeState>),
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (captured_generation, tee_state) = captures;
    // Step helper: "If thisReader is not reader, return."
    if tee_state.borrow().reader_generation != *captured_generation {
        return Ok(JsValue::undefined());
    }
    let error = args.get_or_undefined(0).clone();
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let tee = tee_state.borrow();
        (
            tee.branch1.clone(),
            tee.branch2.clone(),
            tee.canceled1,
            tee.canceled2,
            tee.cancel_resolvers.clone(),
        )
    };
    if let Some(ref branch1) = branch1 {
        if let Err(error) = byte_tee_error_branch(branch1, error.clone(), ec) {
            error!("[readable-stream] byte tee error branch1 failed: {error:?}");
        }
    }
    if let Some(ref branch2) = branch2 {
        if let Err(error) = byte_tee_error_branch(branch2, error, ec) {
            error!("[readable-stream] byte tee error branch2 failed: {error:?}");
        }
    }
    if !canceled1 || !canceled2 {
        let undefined = ec.value_undefined();
        if let Err(error) = cancel_resolvers.resolve(undefined.clone(), ec) {
            error!("[readable-stream] failed to resolve cancel promise: {error:?}");
        }
    }
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn byte_tee_forward_reader_error(
    reader_object: &JsObject,
    tee_state: &GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let closed_value = js_engine::EcmascriptHost::get(ec, reader_object, "closed")?;
    let closed_promise = crate::js::Types::value_as_object(&closed_value)
        .and_then(|o| crate::js::Types::object_as_promise(&o));
    let Some(closed_promise) = closed_promise else {
        return Ok(());
    };

    // Step helper: "Let thisReader be reader" for the forwardReaderError closure.
    let generation_at_attach = tee_state.borrow().reader_generation;
    let on_rejected = crate::js::builtin_with_captures(
        ec,
        (generation_at_attach, tee_state.clone()),
        byte_tee_forward_error_on_rejected_fn,
        1,
    );
    ec.perform_promise_then(closed_promise, None, Some(on_rejected), None)?;
    Ok(())
}

fn byte_tee_ignore_pull_completion(
    completion: Completion<JsValue, crate::js::Types>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let promise = promise_from_completion(completion.map_err(|e| JsError::from_opaque(e)), ec);
    mark_promise_as_handled(&JsObject::from(promise), ec)
}

fn byte_tee_switch_to_default_reader(
    tee_state: &GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    if !matches!(tee_state.borrow().reader, ReadableStreamReader::BYOB(_)) {
        return Ok(());
    }

    let (old_reader, source_stream) = {
        let tee = tee_state.borrow();
        (
            tee.reader.as_byob_reader().unwrap(),
            tee.source_stream.clone(),
        )
    };
    tee_state.borrow_mut().reader_generation += 1;
    readable_stream_byob_reader_release(old_reader, ec)?;
    let new_reader_object = acquire_readable_stream_default_reader(source_stream, ec)?;
    let new_reader = with_readable_stream_default_reader_ref_ec(
        &new_reader_object,
        ec,
        |r: &ReadableStreamDefaultReader| r.clone(),
    )?;
    tee_state.borrow_mut().reader = ReadableStreamReader::Default(new_reader);
    byte_tee_forward_reader_error(&new_reader_object, tee_state, ec)
}

fn byte_tee_switch_to_byob_reader(
    tee_state: &GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    if !matches!(tee_state.borrow().reader, ReadableStreamReader::Default(_)) {
        return Ok(());
    }

    let (old_reader, source_stream) = {
        let tee = tee_state.borrow();
        (
            tee.reader.as_default_reader().unwrap(),
            tee.source_stream.clone(),
        )
    };
    tee_state.borrow_mut().reader_generation += 1;
    readable_stream_default_reader_release(old_reader, ec)?;
    let new_reader_object = acquire_readable_stream_byob_reader(source_stream, ec)?;
    let new_reader =
        with_readable_stream_byob_reader_ref_ec(&new_reader_object, ec, |r| r.clone())?;
    tee_state.borrow_mut().reader = ReadableStreamReader::BYOB(new_reader);
    byte_tee_forward_reader_error(&new_reader_object, tee_state, ec)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_chunk_steps(
    tee_state: GcCell<ByteTeeState>,
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    queue_internal_stream_microtask(
        move |job_ec| {
            // Step 18.2 chunk steps 1.1: "Set readAgainForBranch1 to false."
            // Step 18.2 chunk steps 1.2: "Set readAgainForBranch2 to false."
            {
                let mut tee = tee_state.borrow_mut();
                tee.read_again_for_branch1 = false;
                tee.read_again_for_branch2 = false;
            }

            // Step 18.2 chunk steps 1.3: "Let chunk1 and chunk2 be chunk."
            let chunk1 = chunk.clone();
            let mut chunk2 = chunk;
            let (branch1, branch2, canceled1, canceled2) = {
                let tee = tee_state.borrow();
                (
                    tee.branch1.clone(),
                    tee.branch2.clone(),
                    tee.canceled1,
                    tee.canceled2,
                )
            };

            // Step 18.2 chunk steps 1.4: "If canceled1 is false and canceled2 is false,"
            if !canceled1 && !canceled2 {
                // Step 18.2 chunk steps 1.4.1: "Let cloneResult be CloneAsUint8Array(chunk)."
                match clone_as_uint8_array(chunk1.clone(), job_ec) {
                    Ok(cloned_chunk) => {
                        // Step 18.2 chunk steps 1.4.3: "Otherwise, set chunk2 to cloneResult.[[Value]]."
                        chunk2 = cloned_chunk;
                    }
                    Err(error) => {
                        // Step 18.2 chunk steps 1.4.2.1: "Perform ! ReadableByteStreamControllerError(branch1.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch1) = branch1.as_ref() {
                            if let Err(inner_error) =
                                byte_tee_error_branch(branch1, error.clone(), job_ec)
                            {
                                error!(
                                    "[readable-stream] byte tee error branch1 (chunk) failed: {inner_error:?}"
                                );
                            }
                        }

                        // Step 18.2 chunk steps 1.4.2.2: "Perform ! ReadableByteStreamControllerError(branch2.[[controller]], cloneResult.[[Value]])."
                        if let Some(branch2) = branch2.as_ref() {
                            if let Err(error) =
                                byte_tee_error_branch(branch2, error.clone(), job_ec)
                            {
                                error!(
                                    "[readable-stream] byte tee error branch2 (chunk) failed: {error:?}"
                                );
                            }
                        }

                        // Step 18.2 chunk steps 1.4.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                        let source_stream = tee_state.borrow().source_stream.clone();
                        let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                        let cancel_result = readable_stream_cancel(source_stream, error, job_ec)?;
                        let undefined = job_ec.value_undefined();
                        job_ec.call(
                            &cancel_resolvers.resolve,
                            &undefined,
                            &[cancel_result.into()],
                        )?;

                        // Step 18.2 chunk steps 1.4.2.4: "Return."
                        return Ok(());
                    }
                }
            }

            // Step 18.2 chunk steps 1.5: "If canceled1 is false, perform ! ReadableByteStreamControllerEnqueue(branch1.[[controller]], chunk1)."
            if !canceled1 {
                if let Some(branch1) = branch1.as_ref() {
                    byte_tee_enqueue_to_branch(branch1, chunk1, job_ec)?;
                }
            }

            // Step 18.2 chunk steps 1.6: "If canceled2 is false, perform ! ReadableByteStreamControllerEnqueue(branch2.[[controller]], chunk2)."
            if !canceled2 {
                if let Some(branch2) = branch2.as_ref() {
                    byte_tee_enqueue_to_branch(branch2, chunk2, job_ec)?;
                }
            }

            // Step 18.2 chunk steps 1.7: "Set reading to false."
            // Step 18.2 chunk steps 1.8: "If readAgainForBranch1 is true, perform pull1Algorithm."
            // Step 18.2 chunk steps 1.9: "Otherwise, if readAgainForBranch2 is true, perform pull2Algorithm."
            let (read_again1, read_again2) = {
                let mut tee = tee_state.borrow_mut();
                tee.reading = false;
                (tee.read_again_for_branch1, tee.read_again_for_branch2)
            };
            if read_again1 {
                byte_tee_ignore_pull_completion(
                    readable_byte_stream_tee_pull1_algorithm(tee_state.clone(), job_ec),
                    job_ec,
                )?;
            } else if read_again2 {
                byte_tee_ignore_pull_completion(
                    readable_byte_stream_tee_pull2_algorithm(tee_state.clone(), job_ec),
                    job_ec,
                )?;
            }

            Ok(())
        },
        ec,
    )
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_close_steps(
    tee_state: GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.reading = false;
        (
            tee.branch1.clone(),
            tee.branch2.clone(),
            tee.canceled1,
            tee.canceled2,
            tee.cancel_resolvers.clone(),
        )
    };

    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            byte_tee_close_branch(branch1, ec)?;
        }
    }
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            byte_tee_close_branch(branch2, ec)?;
        }
    }
    if !canceled1 {
        if let Some(branch1) = branch1.as_ref() {
            if let Some(controller) = byte_tee_pending_pull_into_controller(branch1) {
                controller.respond(0, ec)?;
            }
        }
    }
    if !canceled2 {
        if let Some(branch2) = branch2.as_ref() {
            if let Some(controller) = byte_tee_pending_pull_into_controller(branch2) {
                controller.respond(0, ec)?;
            }
        }
    }
    if !canceled1 || !canceled2 {
        let undefined = ec.value_undefined();
        ec.call(&cancel_resolvers.resolve, &undefined, &[undefined.clone()])?;
    }
    Ok(())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_default_reader_error_steps(tee_state: GcCell<ByteTeeState>) {
    tee_state.borrow_mut().reading = false;
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee_pull_with_default_reader(
    tee_state: GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Step 18.1: "If reader implements ReadableStreamBYOBReader,"
    byte_tee_switch_to_default_reader(&tee_state, ec)?;

    // Step 18.2: "Let readRequest be a read request with the following items:"
    let default_reader = tee_state.borrow().reader.as_default_reader().unwrap();
    let read_request = ReadRequest::ReadableByteStreamTee {
        tee_state: tee_state.clone(),
    };

    // Step 18.3: "Perform ! ReadableStreamDefaultReaderRead(reader, readRequest)."
    default_reader.read_with_request(read_request, ec)
}

fn byte_tee_pull_byob_on_rejected_fn(
    _args: &[JsValue],
    _this: JsValue,
    tee_state: &GcCell<ByteTeeState>,
    _ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    tee_state.borrow_mut().reading = false;
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee_pull_with_byob_reader(
    tee_state: GcCell<ByteTeeState>,
    view_value: JsValue,
    for_branch2: bool,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let view = match ArrayBufferViewDescriptor::from_value(view_value.clone(), ec) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

    // Step 19.1: "If reader implements ReadableStreamDefaultReader,"
    byte_tee_switch_to_byob_reader(&tee_state, ec)?;
    let byob_reader = tee_state.borrow().reader.as_byob_reader().unwrap();

    let on_fulfilled = crate::js::builtin_with_captures(
        ec,
        (tee_state.clone(), for_branch2),
        byte_tee_pull_byob_on_fulfilled_fn,
        1,
    );

    let on_rejected = crate::js::builtin_with_captures(
        ec,
        tee_state.clone(),
        byte_tee_pull_byob_on_rejected_fn,
        0,
    );

    let (read_into_request, promise_obj) = ReadIntoRequest::new(ec)?;
    let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&promise_obj)
        .ok_or_else(|| ec.new_type_error("not a Promise"))?;

    // Step 19.5: "Perform ! ReadableStreamBYOBReaderRead(reader, view, 1, readIntoRequest)."
    byob_reader.read_steps(view, 1, read_into_request, ec)?;
    ec.perform_promise_then(js_promise, Some(on_fulfilled), Some(on_rejected), None)?;
    Ok(())
}

fn byte_tee_pull_byob_on_fulfilled_fn(
    args: &[JsValue],
    _this: JsValue,
    captures: &(GcCell<ByteTeeState>, bool),
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (tee_state, for_branch2) = captures;
    let arg0 = args
        .get(0)
        .cloned()
        .unwrap_or_else(|| boa_engine::JsValue::undefined());
    let result = ec.to_object(arg0)?;
    let done_val = js_engine::EcmascriptHost::get(ec, &result, "done")?;
    let done = ec.to_boolean(&done_val);
    let chunk = js_engine::EcmascriptHost::get(ec, &result, "value")?;

    let (byob_branch, other_branch, byob_canceled, other_canceled) = {
        let tee = tee_state.borrow();
        if *for_branch2 {
            (
                tee.branch2.clone(),
                tee.branch1.clone(),
                tee.canceled2,
                tee.canceled1,
            )
        } else {
            (
                tee.branch1.clone(),
                tee.branch2.clone(),
                tee.canceled1,
                tee.canceled2,
            )
        }
    };

    queue_internal_stream_microtask(
        {
            let tee_state = tee_state.clone();
            move |job_ec| {
                // Step 19.4 chunk steps 1.1: "Set readAgainForBranch1 to false."
                // Step 19.4 chunk steps 1.2: "Set readAgainForBranch2 to false."
                {
                    let mut tee = tee_state.borrow_mut();
                    tee.read_again_for_branch1 = false;
                    tee.read_again_for_branch2 = false;
                }

                if done {
                    // Step 19.4 close steps 1: "Set reading to false."
                    tee_state.borrow_mut().reading = false;

                    // Step 19.4 close steps 4: "If byobCanceled is false, perform ! ReadableByteStreamControllerClose(byobBranch.[[controller]])."
                    if !byob_canceled {
                        if let Some(branch) = byob_branch.as_ref() {
                            byte_tee_close_branch(branch, job_ec)?;
                        }
                    }

                    // Step 19.4 close steps 5: "If otherCanceled is false, perform ! ReadableByteStreamControllerClose(otherBranch.[[controller]])."
                    if !other_canceled {
                        if let Some(branch) = other_branch.as_ref() {
                            byte_tee_close_branch(branch, job_ec)?;
                        }
                    }

                    // Step 19.4 close steps 6: "If chunk is not undefined,"
                    let undefined = job_ec.value_undefined();
                    if !job_ec.same_value(&chunk, &undefined) {
                        // Step 19.4 close steps 6.2: "If byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                        if !byob_canceled {
                            if let Some(branch) = byob_branch.as_ref() {
                                if let Ok(view) =
                                    ArrayBufferViewDescriptor::from_value(chunk.clone(), job_ec)
                                {
                                    if let Some(view_object) =
                                        <crate::js::Types as JsTypes>::value_as_object(&chunk)
                                    {
                                        if let Some(controller) = branch
                                            .controller_slot()
                                            .and_then(|c| c.as_byte_controller())
                                        {
                                            let _ = controller.respond_with_new_view(
                                                view,
                                                view_object,
                                                job_ec,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Step 19.4 close steps 6.3: "If otherCanceled is false and otherBranch.[[controller]].[[pendingPullIntos]] is not empty, perform ! ReadableByteStreamControllerRespond(otherBranch.[[controller]], 0)."
                        if !other_canceled {
                            if let Some(branch) = other_branch.as_ref() {
                                if let Some(controller) =
                                    byte_tee_pending_pull_into_controller(branch)
                                {
                                    let _ = controller.respond(0, job_ec);
                                }
                            }
                        }
                    }

                    // Step 19.4 close steps 7: "If byobCanceled is false or otherCanceled is false, resolve cancelPromise with undefined."
                    if !byob_canceled || !other_canceled {
                        let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                        job_ec.call(&cancel_resolvers.resolve, &undefined, &[undefined.clone()])?;
                    }

                    return Ok(());
                }

                // Step 19.4 chunk steps 1.3: "Let byobCanceled be canceled2 if forBranch2 is true, and canceled1 otherwise."
                // Step 19.4 chunk steps 1.4: "Let otherCanceled be canceled2 if forBranch2 is false, and canceled1 otherwise."
                if !other_canceled {
                    // Step 19.4 chunk steps 1.5.1: "Let cloneResult be CloneAsUint8Array(chunk)."
                    match clone_as_uint8_array(chunk.clone(), job_ec) {
                        Ok(cloned_chunk) => {
                            // Step 19.4 chunk steps 1.5.3: "Otherwise, let clonedChunk be cloneResult.[[Value]]."
                            // Step 19.4 chunk steps 1.5.4: "If byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                            if !byob_canceled {
                                if let Some(branch) = byob_branch.as_ref() {
                                    if let Ok(view) =
                                        ArrayBufferViewDescriptor::from_value(chunk.clone(), job_ec)
                                    {
                                        if let Some(view_object) =
                                            <crate::js::Types as JsTypes>::value_as_object(&chunk)
                                        {
                                            if let Some(controller) = branch
                                                .controller_slot()
                                                .and_then(|c| c.as_byte_controller())
                                            {
                                                let _ = controller.respond_with_new_view(
                                                    view,
                                                    view_object,
                                                    job_ec,
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            // Step 19.4 chunk steps 1.5.5: "Perform ! ReadableByteStreamControllerEnqueue(otherBranch.[[controller]], clonedChunk)."
                            if let Some(branch) = other_branch.as_ref() {
                                byte_tee_enqueue_to_branch(branch, cloned_chunk, job_ec)?;
                            }
                        }
                        Err(error) => {
                            // Step 19.4 chunk steps 1.5.2.1: "Perform ! ReadableByteStreamControllerError(byobBranch.[[controller]], cloneResult.[[Value]])."
                            if let Some(branch) = byob_branch.as_ref() {
                                if let Err(error) =
                                    byte_tee_error_branch(branch, error.clone(), job_ec)
                                {
                                    error!(
                                        "[readable-stream] byte tee error byob-branch (chunk) failed: {error:?}"
                                    );
                                }
                            }

                            // Step 19.4 chunk steps 1.5.2.2: "Perform ! ReadableByteStreamControllerError(otherBranch.[[controller]], cloneResult.[[Value]])."
                            if let Some(branch) = other_branch.as_ref() {
                                if let Err(error) =
                                    byte_tee_error_branch(branch, error.clone(), job_ec)
                                {
                                    error!(
                                        "[readable-stream] byte tee error other-branch (chunk) failed: {error:?}"
                                    );
                                }
                            }

                            // Step 19.4 chunk steps 1.5.2.3: "Resolve cancelPromise with ! ReadableStreamCancel(stream, cloneResult.[[Value]])."
                            let source_stream = tee_state.borrow().source_stream.clone();
                            let cancel_resolvers = tee_state.borrow().cancel_resolvers.clone();
                            let cancel_result =
                                readable_stream_cancel(source_stream, error, job_ec)?;
                            let undefined = job_ec.value_undefined();
                            job_ec.call(
                                &cancel_resolvers.resolve,
                                &undefined,
                                &[cancel_result.into()],
                            )?;

                            // Step 19.4 chunk steps 1.5.2.4: "Return."
                            tee_state.borrow_mut().reading = false;
                            return Ok(());
                        }
                    }
                } else if !byob_canceled {
                    // Step 19.4 chunk steps 1.6: "Otherwise, if byobCanceled is false, perform ! ReadableByteStreamControllerRespondWithNewView(byobBranch.[[controller]], chunk)."
                    if let Some(branch) = byob_branch.as_ref() {
                        if let Ok(view) =
                            ArrayBufferViewDescriptor::from_value(chunk.clone(), job_ec)
                        {
                            if let Some(view_object) =
                                <crate::js::Types as JsTypes>::value_as_object(&chunk)
                            {
                                if let Some(controller) = branch
                                    .controller_slot()
                                    .and_then(|c| c.as_byte_controller())
                                {
                                    let _ =
                                        controller.respond_with_new_view(view, view_object, job_ec);
                                }
                            }
                        }
                    }
                }

                // Step 19.4 chunk steps 1.7: "Set reading to false."
                let (read_again1, read_again2) = {
                    let mut tee = tee_state.borrow_mut();
                    tee.reading = false;
                    (tee.read_again_for_branch1, tee.read_again_for_branch2)
                };

                // Step 19.4 chunk steps 1.8: "If readAgainForBranch1 is true, perform pull1Algorithm."
                // Step 19.4 chunk steps 1.9: "Otherwise, if readAgainForBranch2 is true, perform pull2Algorithm."
                if read_again1 {
                    byte_tee_ignore_pull_completion(
                        readable_byte_stream_tee_pull1_algorithm(tee_state.clone(), job_ec),
                        job_ec,
                    )?;
                } else if read_again2 {
                    byte_tee_ignore_pull_completion(
                        readable_byte_stream_tee_pull2_algorithm(tee_state.clone(), job_ec),
                        job_ec,
                    )?;
                } else if matches!(tee_state.borrow().reader, ReadableStreamReader::BYOB(_)) {
                    // Note: Switch back to the default reader when no branch has an outstanding BYOB pull.
                    byte_tee_switch_to_default_reader(&tee_state, job_ec)?;
                }

                Ok(())
            }
        },
        ec,
    )?;

    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_pull1_algorithm(
    tee_state: GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    {
        let mut tee = tee_state.borrow_mut();

        // Step 20.1: "If reading is true,"
        if tee.reading {
            // Step 20.1.1: "Set readAgainForBranch1 to true."
            tee.read_again_for_branch1 = true;

            // Step 20.1.2: "Return a promise resolved with undefined."
            return Ok(ec.value_undefined());
        }

        // Step 20.2: "Set reading to true."
        tee.reading = true;
    }

    // Step 20.3: "Let byobRequest be ! ReadableByteStreamControllerGetBYOBRequest(branch1.[[controller]])."
    let byob_request_view = {
        let tee = tee_state.borrow();
        tee.branch1
            .as_ref()
            .and_then(|branch| branch.controller_slot())
            .and_then(|controller| controller.as_byte_controller())
            .and_then(|controller| controller.byob_request(ec).ok().flatten())
            .and_then(|request| js_engine::EcmascriptHost::get(ec, &request, "view").ok())
            .filter(|value: &JsValue| !value.is_null() && !value.is_undefined())
    };

    // Step 20.4: "If byobRequest is null, perform pullWithDefaultReader."
    if let Some(view) = byob_request_view {
        // Step 20.5: "Otherwise, perform pullWithBYOBReader, given byobRequest.[[view]] and false."
        readable_byte_stream_tee_pull_with_byob_reader(tee_state, view, false, ec)?;
    } else {
        readable_byte_stream_tee_pull_with_default_reader(tee_state, ec)?;
    }

    // Step 20.6: "Return a promise resolved with undefined."
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_pull2_algorithm(
    tee_state: GcCell<ByteTeeState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    {
        let mut tee = tee_state.borrow_mut();

        // Step 21.1: "If reading is true,"
        if tee.reading {
            // Step 21.1.1: "Set readAgainForBranch2 to true."
            tee.read_again_for_branch2 = true;

            // Step 21.1.2: "Return a promise resolved with undefined."
            return Ok(ec.value_undefined());
        }

        // Step 21.2: "Set reading to true."
        tee.reading = true;
    }

    // Step 21.3: "Let byobRequest be ! ReadableByteStreamControllerGetBYOBRequest(branch2.[[controller]])."
    let byob_request_view = {
        let tee = tee_state.borrow();
        tee.branch2
            .as_ref()
            .and_then(|branch| branch.controller_slot())
            .and_then(|controller| controller.as_byte_controller())
            .and_then(|controller| controller.byob_request(ec).ok().flatten())
            .and_then(|request| js_engine::EcmascriptHost::get(ec, &request, "view").ok())
            .filter(|value: &JsValue| !value.is_null() && !value.is_undefined())
    };

    // Step 21.4: "If byobRequest is null, perform pullWithDefaultReader."
    if let Some(view) = byob_request_view {
        // Step 21.5: "Otherwise, perform pullWithBYOBReader, given byobRequest.[[view]] and true."
        readable_byte_stream_tee_pull_with_byob_reader(tee_state, view, true, ec)?;
    } else {
        readable_byte_stream_tee_pull_with_default_reader(tee_state, ec)?;
    }

    // Step 21.6: "Return a promise resolved with undefined."
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_cancel1_algorithm(
    tee_state: GcCell<ByteTeeState>,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let (source_stream, cancel_promise, canceled2, reason1, reason2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.canceled1 = true;
        tee.reason1 = reason;
        (
            tee.source_stream.clone(),
            tee.cancel_promise.clone(),
            tee.canceled2,
            tee.reason1.clone(),
            tee.reason2.clone(),
            tee.cancel_resolvers.clone(),
        )
    };
    if canceled2 {
        let composite_reason_array = ec.create_empty_array();
        ec.array_push(&composite_reason_array, reason1.clone())?;
        ec.array_push(&composite_reason_array, reason2)?;
        let composite_reason =
            <crate::js::Types as JsTypes>::value_from_object(composite_reason_array);
        let cancel_result = readable_stream_cancel(source_stream, composite_reason, ec)?;
        let undefined = ec.value_undefined();
        ec.call(
            &cancel_resolvers.resolve,
            &undefined,
            &[JsValue::from(cancel_result)],
        )?;
    }
    Ok(cancel_promise)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
pub(crate) fn readable_byte_stream_tee_cancel2_algorithm(
    tee_state: GcCell<ByteTeeState>,
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let (source_stream, cancel_promise, canceled1, reason1, reason2, cancel_resolvers) = {
        let mut tee = tee_state.borrow_mut();
        tee.canceled2 = true;
        tee.reason2 = reason;
        (
            tee.source_stream.clone(),
            tee.cancel_promise.clone(),
            tee.canceled1,
            tee.reason1.clone(),
            tee.reason2.clone(),
            tee.cancel_resolvers.clone(),
        )
    };
    if canceled1 {
        let composite_reason_array = ec.create_empty_array();
        ec.array_push(&composite_reason_array, reason1)?;
        ec.array_push(&composite_reason_array, reason2)?;
        let composite_reason =
            <crate::js::Types as JsTypes>::value_from_object(composite_reason_array);
        let cancel_result = readable_stream_cancel(source_stream, composite_reason, ec)?;
        let undefined = ec.value_undefined();
        ec.call(
            &cancel_resolvers.resolve,
            &undefined,
            &[JsValue::from(cancel_result)],
        )?;
    }
    Ok(cancel_promise)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablebytestreamtee>
fn readable_byte_stream_tee(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<ReadableStreamTeeBranches, crate::js::Types> {
    // Steps 1-2: Assert stream and stream.[[controller]] (implicit in types).
    // Step 3: Let reader be ? AcquireReadableStreamDefaultReader(stream).
    let reader_object = acquire_readable_stream_default_reader(stream.clone(), ec)?;
    let reader = with_readable_stream_default_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let reader_closed_promise = reader.closed(ec)?;
    mark_promise_as_handled(&reader_closed_promise, ec)?;

    // Step 4: Let reading be false.
    // Step 5: Let readAgainForBranch1 be false.
    // Step 6: Let readAgainForBranch2 be false.
    // Step 7: Let canceled1 be false.
    // Step 8: Let canceled2 be false.
    // Step 9: Let reason1 be undefined.
    // Step 10: Let reason2 be undefined.
    // Step 11: Let branch1 be undefined.
    // Step 12: Let branch2 be undefined.
    // (All steps 4-12 initialized in ByteTeeState below.)

    // Step 13: Let cancelPromise be a new promise.
    let (cancel_promise_value, cancel_resolvers) = ec.new_promise_pending()?;
    let cancel_promise = <crate::js::Types as JsTypes>::value_as_object(&cancel_promise_value)
        .unwrap_or_else(|| ec.realm_global_object());

    let undefined = ec.value_undefined();
    let tee_state = gc_cell_new(ByteTeeState {
        source_stream: stream,
        reader: ReadableStreamReader::Default(reader),
        branch1: None,
        branch2: None,
        cancel_promise,
        cancel_resolvers,
        reading: false,
        read_again_for_branch1: false,
        read_again_for_branch2: false,
        canceled1: false,
        canceled2: false,
        reason1: undefined.clone(),
        reason2: undefined,
        reader_generation: 0,
    });

    // Step 22: "Let startAlgorithm be an algorithm that returns undefined."
    // Step 23: "Set branch1 to ! CreateReadableByteStream(startAlgorithm, pull1Algorithm, cancel1Algorithm)."
    let (branch1, branch1_object) = create_readable_byte_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableByteStreamTeeBranch1(tee_state.clone()),
        CancelAlgorithm::ReadableByteStreamTeeBranch1(tee_state.clone()),
        ec,
    )?;

    // Step 24: "Set branch2 to ! CreateReadableByteStream(startAlgorithm, pull2Algorithm, cancel2Algorithm)."
    let (branch2, branch2_object) = create_readable_byte_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::ReadableByteStreamTeeBranch2(tee_state.clone()),
        CancelAlgorithm::ReadableByteStreamTeeBranch2(tee_state.clone()),
        ec,
    )?;

    {
        let mut tee = tee_state.borrow_mut();
        tee.branch1 = Some(branch1.clone());
        tee.branch2 = Some(branch2.clone());
    }

    // Step 23: Perform forwardReaderError, given reader.
    byte_tee_forward_reader_error(&reader_object, &tee_state, ec)?;
    // Step 24: Return « branch1, branch2 ».

    Ok(ReadableStreamTeeBranches {
        _branch1: branch1,
        branch1_object,
        _branch2: branch2,
        branch2_object,
    })
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-cloneasuint8array>
fn clone_as_uint8_array(
    chunk: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: Assert: O is an Object.
    // Step 2: Assert: O has an [[ViewedArrayBuffer]] internal slot.
    let typed_array = <crate::js::Types as JsTypes>::value_as_object(&chunk)
        .and_then(|obj| <crate::js::Types as JsTypes>::object_as_typed_array(&obj))
        .ok_or_else(|| ec.new_type_error("Expected a TypedArray"))?;

    // Note: Step 3 (IsDetachedBuffer) is not asserted — Boa's JsArrayBuffer
    // does not expose is_detached publicly; is_detached_buffer always returns
    // false on the Boa backend. CloneArrayBuffer below will fail if detached.

    let buffer = ec.typed_array_buffer(&typed_array)?;
    let byte_offset = ec.typed_array_byte_offset(&typed_array)?;
    let byte_length = ec.typed_array_byte_length(&typed_array)?;
    let intrinsics = ec.realm_intrinsics(&ec.current_realm());

    // Step 4: Let buffer be ? CloneArrayBuffer(O.[[ViewedArrayBuffer]], O.[[ByteOffset]], O.[[ByteLength]], %ArrayBuffer%).
    let cloned =
        ec.clone_array_buffer(buffer, byte_offset, byte_length, intrinsics.array_buffer)?;

    // Step 5: Let array be ! Construct(%Uint8Array%, « buffer »).
    let buffer_obj = <crate::js::Types as JsTypes>::object_from_array_buffer(cloned);
    let buffer_val = <crate::js::Types as JsTypes>::value_from_object(buffer_obj);
    let array = ec.construct(intrinsics.uint8_array, &[buffer_val], None)?;

    // Step 6: Return array.
    Ok(<crate::js::Types as JsTypes>::value_from_object(array))
}

/// invocation.
fn underlying_source_type(
    source_object: Option<&JsObject>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<String>, crate::js::Types> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    let type_key = ec.property_key_from_str("type");
    if !ec.has_property(source_object.clone(), type_key.clone())? {
        return Ok(None);
    }

    let value = ExecutionContext::get(ec, source_object.clone(), type_key)?;
    let undefined_value = ec.value_undefined();
    if ec.same_value(&value, &undefined_value) {
        return Ok(None);
    }

    Ok(Some(ec.to_rust_string(value)?))
}
fn strategy_has_size(
    strategy: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<bool, crate::js::Types> {
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(false);
    }

    let strategy = ec.to_object(strategy.clone())?;
    let size_key = ec.property_key_from_str("size");
    if !ec.has_property(strategy.clone(), size_key.clone())? {
        return Ok(false);
    }

    let value = ExecutionContext::get(ec, strategy, size_key)?;
    let undefined_value = ec.value_undefined();
    Ok(!ec.same_value(&value, &undefined_value))
}

fn extract_abort_signal(
    options_object: Option<&<crate::js::Types as JsTypes>::JsObject>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<AbortSignal>, crate::js::Types> {
    let Some(options_object) = options_object else {
        return Ok(None);
    };

    if !ec.has_property(options_object.clone(), ec.property_key_from_str("signal"))? {
        return Ok(None);
    }

    let signal = EcmascriptHost::get(ec, &options_object, "signal")?;
    if signal.is_undefined() {
        return Ok(None);
    }

    if signal.is_null() {
        return Err(ec.new_type_error("ReadableStream pipe options.signal must be an AbortSignal"));
    }

    let signal_object = signal.as_object().ok_or_else(|| {
        ec.new_type_error("ReadableStream pipe options.signal must be an AbortSignal")
    })?;

    with_abort_signal_ref(&signal_object, |signal| signal.clone())
        .map(Some)
        .map_err(|_| ec.new_type_error("options.signal is not an AbortSignal"))
}

struct PipeOptions {
    prevent_abort: bool,
    prevent_cancel: bool,
    prevent_close: bool,
    signal: Option<AbortSignal>,
}

fn normalize_pipe_options(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<PipeOptions, crate::js::Types> {
    let options_object = if options.is_undefined() || options.is_null() {
        None
    } else {
        let obj = ec.to_object(options.clone())?;
        Some(obj)
    };

    let prevent_abort = match options_object.as_ref() {
        Some(options_object) => {
            let val = EcmascriptHost::get(ec, options_object, "preventAbort")?;
            ec.to_boolean(&val)
        }
        None => false,
    };

    let prevent_cancel = match options_object.as_ref() {
        Some(options_object) => {
            let val = EcmascriptHost::get(ec, options_object, "preventCancel")?;
            ec.to_boolean(&val)
        }
        None => false,
    };

    let prevent_close = match options_object.as_ref() {
        Some(options_object) => {
            let val = EcmascriptHost::get(ec, options_object, "preventClose")?;
            ec.to_boolean(&val)
        }
        None => false,
    };

    let signal = extract_abort_signal(options_object.as_ref(), ec)?;

    Ok(PipeOptions {
        prevent_abort,
        prevent_cancel,
        prevent_close,
        signal,
    })
}

fn promise_rejected_with_reason(
    reason: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    crate::webidl::rejected_promise(reason, ec)
}

fn promise_rejected_with_type_error(
    message: &'static str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let reason = type_error_value(message, ec)?;
    promise_rejected_with_reason(reason, ec)
}

fn promise_rejected_with_error(
    error: JsError,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    Ok(crate::webidl::rejected_promise_from_error(error, ec))
}

fn reject_promise_with_error(
    resolvers: &js_engine::PromiseResolvers<crate::js::Types>,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) {
    let undefined = ec.value_undefined();
    if let Err(error) = ec.call(&resolvers.reject, &undefined, &[error]) {
        error!("[readable-stream] failed to reject promise with error: {error:?}");
    }
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn readable_stream_pipe_to(
    source: ReadableStream,
    dest: super::WritableStream,
    prevent_close: bool,
    prevent_abort: bool,
    prevent_cancel: bool,
    signal: Option<AbortSignal>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Assert: source implements ReadableStream."

    // Step 2: "Assert: dest implements WritableStream."

    // Step 3: "Assert: preventClose, preventAbort, and preventCancel are all booleans."

    // Step 4: "If signal was not given, let signal be undefined."

    // Step 5: "Assert: either signal is undefined, or signal implements AbortSignal."
    // Note: `pipe_to()` and `pipe_through()` normalize the `signal` argument to `Option<AbortSignal>` before calling this helper.

    // Step 13: "Let promise be a new promise."
    // Note: the promise is allocated before the remaining setup so unexpected internal setup
    // failures are still reported through the same returned promise object.
    let (pipe_promise, pipe_resolvers) = ec.new_promise_pending()?;
    let pipe_promise_obj = pipe_promise
        .as_object()
        .map(|o| o.clone())
        .unwrap_or_else(|| ec.realm_global_object());

    // Step 8: "If source.[[controller]] implements ReadableByteStreamController, let reader be either ! AcquireReadableStreamBYOBReader(source) or ! AcquireReadableStreamDefaultReader(source), at the user agent's discretion."
    // Note: Readable byte streams are not implemented yet, so the implementation always uses the default reader path.

    // Step 9: "Otherwise, let reader be ! AcquireReadableStreamDefaultReader(source)."
    let reader_object = match acquire_readable_stream_default_reader(source.clone(), ec) {
        Ok(reader_object) => reader_object,
        Err(error) => {
            reject_promise_with_error(&pipe_resolvers, error, ec);
            return Ok(pipe_promise_obj);
        }
    };
    let reader =
        match with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone()) {
            Ok(reader) => reader,
            Err(error) => {
                let reason = crate::webidl::error_to_rejection_reason(error, ec);
                reject_promise_with_error(&pipe_resolvers, reason, ec);
                return Ok(pipe_promise_obj);
            }
        };

    // Step 10: "Let writer be ! AcquireWritableStreamDefaultWriter(dest)."
    let writer_object = match super::acquire_writable_stream_default_writer(dest.clone(), ec) {
        Ok(writer_object) => writer_object,
        Err(error) => {
            if let Err(error) = readable_stream_default_reader_release(reader.clone(), ec) {
                error!("[readable-stream] failed to release reader on pipe setup error: {error:?}");
            }
            reject_promise_with_error(&pipe_resolvers, error, ec);
            return Ok(pipe_promise_obj);
        }
    };
    let writer =
        match super::with_writable_stream_default_writer_ref(&writer_object, ec, |writer| {
            writer.clone()
        }) {
            Ok(writer) => writer,
            Err(reason) => {
                if let Err(error) = readable_stream_default_reader_release(reader.clone(), ec) {
                    error!("[readable-stream] failed to release reader on writer error: {error:?}");
                }
                reject_promise_with_error(&pipe_resolvers, reason, ec);
                return Ok(pipe_promise_obj);
            }
        };

    // Step 11: "Set source.[[disturbed]] to true."
    source.set_disturbed(true);

    // Step 12: "Let shuttingDown be false."

    // Step 15: "In parallel but not really; see #905, using reader and writer, read all chunks from source and write them to dest."
    // Note: The pipe progress below follows a single typed state machine and advances from Boa promise reactions at each microtask.
    let state = PipeToState::new(PipeToStateInner {
        promise: pipe_promise_obj.clone(),
        reader,
        writer,
        pending_writes: VecDeque::new(),
        state: PipePumpState::Starting,
        prevent_close,
        prevent_abort,
        prevent_cancel,
        signal: signal.clone(),
        shutdown_error: None,
        shutdown_action_promise: None,
        resolvers: Some(pipe_resolvers),
        shutting_down: false,
    });

    // Step 14: "If signal is not undefined,"
    if let Some(signal) = signal {
        // Step 14.1: "Let abortAlgorithm be the following steps:"
        let abort_algorithm = SignalAbortAlgorithm::ReadableStreamPipeTo {
            state: state.clone(),
        };

        // Step 14.2: "If signal is aborted, perform abortAlgorithm and return promise."
        if signal.aborted_value() {
            if let Err(error) = state.run_abort_algorithm(ec) {
                state.reject_and_finalize_with_error(error, ec);
            }
            return Ok(state.promise());
        }

        // Step 14.3: "Add abortAlgorithm to signal."
        signal.add_abort_algorithm(abort_algorithm);
    }

    // Step 16: "Return promise."
    if let Err(error) = state.check_and_propagate_errors_forward(ec) {
        state.reject_and_finalize_with_error(error, ec);
        return Ok(state.promise());
    }
    if let Err(error) = state.check_and_propagate_errors_backward(ec) {
        state.reject_and_finalize_with_error(error, ec);
        return Ok(state.promise());
    }
    if let Err(error) = state.check_and_propagate_closing_forward(ec) {
        state.reject_and_finalize_with_error(error, ec);
        return Ok(state.promise());
    }
    if let Err(error) = state.check_and_propagate_closing_backward(ec) {
        state.reject_and_finalize_with_error(error, ec);
        return Ok(state.promise());
    }

    if state.is_shutting_down() {
        return Ok(state.promise());
    }

    if let Err(error) = state.wait_for_writer_ready(ec) {
        state.reject_and_finalize_with_error(error, ec);
    }

    Ok(state.promise())
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct PipeToState(GcCell<PipeToStateInner>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipePumpState {
    Starting,
    PendingReady,
    PendingRead,
    ShuttingDownWithPendingWrites(Option<PipeShutdownAction>),
    ShuttingDownPendingAction(PipeShutdownAction),
    Finalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeShutdownAction {
    AbortDestination,
    CancelSource,
    CloseDestination,
    Abort,
}

#[derive(Trace, Finalize)]
pub(crate) struct PipeToStateInner {
    promise: JsObject,
    reader: ReadableStreamDefaultReader,
    writer: super::WritableStreamDefaultWriter,
    pending_writes: VecDeque<JsObject>,

    #[unsafe_ignore_trace]
    state: PipePumpState,

    #[unsafe_ignore_trace]
    prevent_close: bool,

    #[unsafe_ignore_trace]
    prevent_abort: bool,

    #[unsafe_ignore_trace]
    prevent_cancel: bool,

    signal: Option<AbortSignal>,
    shutdown_error: Option<JsValue>,
    shutdown_action_promise: Option<JsObject>,
    resolvers: Option<js_engine::PromiseResolvers<crate::js::Types>>,

    #[unsafe_ignore_trace]
    shutting_down: bool,
}

impl PipeToState {
    fn new(state: PipeToStateInner) -> Self {
        Self(gc_cell_new(state))
    }

    fn borrow(&self) -> GcRef<'_, PipeToStateInner> {
        self.0.borrow()
    }

    fn borrow_mut(&self) -> GcRefMut<'_, PipeToStateInner> {
        self.0.borrow_mut()
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Gc::ptr_eq(&self.0, &other.0)
    }

    fn promise(&self) -> JsObject {
        self.borrow().promise.clone()
    }

    pub(crate) fn on_read_request_settled(
        &self,
        result: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        pipe_to_on_promise_settled(self.clone(), result, ec)
    }

    fn reject_and_finalize_with_error(
        &self,
        error: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {
        self.reject_and_finalize_with_reason(error, ec)
    }

    fn reject_and_finalize_with_reason(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) {
        self.set_shutdown_error(Some(reason));
        if let Err(error) = self.finalize(ec) {
            error!("[readable-stream] failed to finalize on rejection: {error:?}");
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    pub(crate) fn run_abort_algorithm(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let error = {
            let state = self.borrow();
            state
                .signal
                .as_ref()
                .map(AbortSignal::reason_value)
                .ok_or_else(|| {
                    ec.new_type_error(
                        "ReadableStreamPipeTo abort algorithm ran without an attached AbortSignal",
                    )
                })?
        };

        self.set_shutdown_error(Some(error));
        self.shutdown(Some(PipeShutdownAction::Abort), ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_for_writer_ready(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        self.set_state(PipePumpState::PendingReady);

        let (writer, reader) = {
            let state = self.borrow();
            (state.writer.clone(), state.reader.clone())
        };
        let ready_promise = writer.ready(ec)?;
        let reader_closed_promise = reader.closed(ec)?;

        if matches!(
            ec.promise_state(&ready_promise)?,
            js_engine::PromiseState::Fulfilled(_)
        ) {
            return self.read_chunk(ec);
        }

        self.append_reaction(ready_promise, ec)?;
        self.append_reaction(reader_closed_promise, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn read_chunk(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        self.set_state(PipePumpState::PendingRead);

        let (reader, writer) = {
            let state = self.borrow();
            (state.reader.clone(), state.writer.clone())
        };
        let read_request = ReadRequest::ReadableStreamPipeTo {
            state: self.clone(),
        };
        reader.read_with_request(read_request, ec)?;
        let writer_closed_promise = writer.closed(ec)?;

        self.append_reaction(writer_closed_promise, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn write_chunk(
        &self,
        result: JsValue,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<bool, crate::js::Types> {
        let Some(result_object) = result.as_object() else {
            return Ok(false);
        };

        if !ec.has_property(result_object.clone(), ec.property_key_from_str("done"))? {
            return Ok(false);
        }

        let done = EcmascriptHost::get(ec, &result_object, "done")?;
        if ec.to_boolean(&done) {
            return Ok(false);
        }

        let value = EcmascriptHost::get(ec, &result_object, "value")?;
        let writer = {
            let state = self.borrow();
            state.writer.clone()
        };
        let write_promise = writer.write(value, ec)?;
        self.borrow_mut().pending_writes.push_back(write_promise);
        Ok(true)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_on_pending_write(
        &self,
        promise: JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        self.append_reaction(promise, ec)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_forward(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (source, dest, prevent_abort) = {
            let state = self.borrow();
            (
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state.prevent_abort,
            )
        };
        let Some(source) = source else {
            return Ok(());
        };

        if source.state() != ReadableStreamState::Errored {
            return Ok(());
        }

        if !prevent_abort
            && dest
                .as_ref()
                .is_some_and(|dest| dest.state() == super::WritableStreamState::Erroring)
        {
            return Ok(());
        }

        self.set_shutdown_error(Some(source.stored_error()));
        if prevent_abort {
            self.shutdown(None, ec)
        } else {
            self.shutdown(Some(PipeShutdownAction::AbortDestination), ec)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_backward(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (dest, source, prevent_cancel) = {
            let state = self.borrow();
            (
                state.writer.stream_slot_value(),
                state.reader.stream_slot_value(),
                state.prevent_cancel,
            )
        };
        let Some(dest) = dest else {
            return Ok(());
        };

        if !matches!(
            dest.state(),
            super::WritableStreamState::Erroring | super::WritableStreamState::Errored
        ) {
            return Ok(());
        }

        self.set_shutdown_error(Some(dest.stored_error()));
        let should_cancel = !prevent_cancel
            && source
                .as_ref()
                .is_some_and(|source| source.state() == ReadableStreamState::Readable);
        if !should_cancel {
            self.shutdown(None, ec)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), ec)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_forward(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (source, prevent_close) = {
            let state = self.borrow();
            (state.reader.stream_slot_value(), state.prevent_close)
        };
        let Some(source) = source else {
            return Ok(());
        };

        if source.state() != ReadableStreamState::Closed {
            return Ok(());
        }

        if prevent_close {
            self.shutdown(None, ec)
        } else {
            self.shutdown(Some(PipeShutdownAction::CloseDestination), ec)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_backward(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.is_shutting_down() {
            return Ok(());
        }

        let (dest, source, prevent_cancel) = {
            let state = self.borrow();
            (
                state.writer.stream_slot_value(),
                state.reader.stream_slot_value(),
                state.prevent_cancel,
            )
        };
        let Some(dest) = dest else {
            return Ok(());
        };

        let source_is_readable = source
            .as_ref()
            .is_some_and(|source| source.state() == ReadableStreamState::Readable);

        if !source_is_readable {
            return Ok(());
        }

        if dest.state() != super::WritableStreamState::Closed && !dest.close_queued_or_in_flight() {
            return Ok(());
        }

        let error = type_error_value(
            "The destination WritableStream closed before the pipe operation completed",
            ec,
        )?;
        self.set_shutdown_error(Some(error));
        if prevent_cancel {
            self.shutdown(None, ec)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), ec)
        }
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    /// Note: This also covers <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown> when `action` is `None`.
    fn shutdown(
        &self,
        action: Option<PipeShutdownAction>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let pending_write = {
            let mut state = self.borrow_mut();
            if state.shutting_down {
                return Ok(());
            }

            state.shutting_down = true;

            let should_wait = state.writer.stream_slot_value().is_some_and(|dest| {
                dest.state() == super::WritableStreamState::Writable
                    && !dest.close_queued_or_in_flight()
                    && !state.pending_writes.is_empty()
            });
            if should_wait {
                state.state = PipePumpState::ShuttingDownWithPendingWrites(action);
                state.pending_writes.front().cloned()
            } else {
                None
            }
        };

        if let Some(pending_write) = pending_write {
            return self.wait_on_pending_write(pending_write, ec);
        }

        if let Some(action) = action {
            return self.perform_action(action, ec);
        }

        self.finalize(ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    fn perform_action(
        &self,
        action: PipeShutdownAction,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let (writer, source, dest, error, prevent_abort, prevent_cancel) = {
            let mut state = self.borrow_mut();
            state.state = PipePumpState::ShuttingDownPendingAction(action);
            (
                state.writer.clone(),
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state
                    .shutdown_error
                    .clone()
                    .unwrap_or_else(|| ec.value_undefined()),
                state.prevent_abort,
                state.prevent_cancel,
            )
        };

        let action_promise = match action {
            PipeShutdownAction::AbortDestination => match dest {
                Some(dest) => dest.abort_stream(error, ec)?,
                None => resolved_promise(ec.value_undefined(), ec)?,
            },
            PipeShutdownAction::CancelSource => match source {
                Some(source) => readable_stream_cancel(source, error, ec)?,
                None => resolved_promise(ec.value_undefined(), ec)?,
            },
            PipeShutdownAction::CloseDestination => match dest {
                Some(dest)
                    if dest.state() == super::WritableStreamState::Closed
                        || dest.close_queued_or_in_flight() =>
                {
                    resolved_promise(ec.value_undefined(), ec)?
                }
                _ => writer.close(ec)?,
            },
            PipeShutdownAction::Abort => {
                let abort_promise = if !prevent_abort {
                    match dest {
                        Some(dest) if dest.state() == super::WritableStreamState::Writable => {
                            Some(dest.abort_stream(error.clone(), ec)?)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                let cancel_source = if !prevent_cancel {
                    match source {
                        Some(source) if source.state() == ReadableStreamState::Readable => {
                            Some(source)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                match (abort_promise, cancel_source) {
                    (Some(abort_promise), Some(source)) => {
                        abort_destination_then_cancel_source(abort_promise, source, error, ec)?
                    }
                    (Some(abort_promise), None) => abort_promise,
                    (None, Some(source)) => readable_stream_cancel(source, error, ec)?,
                    (None, None) => resolved_promise(ec.value_undefined(), ec)?,
                }
            }
        };

        self.borrow_mut().shutdown_action_promise = Some(action_promise.clone());
        self.append_reaction(action_promise, ec)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-finalize>
    fn finalize(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        if self.current_state() == PipePumpState::Finalized {
            return Ok(());
        }

        let (writer, reader, signal, mut error, resolvers) = {
            let mut state = self.borrow_mut();
            state.state = PipePumpState::Finalized;
            (
                state.writer.clone(),
                state.reader.clone(),
                state.signal.clone(),
                state.shutdown_error.clone(),
                state.resolvers.take(),
            )
        };

        if let Err(release_error) = super::writable_stream_default_writer_release(writer, ec) {
            if error.is_none() {
                error = Some(release_error);
            }
        }
        if let Err(release_error) = super::readable_stream_default_reader_release(reader, ec) {
            if error.is_none() {
                error = Some(release_error);
            }
        }

        if let Some(signal) = signal {
            signal.remove_abort_algorithm(&SignalAbortAlgorithm::ReadableStreamPipeTo {
                state: self.clone(),
            });
        }

        if let Some(resolvers) = resolvers {
            let undefined = ec.value_undefined();
            match error {
                Some(error) => {
                    let reject: <crate::js::Types as JsTypes>::JsObject =
                        resolvers.reject.clone().into();
                    ec.call(&reject, &undefined, &[error])?;
                }
                None => {
                    let resolve: <crate::js::Types as JsTypes>::JsObject =
                        resolvers.resolve.clone().into();
                    ec.call(&resolve, &undefined, &[undefined.clone()])?;
                }
            }
        }

        Ok(())
    }

    fn current_state(&self) -> PipePumpState {
        self.borrow().state.clone()
    }

    fn set_state(&self, state: PipePumpState) {
        self.borrow_mut().state = state;
    }

    fn is_shutting_down(&self) -> bool {
        self.borrow().shutting_down
    }

    fn set_shutdown_error(&self, error: Option<JsValue>) {
        self.borrow_mut().shutdown_error = error;
    }

    fn update_pending_shutdown_action(
        &self,
        action: Option<PipeShutdownAction>,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<PipeShutdownAction>, crate::js::Types> {
        if action != Some(PipeShutdownAction::CloseDestination) {
            return Ok(action);
        }

        let (source, dest, prevent_cancel) = {
            let state = self.borrow();
            (
                state.reader.stream_slot_value(),
                state.writer.stream_slot_value(),
                state.prevent_cancel,
            )
        };

        let Some(dest) = dest else {
            return Ok(action);
        };

        let source_is_readable = source
            .as_ref()
            .is_some_and(|source| source.state() == ReadableStreamState::Readable);

        if matches!(
            dest.state(),
            super::WritableStreamState::Erroring | super::WritableStreamState::Errored
        ) {
            self.set_shutdown_error(Some(dest.stored_error()));
            return Ok(
                (!prevent_cancel && source_is_readable).then_some(PipeShutdownAction::CancelSource)
            );
        }

        if dest.state() == super::WritableStreamState::Closed || dest.close_queued_or_in_flight() {
            if !source_is_readable {
                return Ok(None);
            }

            let error = type_error_value(
                "The destination WritableStream closed before the pipe operation completed",
                ec,
            )?;
            self.set_shutdown_error(Some(error));
            return Ok(
                (!prevent_cancel && source_is_readable).then_some(PipeShutdownAction::CancelSource)
            );
        }

        Ok(action)
    }

    fn pending_write_front(&self) -> Option<JsObject> {
        self.borrow().pending_writes.front().cloned()
    }

    fn shutdown_action_promise_state(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Option<js_engine::PromiseState<crate::js::Types>>, crate::js::Types> {
        self.borrow()
            .shutdown_action_promise
            .clone()
            .map(|promise| Ok(ec.promise_state(&promise)?))
            .transpose()
    }

    fn prune_settled_pending_writes(
        &self,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let mut handled = Vec::new();
        {
            let mut state = self.borrow_mut();
            state.pending_writes.retain(|promise_object| {
                let ok = <crate::js::Types as JsTypes>::object_as_promise(promise_object).is_some();
                if !ok {
                    debug_assert!(false, "pipeTo tracked a non-promise write handle");
                    return false;
                }
                let pending = matches!(
                    ec.promise_state(promise_object),
                    Ok(js_engine::PromiseState::Pending)
                );
                if !pending {
                    handled.push(promise_object.clone());
                }
                pending
            });
        }

        for promise in handled {
            crate::webidl::mark_promise_as_handled(&promise, ec)?;
        }

        Ok(())
    }

    fn append_reaction(
        &self,
        promise: JsObject,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<(), crate::js::Types> {
        let state = self.clone();
        let on_fulfilled = ec.create_builtin_function(
            Box::new(
                move |_args: &[<crate::js::Types as JsTypes>::JsValue], _this, inner_ec| {
                    pipe_to_on_promise_settled(
                        state.clone(),
                        _args.get_or_undefined(0).clone(),
                        inner_ec,
                    )?;
                    Ok(inner_ec.value_undefined())
                },
            ),
            0,
            ec.property_key_from_str(""),
        );
        let state = self.clone();
        let on_rejected = ec.create_builtin_function(
            Box::new(
                move |_args: &[<crate::js::Types as JsTypes>::JsValue], _this, inner_ec| {
                    pipe_to_on_promise_settled(
                        state.clone(),
                        _args.get_or_undefined(0).clone(),
                        inner_ec,
                    )?;
                    Ok(inner_ec.value_undefined())
                },
            ),
            0,
            ec.property_key_from_str(""),
        );
        let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&promise)
            .ok_or_else(|| ec.new_type_error("not a promise"))?;
        ec.perform_promise_then(js_promise, Some(on_fulfilled), Some(on_rejected), None)?;
        Ok(())
    }
}

#[derive(Trace, Finalize)]
struct AbortThenCancelState {
    source: Option<ReadableStream>,
    error: JsValue,
    abort_rejection: Option<JsValue>,
    resolvers: PromiseResolvers<crate::js::Types>,
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_to_on_promise_settled(
    state: PipeToState,
    result: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    state.prune_settled_pending_writes(ec)?;

    let state_before_checks = state.current_state();

    if state_before_checks == PipePumpState::PendingRead {
        let (source, dest) = {
            let state_ref = state.borrow();
            (
                state_ref.reader.stream_slot_value(),
                state_ref.writer.stream_slot_value(),
            )
        };

        if let Some(source) = source {
            if source.state() == ReadableStreamState::Closed {
                if let Some(dest) = dest {
                    if dest.state() == super::WritableStreamState::Writable
                        && !dest.close_queued_or_in_flight()
                    {
                        let Some(done) = pipe_read_result_done(&result, ec)? else {
                            return Ok(());
                        };

                        if !done {
                            let _ = state.write_chunk(result.clone(), ec)?;
                        }
                    }
                }
            }
        }
    }

    state.check_and_propagate_errors_forward(ec)?;
    state.check_and_propagate_errors_backward(ec)?;
    state.check_and_propagate_closing_forward(ec)?;
    state.check_and_propagate_closing_backward(ec)?;

    let current_state = state.current_state();
    if current_state != state_before_checks {
        return Ok(());
    }

    match current_state {
        PipePumpState::Starting => {
            debug_assert!(
                false,
                "ReadableStream pipeTo callback reached the Starting state"
            );
        }
        PipePumpState::PendingReady => {
            state.read_chunk(ec)?;
        }
        PipePumpState::PendingRead => {
            let _ = state.write_chunk(result, ec)?;
            if state.is_shutting_down() {
                return Ok(());
            }
            state.wait_for_writer_ready(ec)?;
        }
        PipePumpState::ShuttingDownWithPendingWrites(action) => {
            let action = state.update_pending_shutdown_action(action, ec)?;
            state.set_state(PipePumpState::ShuttingDownWithPendingWrites(action));

            if let Some(pending_write) = state.pending_write_front() {
                state.wait_on_pending_write(pending_write, ec)?;
            } else if let Some(action) = action {
                state.perform_action(action, ec)?;
            } else {
                state.finalize(ec)?;
            }
        }
        PipePumpState::ShuttingDownPendingAction(action) => {
            match state.shutdown_action_promise_state(ec)? {
                Some(js_engine::PromiseState::Pending) => return Ok(()),
                Some(js_engine::PromiseState::Rejected(error)) => {
                    state.set_shutdown_error(Some(error))
                }
                Some(js_engine::PromiseState::Fulfilled(value)) => {
                    if action != PipeShutdownAction::Abort && !value.is_undefined() {
                        state.set_shutdown_error(Some(value));
                    }
                }
                None => {}
            }

            state.finalize(ec)?;
        }
        PipePumpState::Finalized => {}
    }

    Ok(())
}

fn pipe_read_result_done(
    result: &<crate::js::Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<bool>, crate::js::Types> {
    let Some(result_object) = result.as_object() else {
        return Ok(None);
    };

    if !ec.has_property(result_object.clone(), ec.property_key_from_str("done"))? {
        return Ok(None);
    }

    let done = EcmascriptHost::get(ec, &result_object, "done")?;
    Ok(Some(ec.to_boolean(&done)))
}

fn start_abort_cancel_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    state: &GcCell<AbortThenCancelState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    finalize_abort_cancel_source(state.clone(), None, ec)
}

fn start_abort_cancel_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    state: &GcCell<AbortThenCancelState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    finalize_abort_cancel_source(state.clone(), Some(args.get_or_undefined(0).clone()), ec)
}

fn abort_destination_then_cancel_on_fulfilled_fn(
    _args: &[JsValue],
    _this: JsValue,
    state: &GcCell<AbortThenCancelState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    start_abort_cancel_source(state.clone(), None, ec)
}

fn abort_destination_then_cancel_on_rejected_fn(
    args: &[JsValue],
    _this: JsValue,
    state: &GcCell<AbortThenCancelState>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    start_abort_cancel_source(state.clone(), Some(args.get_or_undefined(0).clone()), ec)
}

fn abort_destination_then_cancel_source(
    abort_promise: JsObject,
    source: ReadableStream,
    error: JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let (promise, resolvers) = ec.new_promise_pending()?;
    let promise_obj = promise
        .as_object()
        .map(|o| o.clone())
        .unwrap_or_else(|| ec.realm_global_object());
    let state = gc_cell_new(AbortThenCancelState {
        source: Some(source),
        error,
        abort_rejection: None,
        resolvers,
    });

    let on_fulfilled = crate::js::builtin_with_captures(
        ec,
        state.clone(),
        abort_destination_then_cancel_on_fulfilled_fn,
        0,
    );
    let on_rejected = crate::js::builtin_with_captures(
        ec,
        state.clone(),
        abort_destination_then_cancel_on_rejected_fn,
        1,
    );
    let js_promise = <crate::js::Types as JsTypes>::object_as_promise(&abort_promise)
        .ok_or_else(|| ec.new_type_error("abort_promise is not a Promise"))?;
    ec.perform_promise_then(js_promise, Some(on_fulfilled), Some(on_rejected), None)?;

    Ok(promise_obj)
}

fn start_abort_cancel_source(
    state: GcCell<AbortThenCancelState>,
    abort_rejection: Option<JsValue>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (source, error) = {
        let mut state_ref = state.borrow_mut();
        state_ref.abort_rejection = abort_rejection;
        (state_ref.source.take(), state_ref.error.clone())
    };

    let cancel_promise = match source {
        Some(source) => readable_stream_cancel(source, error, ec)?,
        None => resolved_promise(ec.value_undefined(), ec)?,
    };

    let on_fulfilled =
        crate::js::builtin_with_captures(ec, state.clone(), start_abort_cancel_on_fulfilled_fn, 0);
    let on_rejected =
        crate::js::builtin_with_captures(ec, state, start_abort_cancel_on_rejected_fn, 1);

    let promise = <crate::js::Types as JsTypes>::object_as_promise(&cancel_promise)
        .ok_or_else(|| ec.new_type_error("cancel_promise is not a Promise"))?;
    ec.perform_promise_then(promise, Some(on_fulfilled), Some(on_rejected), None)?;
    Ok(ec.value_undefined())
}

fn finalize_abort_cancel_source(
    state: GcCell<AbortThenCancelState>,
    cancel_rejection: Option<JsValue>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (abort_rejection, resolvers) = {
        let state_ref = state.borrow();
        (
            state_ref.abort_rejection.clone(),
            state_ref.resolvers.clone(),
        )
    };

    let undefined = ec.value_undefined();
    if let Some(reason) = abort_rejection.or(cancel_rejection) {
        let reject: JsObject = resolvers.reject.clone().into();
        ec.call(&reject, &undefined, &[reason])?;
    } else {
        let resolve: JsObject = resolvers.resolve.clone().into();
        ec.call(&resolve, &undefined, &[undefined.clone()])?;
    }

    Ok(JsValue::undefined())
}
