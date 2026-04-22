use std::{cell::{Cell, RefCell}, rc::Rc};

use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    class::Class,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::{JsArray, JsFunction, JsPromise}},
};
use boa_gc::{Finalize, Gc, GcRef, GcRefCell, GcRefMut, Trace};

use crate::boa::with_abort_signal_ref;
use crate::dom::{AbortAlgorithm as SignalAbortAlgorithm, AbortSignal};
use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::{
    mark_promise_as_handled, rejected_promise, resolved_promise,
    transform_promise_to_undefined,
};

use super::{
    CancelAlgorithm, PullAlgorithm, ReadableStreamController, ReadableStreamReader,
    ReadableStreamState, SourceMethod, StartAlgorithm,
    ReadableStreamDefaultReader, ReadableStreamGenericReader, ReadRequest,
    acquire_readable_stream_default_reader,
    readable_stream_default_reader_error_read_requests, rejected_type_error_promise,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source,
    type_error_value, with_readable_stream_default_reader_ref,
};

/// <https://streams.spec.whatwg.org/#rs-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStream {
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#readablestream-controller>
    controller: Gc<GcRefCell<Option<ReadableStreamController>>>,

    controller_object: Gc<GcRefCell<Option<JsObject>>>,

    /// <https://streams.spec.whatwg.org/#readablestream-reader>
    reader: Gc<GcRefCell<Option<ReadableStreamReader>>>,

    /// <https://streams.spec.whatwg.org/#readablestream-disturbed>
    #[unsafe_ignore_trace]
    disturbed: Rc<Cell<bool>>,

    /// <https://streams.spec.whatwg.org/#readablestream-state>
    #[unsafe_ignore_trace]
    state: Rc<RefCell<ReadableStreamState>>,

    /// <https://streams.spec.whatwg.org/#readablestream-storederror>
    stored_error: Gc<GcRefCell<JsValue>>,
}

impl ReadableStream {
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            controller: Gc::new(GcRefCell::new(None)),
            controller_object: Gc::new(GcRefCell::new(None)),
            reader: Gc::new(GcRefCell::new(None)),
            disturbed: Rc::new(Cell::new(false)),
            state: Rc::new(RefCell::new(ReadableStreamState::Readable)),
            stored_error: Gc::new(GcRefCell::new(JsValue::undefined())),
        }
    }

    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream is missing its JavaScript object")
                .into()
        })
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
    pub(crate) fn cancel(&mut self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.is_readable_stream_locked() {
            return rejected_type_error_promise(
                "Cannot cancel a stream that already has a reader",
                context,
            );
        }

        // Step 2: "Return ! ReadableStreamCancel(this, reason)."
        readable_stream_cancel(self.clone(), reason, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-get-reader>
    pub(crate) fn get_reader(
        &mut self,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        let options_object = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(options.to_object(context)?)
        };

        // Step 1: "If options[\"mode\"] does not exist, return ? AcquireReadableStreamDefaultReader(this)."
        let Some(options_object) = options_object else {
            return acquire_readable_stream_default_reader(self.clone(), context);
        };

        if !options_object.has_property(js_string!("mode"), context)? {
            return acquire_readable_stream_default_reader(self.clone(), context);
        }

        let mode = options_object.get(js_string!("mode"), context)?;
        if mode.is_undefined() {
            return acquire_readable_stream_default_reader(self.clone(), context);
        }

        // Step 2: "Assert: options[\"mode\"] is \"byob\"."
        let mode = mode.to_string(context)?.to_std_string_escaped();
        if mode != "byob" {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.getReader() only supports the default reader mode")
                .into());
        }

        // Step 3: "Return ? AcquireReadableStreamBYOBReader(this)."
        // TODO: Implement `ReadableStreamBYOBReader`.
        Err(JsNativeError::typ()
            .with_message("ReadableStreamBYOBReader is not implemented yet")
            .into())
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-through>
    pub(crate) fn pipe_through(
        &mut self,
        transform: &JsValue,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, throw a TypeError exception."
        if self.locked() {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough() called on a locked stream")
                .into());
        }

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        let transform_obj = transform.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough() requires a ReadableWritablePair")
        })?;
        let writable_value = transform_obj.get(js_string!("writable"), context)?;
        let writable_obj = writable_value.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableWritablePair is missing its writable property")
        })?;
        let writable_locked = super::with_writable_stream_ref(&writable_obj, |ws| ws.locked())?;
        if writable_locked {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.pipeThrough(): destination writable stream is locked")
                .into());
        }

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        let options_object = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(options.to_object(context)?)
        };

        let signal = extract_abort_signal(options_object.as_ref(), context)?;

        let readable_value = transform_obj.get(js_string!("readable"), context)?;

        // Step 4: "Let promise be ! ReadableStreamPipeTo(this, transform[\"writable\"], options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        let prevent_close = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventClose"), context)?
                .to_boolean(),
            None => false,
        };

        let prevent_abort = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventAbort"), context)?
                .to_boolean(),
            None => false,
        };

        let prevent_cancel = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventCancel"), context)?
                .to_boolean(),
            None => false,
        };

        let destination = super::with_writable_stream_ref(&writable_obj, |ws| ws.clone())?;
        let promise = readable_stream_pipe_to(
            self.clone(),
            destination,
            prevent_close,
            prevent_abort,
            prevent_cancel,
            signal,
            context,
        )?;

        // Step 5: "Set promise.[[PromiseIsHandled]] to true."
        crate::webidl::mark_promise_as_handled(&promise, context)?;

        // Step 6: "Return transform[\"readable\"]."
        Ok(readable_value)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-to>
    pub(crate) fn pipe_to(
        &mut self,
        destination: &JsValue,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        if self.locked() {
            return rejected_type_error_promise(
                "ReadableStream.pipeTo() called on a locked stream",
                context,
            );
        }

        // Step 2: "If ! IsWritableStreamLocked(destination) is true, return a promise rejected with a TypeError exception."
        let dest_obj = match destination.as_object() {
            Some(obj) => obj.clone(),
            None => {
                return rejected_type_error_promise(
                    "ReadableStream.pipeTo() requires a WritableStream destination",
                    context,
                );
            }
        };
        let dest_locked = super::with_writable_stream_ref(&dest_obj, |ws| ws.locked())?;
        if dest_locked {
            return rejected_type_error_promise(
                "ReadableStream.pipeTo(): destination is locked",
                context,
            );
        }

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        let options_object = if options.is_undefined() || options.is_null() {
            None
        } else {
            Some(options.to_object(context)?)
        };

        let signal = match extract_abort_signal(options_object.as_ref(), context) {
            Ok(signal) => signal,
            Err(error) => return rejected_promise(error.into_opaque(context)?, context),
        };

        // Step 4: "Return ! ReadableStreamPipeTo(this, destination, options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        let prevent_close = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventClose"), context)?
                .to_boolean(),
            None => false,
        };

        let prevent_abort = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventAbort"), context)?
                .to_boolean(),
            None => false,
        };

        let prevent_cancel = match options_object.as_ref() {
            Some(options_object) => options_object
                .get(js_string!("preventCancel"), context)?
                .to_boolean(),
            None => false,
        };

        let dest = super::with_writable_stream_ref(&dest_obj, |ws| ws.clone())?;
        readable_stream_pipe_to(
            self.clone(),
            dest,
            prevent_close,
            prevent_abort,
            prevent_cancel,
            signal,
            context,
        )
    }

    /// <https://streams.spec.whatwg.org/#rs-tee>
    pub(crate) fn tee(&mut self, context: &mut Context) -> JsResult<JsValue> {
        // Step 1: "Return ? ReadableStreamTee(this, false)."
        readable_stream_tee(self.clone(), false, context)
    }

}
/// pull and cancel algorithms.
#[derive(Trace, Finalize)]
struct TeeState {
    source_stream: ReadableStream,
    reader: ReadableStreamDefaultReader,
    branch1: Option<ReadableStream>,
    branch2: Option<ReadableStream>,
    pull_function: Option<JsObject>,
    cancel_promise: JsObject,
    cancel_resolvers: boa_engine::builtins::promise::ResolvingFunctions,
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
/// <https://streams.spec.whatwg.org/#rs-constructor>
pub(crate) fn construct_readable_stream(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStream> {
    let mut stream = ReadableStream::new(None);

    // Step 1: "If underlyingSource is missing, set it to undefined."
    let underlying_source = if args.is_empty() {
        JsValue::undefined()
    } else {
        args[0].clone()
    };

    // Step 2: "Let underlyingSourceDict be underlyingSource, converted to an IDL value of type UnderlyingSource."
    // Note: The current runtime keeps the original JavaScript object so it can invoke the underlying source callbacks directly.
    let underlying_source_object = if underlying_source.is_undefined() {
        None
    } else {
        Some(underlying_source.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream underlyingSource must be an object")
        })?)
    };

    // Step 3: "Perform ! InitializeReadableStream(this)."
    // Note: `data_constructor` creates the native carrier before Boa allocates the wrapping
    // object, so this helper initializes the carrier and `object_constructor` wires the wrapper.
    stream.initialize_readable_stream();

    let strategy = args.get_or_undefined(1).clone();
    match underlying_source_type(underlying_source_object.as_ref(), context)?.as_deref() {
        Some("bytes") => {
            // Step 4.1: "If strategy[\"size\"] exists, throw a RangeError exception."
            if strategy_has_size(&strategy, context)? {
                return Err(JsNativeError::range()
                    .with_message("a byte stream strategy cannot include a size function")
                    .into());
            }

            // Step 4.2: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 0)."
            let _ = extract_high_water_mark(&strategy, 0.0, context)?;

            // Step 4.3: "Perform ? SetUpReadableByteStreamControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark)."
            // TODO: Implement `ReadableByteStreamController` and BYOB readers.
            return Err(JsNativeError::typ()
                .with_message("Readable byte streams are not implemented yet")
                .into());
        }
        Some(_) => {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream underlyingSource.type must be \"bytes\" when present")
                .into());
        }
        None => {}
    }

    // Step 5.1: "Assert: underlyingSourceDict[\"type\"] does not exist."
    debug_assert!(underlying_source_type(underlying_source_object.as_ref(), context)?.is_none());

    // Step 5.2: "Let sizeAlgorithm be ! ExtractSizeAlgorithm(strategy)."
    let size_algorithm = extract_size_algorithm(&strategy, context)?;

    // Step 5.3: "Let highWaterMark be ? ExtractHighWaterMark(strategy, 1)."
    let high_water_mark = extract_high_water_mark(&strategy, 1.0, context)?;

    // Step 5.4: "Perform ? SetUpReadableStreamDefaultControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller_from_underlying_source(
        stream.clone(),
        underlying_source_object,
        high_water_mark,
        size_algorithm,
        context,
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
    context: &mut Context,
) -> JsResult<ReadableStream> {
    // Step 1: "If highWaterMark was not passed, set it to 1."
    let high_water_mark = high_water_mark.unwrap_or(1.0);

    // Step 2: "If sizeAlgorithm was not passed, set it to an algorithm that returns 1."
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);

    // Step 3: "Assert: ! IsNonNegativeNumber(highWaterMark) is true."
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    // Step 4: "Let stream be a new ReadableStream."
    let mut stream = create_readable_stream_object(context)?;

    // Step 5: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 6: "Let controller be a new ReadableStreamDefaultController."
    let controller = super::ReadableStreamDefaultController::new();
    let controller_object = super::ReadableStreamDefaultController::from_data(controller.clone(), context)?;

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
        context,
    )?;

    // Step 8: "Return stream."
    Ok(stream)
}
fn create_readable_stream_object(context: &mut Context) -> JsResult<ReadableStream> {
    let stream = ReadableStream::new(None);
    let stream_object = ReadableStream::from_data(stream.clone(), context)?;
    stream.set_reflector(stream_object);
    Ok(stream)
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

/// <https://streams.spec.whatwg.org/#readable-stream-cancel>
pub(crate) fn readable_stream_cancel(
    stream: ReadableStream,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Set stream.[[disturbed]] to true."
    stream.set_disturbed(true);

    // Step 2: "If stream.[[state]] is \"closed\", return a promise resolved with undefined."
    if stream.state() == ReadableStreamState::Closed {
        return resolved_promise(JsValue::undefined(), context);
    }

    // Step 3: "If stream.[[state]] is \"errored\", return a promise rejected with stream.[[storedError]]."
    if stream.state() == ReadableStreamState::Errored {
        return rejected_promise(stream.stored_error(), context);
    }

    // Step 4: "Perform ! ReadableStreamClose(stream)."
    readable_stream_close(stream.clone(), context)?;

    // Step 5: "Let reader be stream.[[reader]]."
    let reader = stream.reader_slot();

    // Step 6: "If reader is not undefined and reader implements ReadableStreamBYOBReader,"
    // Note: The current runtime does not yet implement `ReadableStreamBYOBReader`, so there are no BYOB read-into requests to clear here.
    if let Some(reader) = reader {
        debug_assert!(reader.is_default_reader());
    }

    // Step 7: "Let sourceCancelPromise be ! stream.[[controller]].[[CancelSteps]](reason)."
    let controller = stream.controller_slot().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream is missing its controller")
    })?;
    let source_cancel_promise = controller.cancel_steps(reason, context)?;

    // Step 8: "Return the result of reacting to sourceCancelPromise with a fulfillment step that returns undefined."
    transform_promise_to_undefined(&source_cancel_promise, context)
}

/// <https://streams.spec.whatwg.org/#readable-stream-close>
pub(crate) fn readable_stream_close(stream: ReadableStream, context: &mut Context) -> JsResult<()> {
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

    let Some(reader) = reader.as_default_reader() else {
        return Ok(());
    };

    // Step 5: "Resolve reader.[[closedPromise]] with undefined."
    if let Some(resolvers) = reader.closed_resolvers_slot_value() {
        resolvers
            .resolve
            .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
        reader.set_closed_resolvers_slot_value(None);
    }

    // Step 6.1: "Let readRequests be reader.[[readRequests]]."
    let read_requests = reader.take_read_requests();

    // Step 6.2: "Set reader.[[readRequests]] to an empty list."
    // Note: `take_read_requests()` empties the list before the requests are processed.

    // Step 6.3: "For each readRequest of readRequests,"
    for read_request in read_requests {
        // Step 6.3.1: "Perform readRequest's close steps."
        read_request.close_steps(context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-error>
pub(crate) fn readable_stream_error(
    stream: ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
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

    let Some(reader) = reader.as_default_reader() else {
        return Ok(());
    };

    // Step 6: "Reject reader.[[closedPromise]] with e."
    if let Some(resolvers) = reader.closed_resolvers_slot_value() {
        resolvers
            .reject
            .call(&JsValue::undefined(), &[error.clone()], context)?;
        reader.set_closed_resolvers_slot_value(None);
    }

    // Step 7: "Set reader.[[closedPromise]].[[PromiseIsHandled]] to true."
    if let Some(closed_promise) = reader.closed_promise_slot_value() {
        mark_promise_as_handled(&closed_promise, context)?;
    }

    // Step 8.1: "Perform ! ReadableStreamDefaultReaderErrorReadRequests(reader, e)."
    readable_stream_default_reader_error_read_requests(reader, error, context)
}

/// <https://streams.spec.whatwg.org/#readable-stream-add-read-request>
pub(crate) fn readable_stream_add_read_request(
    stream: ReadableStream,
    read_request: ReadRequest,
) -> JsResult<()> {
    // Step 1: "Assert: stream.[[reader]] implements ReadableStreamDefaultReader."
    let reader = stream.reader_slot().and_then(|reader| reader.as_default_reader()).ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream is not locked to a default reader")
    })?;

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
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Assert: ! ReadableStreamHasDefaultReader(stream) is true."
    let reader = stream.reader_slot().and_then(|reader| reader.as_default_reader()).ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream is not locked to a default reader")
    })?;

    // Step 2: "Let reader be stream.[[reader]]."

    // Step 3: "Assert: reader.[[readRequests]] is not empty."
    debug_assert!(reader.read_requests_len() > 0);

    // Step 4: "Let readRequest be reader.[[readRequests]][0]."
    // Step 5: "Remove readRequest from reader.[[readRequests]]."
    let read_request = reader.shift_read_request().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream has no pending read request")
    })?;

    // Step 6: "If done is true, perform readRequest's close steps."
    if done {
        return read_request.close_steps(context);
    }

    // Step 7: "Otherwise, perform readRequest's chunk steps, given chunk."
    read_request.chunk_steps(chunk, context)
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

/// <https://streams.spec.whatwg.org/#readable-stream-tee>
fn readable_stream_tee(
    stream: ReadableStream,
    _clone_for_branch2: bool,
    context: &mut Context,
) -> JsResult<JsValue> {
    let reader_object = acquire_readable_stream_default_reader(stream.clone(), context)?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone())?;
    let (cancel_promise, cancel_resolvers) = JsPromise::new_pending(context);
    let tee_state = Gc::new(GcRefCell::new(TeeState {
        source_stream: stream,
        reader,
        branch1: None,
        branch2: None,
        pull_function: None,
        cancel_promise: cancel_promise.into(),
        cancel_resolvers,
        reading: false,
        read_again: false,
        canceled1: false,
        canceled2: false,
        reason1: JsValue::undefined(),
        reason2: JsValue::undefined(),
    }));

    let pull_function = NativeFunction::from_copy_closure_with_captures(
        |_, _: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, context| {
            {
                let mut tee_state = tee_state.borrow_mut();
                if tee_state.reading {
                    tee_state.read_again = true;
                    return Ok(JsValue::undefined());
                }
                tee_state.reading = true;
            }

            let reader = tee_state.borrow().reader.clone();
            let read_promise = reader.read(context)?;

            let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
                |_, args: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, context| {
                    let result = args.get_or_undefined(0).to_object(context)?;
                    let done = result.get(js_string!("done"), context)?.to_boolean();

                    if done {
                        let (branch1, branch2, canceled1, canceled2, cancel_resolvers) = {
                            let mut tee_state = tee_state.borrow_mut();
                            tee_state.reading = false;
                            (
                                tee_state.branch1.clone(),
                                tee_state.branch2.clone(),
                                tee_state.canceled1,
                                tee_state.canceled2,
                                tee_state.cancel_resolvers.clone(),
                            )
                        };

                        if !canceled1 {
                            if let Some(branch1) = branch1 {
                                if let Some(mut controller) = branch1
                                    .controller_slot()
                                    .map(|controller| controller.as_default_controller())
                                {
                                    controller.close(context)?;
                                }
                            }
                        }
                        if !canceled2 {
                            if let Some(branch2) = branch2 {
                                if let Some(mut controller) = branch2
                                    .controller_slot()
                                    .map(|controller| controller.as_default_controller())
                                {
                                    controller.close(context)?;
                                }
                            }
                        }
                        if !canceled1 || !canceled2 {
                            cancel_resolvers.resolve.call(
                                &JsValue::undefined(),
                                &[JsValue::undefined()],
                                context,
                            )?;
                        }

                        return Ok(JsValue::undefined());
                    }

                    let value = result.get(js_string!("value"), context)?;
                    let on_chunk = NativeFunction::from_copy_closure_with_captures(
                        |_, _, captures: &(Gc<GcRefCell<TeeState>>, JsValue), context| {
                            let (tee_state, value) = captures;
                            {
                                let mut tee_state = tee_state.borrow_mut();
                                tee_state.read_again = false;
                            }

                            let (branch1, branch2, canceled1, canceled2, pull_function) = {
                                let tee_state = tee_state.borrow();
                                (
                                    tee_state.branch1.clone(),
                                    tee_state.branch2.clone(),
                                    tee_state.canceled1,
                                    tee_state.canceled2,
                                    tee_state.pull_function.clone(),
                                )
                            };

                            let enqueue_result: JsResult<()> = (|| {
                                if !canceled1 {
                                    if let Some(branch1) = branch1 {
                                        if let Some(controller) = branch1
                                            .controller_slot()
                                            .map(|controller| controller.as_default_controller())
                                        {
                                            controller.enqueue(value.clone(), context)?;
                                        }
                                    }
                                }

                                if !canceled2 {
                                    if let Some(branch2) = branch2 {
                                        if let Some(controller) = branch2
                                            .controller_slot()
                                            .map(|controller| controller.as_default_controller())
                                        {
                                            controller.enqueue(value.clone(), context)?;
                                        }
                                    }
                                }

                                Ok(())
                            })();

                            let should_read_again = {
                                let mut tee_state = tee_state.borrow_mut();
                                tee_state.reading = false;
                                let should_read_again = tee_state.read_again;
                                tee_state.read_again = false;
                                should_read_again
                            };

                            enqueue_result?;

                            if should_read_again {
                                if let Some(pull_function) = pull_function {
                                    let pull_function = JsFunction::from_object(pull_function).ok_or_else(|| {
                                        JsNativeError::typ().with_message("tee pull algorithm is not callable")
                                    })?;
                                    let _ = pull_function.call(&JsValue::undefined(), &[], context)?;
                                }
                            }

                            Ok(JsValue::undefined())
                        },
                        (tee_state.clone(), value),
                    )
                    .to_js_function(context.realm());
                    let microtask = resolved_promise(JsValue::undefined(), context)?;
                    let _ = JsPromise::from_object(microtask)?.then(Some(on_chunk), None, context)?;
                    Ok(JsValue::undefined())
                },
                tee_state.clone(),
            )
            .to_js_function(context.realm());
            let on_rejected = NativeFunction::from_copy_closure_with_captures(
                |_, _: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, _: &mut Context| {
                    let mut tee_state = tee_state.borrow_mut();
                    tee_state.reading = false;
                    Ok(JsValue::undefined())
                },
                tee_state.clone(),
            )
            .to_js_function(context.realm());

            let _ = JsPromise::from_object(read_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;
            Ok(JsValue::undefined())
        },
        tee_state.clone(),
    )
    .to_js_function(context.realm());

    let cancel1_function = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, context| {
            let reason = args.get_or_undefined(0).clone();
            let (
                source_stream,
                cancel_promise,
                canceled2,
                reason1,
                reason2,
                cancel_resolvers,
            ) = {
                let mut tee_state = tee_state.borrow_mut();
                tee_state.canceled1 = true;
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

            if canceled2 {
                let composite_reason = JsArray::from_iter(
                    [reason1, reason2].into_iter().map(JsValue::from),
                    context,
                );
                let cancel_result = readable_stream_cancel(
                    source_stream,
                    JsValue::from(composite_reason),
                    context,
                )?;
                cancel_resolvers.resolve.call(
                    &JsValue::undefined(),
                    &[JsValue::from(cancel_result)],
                    context,
                )?;
            }

            Ok(JsValue::from(cancel_promise))
        },
        tee_state.clone(),
    )
    .to_js_function(context.realm());

    let cancel2_function = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, context| {
            let reason = args.get_or_undefined(0).clone();
            let (
                source_stream,
                cancel_promise,
                canceled1,
                reason1,
                reason2,
                cancel_resolvers,
            ) = {
                let mut tee_state = tee_state.borrow_mut();
                tee_state.canceled2 = true;
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

            if canceled1 {
                let composite_reason = JsArray::from_iter(
                    [reason1, reason2].into_iter().map(JsValue::from),
                    context,
                );
                let cancel_result = readable_stream_cancel(
                    source_stream,
                    JsValue::from(composite_reason),
                    context,
                )?;
                cancel_resolvers.resolve.call(
                    &JsValue::undefined(),
                    &[JsValue::from(cancel_result)],
                    context,
                )?;
            }

            Ok(JsValue::from(cancel_promise))
        },
        tee_state.clone(),
    )
    .to_js_function(context.realm());

    let branch1 = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::JavaScript(SourceMethod::new(
            context.global_object(),
            pull_function.clone().into(),
        )),
        CancelAlgorithm::JavaScript(SourceMethod::new(
            context.global_object(),
            cancel1_function.into(),
        )),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        context,
    )?;
    let branch2 = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::JavaScript(SourceMethod::new(
            context.global_object(),
            pull_function.clone().into(),
        )),
        CancelAlgorithm::JavaScript(SourceMethod::new(
            context.global_object(),
            cancel2_function.into(),
        )),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        context,
    )?;

    {
        let mut tee_state = tee_state.borrow_mut();
        tee_state.branch1 = Some(branch1.clone());
        tee_state.branch2 = Some(branch2.clone());
        tee_state.pull_function = Some(pull_function.into());
    }

    let reader_closed_promise = tee_state.borrow().reader.closed()?;
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, context| {
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

            if !canceled1 {
                if let Some(branch1) = branch1 {
                    if let Some(mut controller) = branch1
                        .controller_slot()
                        .map(|controller| controller.as_default_controller())
                    {
                        controller.error(error.clone(), context)?;
                    }
                }
            }
            if !canceled2 {
                if let Some(branch2) = branch2 {
                    if let Some(mut controller) = branch2
                        .controller_slot()
                        .map(|controller| controller.as_default_controller())
                    {
                        controller.error(error, context)?;
                    }
                }
            }
            if !canceled1 || !canceled2 {
                cancel_resolvers.resolve.call(
                    &JsValue::undefined(),
                    &[JsValue::undefined()],
                    context,
                )?;
            }

            Ok(JsValue::undefined())
        },
        tee_state,
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(reader_closed_promise)?.catch(on_rejected, context)?;

    Ok(JsArray::from_iter(
        [branch1.object()?, branch2.object()?]
            .into_iter()
            .map(JsValue::from),
        context,
    )
    .into())
}
/// invocation.
fn underlying_source_type(
    source_object: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<String>> {
    let Some(source_object) = source_object else {
        return Ok(None);
    };

    if !source_object.has_property(js_string!("type"), context)? {
        return Ok(None);
    }

    let value = source_object.get(js_string!("type"), context)?;
    if value.is_undefined() {
        return Ok(None);
    }

    Ok(Some(value.to_string(context)?.to_std_string_escaped()))
}
fn strategy_has_size(strategy: &JsValue, context: &mut Context) -> JsResult<bool> {
    if strategy.is_undefined() || strategy.is_null() {
        return Ok(false);
    }

    let strategy = strategy.to_object(context)?;
    if !strategy.has_property(js_string!("size"), context)? {
        return Ok(false);
    }

    Ok(!strategy.get(js_string!("size"), context)?.is_undefined())
}

fn extract_abort_signal(
    options_object: Option<&JsObject>,
    context: &mut Context,
) -> JsResult<Option<AbortSignal>> {
    let Some(options_object) = options_object else {
        return Ok(None);
    };

    if !options_object.has_property(js_string!("signal"), context)? {
        return Ok(None);
    }

    let signal = options_object.get(js_string!("signal"), context)?;
    if signal.is_undefined() {
        return Ok(None);
    }

    if signal.is_null() {
        return Err(JsNativeError::typ()
            .with_message("ReadableStream pipe options.signal must be an AbortSignal")
            .into());
    }

    let signal_object = signal.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream pipe options.signal must be an AbortSignal")
    })?;

    with_abort_signal_ref(&signal_object, |signal| signal.clone()).map(Some)
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn readable_stream_pipe_to(
    source: ReadableStream,
    dest: super::WritableStream,
    prevent_close: bool,
    prevent_abort: bool,
    prevent_cancel: bool,
    signal: Option<AbortSignal>,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Assert: source implements ReadableStream."

    // Step 2: "Assert: dest implements WritableStream."

    // Step 3: "Assert: preventClose, preventAbort, and preventCancel are all booleans."

    // Step 4: "If signal was not given, let signal be undefined."

    // Step 5: "Assert: either signal is undefined, or signal implements AbortSignal."
    // Note: `pipe_to()` and `pipe_through()` normalize the Web IDL carrier to `Option<AbortSignal>` before calling this helper.

    // Step 6: "Assert: ! IsReadableStreamLocked(source) is false."
    debug_assert!(!source.locked());

    // Step 7: "Assert: ! IsWritableStreamLocked(dest) is false."
    debug_assert!(!dest.locked());

    // Step 8: "If source.[[controller]] implements ReadableByteStreamController, let reader be either ! AcquireReadableStreamBYOBReader(source) or ! AcquireReadableStreamDefaultReader(source), at the user agent’s discretion."
    // Note: Readable byte streams are not implemented yet, so the current runtime always uses the default reader path.

    // Step 9: "Otherwise, let reader be ! AcquireReadableStreamDefaultReader(source)."
    let reader_object = acquire_readable_stream_default_reader(source.clone(), context)?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, |reader| reader.clone())?;

    // Step 10: "Let writer be ! AcquireWritableStreamDefaultWriter(dest)."
    let writer_object = super::acquire_writable_stream_default_writer(dest.clone(), context)?;
    let writer = super::with_writable_stream_default_writer_ref(&writer_object, |writer| writer.clone())?;

    // Step 11: "Set source.[[disturbed]] to true."
    source.set_disturbed(true);

    // Step 12: "Let shuttingDown be false."

    // Step 13: "Let promise be a new promise."
    let (pipe_promise, pipe_resolvers) = JsPromise::new_pending(context);
    let pipe_promise_obj: JsObject = pipe_promise.into();

    // Step 15: "In parallel but not really; see #905, using reader and writer, read all chunks from source and write them to dest."
    // Note: The helper below implements the current default-reader/default-writer pump directly on the carriers.
    let state = PipeToState::new(PipeToStateInner {
        reader,
        writer,
        source: source.clone(),
        dest: dest.clone(),
        prevent_close,
        prevent_abort,
        prevent_cancel,
        signal: signal.clone(),
        resolvers: Some(pipe_resolvers),
        shutting_down: false,
        pending_writes: 0,
        pending_shutdown: None,
    });

    // Step 14: "If signal is not undefined,"
    if let Some(signal) = signal {
        // Step 14.1: "Let abortAlgorithm be the following steps:"
        let abort_algorithm = SignalAbortAlgorithm::ReadableStreamPipeTo {
            state: state.clone(),
        };

        // Step 14.2: "If signal is aborted, perform abortAlgorithm and return promise."
        if signal.aborted_value() {
            state.run_abort_algorithm(context)?;
            return Ok(pipe_promise_obj);
        }

        // Step 14.3: "Add abortAlgorithm to signal."
        signal.add_abort_algorithm(abort_algorithm);
    }

    state.set_up_watchers(context)?;

    // Step 16: "Return promise."
    pipe_loop(state, context)?;

    Ok(pipe_promise_obj)
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct PipeToState(Gc<GcRefCell<PipeToStateInner>>);

#[derive(Trace, Finalize)]
pub(crate) struct PipeToStateInner {
    reader: ReadableStreamDefaultReader,
    writer: super::WritableStreamDefaultWriter,
    source: ReadableStream,
    dest: super::WritableStream,

    #[unsafe_ignore_trace]
    prevent_close: bool,

    #[unsafe_ignore_trace]
    prevent_abort: bool,

    #[unsafe_ignore_trace]
    prevent_cancel: bool,

    signal: Option<AbortSignal>,
    resolvers: Option<ResolvingFunctions>,

    #[unsafe_ignore_trace]
    shutting_down: bool,

    #[unsafe_ignore_trace]
    pending_writes: usize,

    pending_shutdown: Option<PipeShutdownRequest>,
}

impl PipeToState {
    fn new(state: PipeToStateInner) -> Self {
        Self(Gc::new(GcRefCell::new(state)))
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

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    pub(crate) fn run_abort_algorithm(&self, context: &mut Context) -> JsResult<()> {
        let error = self
            .borrow()
            .signal
            .as_ref()
            .map(AbortSignal::reason_value)
            .ok_or_else(|| {
                JsNativeError::typ().with_message(
                    "ReadableStreamPipeTo abort algorithm ran without an attached AbortSignal",
                )
            })?;

        pipe_shutdown_with_action(
            self.clone(),
            PipeShutdownAction::AbortSignal {
                error: error.clone(),
            },
            Some(error),
            context,
        )
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    pub(crate) fn set_up_watchers(&self, context: &mut Context) -> JsResult<()> {
        let reader_closed = self.borrow().reader.closed()?;
        let on_reader_closed = NativeFunction::from_copy_closure_with_captures(
            |_, _, state: &PipeToState, context| {
                pipe_source_closed(state.clone(), context)?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let on_reader_error = NativeFunction::from_copy_closure_with_captures(
            |_, args, state: &PipeToState, context| {
                let error = args.get_or_undefined(0).clone();
                pipe_source_errored(state.clone(), error, context)?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let _ = JsPromise::from_object(reader_closed)?
            .then(Some(on_reader_closed), Some(on_reader_error), context)?;

        let writer_closed = self.borrow().writer.closed()?;
        let on_writer_closed = NativeFunction::from_copy_closure_with_captures(
            |_, _, state: &PipeToState, context| {
                pipe_destination_closed(state.clone(), context)?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let on_writer_error = NativeFunction::from_copy_closure_with_captures(
            |_, args, state: &PipeToState, context| {
                let error = args.get_or_undefined(0).clone();
                pipe_destination_errored(state.clone(), error, context)?;
                Ok(JsValue::undefined())
            },
            self.clone(),
        )
        .to_js_function(context.realm());
        let _ = JsPromise::from_object(writer_closed)?
            .then(Some(on_writer_closed), Some(on_writer_error), context)?;

        Ok(())
    }
}

#[derive(Clone, Trace, Finalize)]
enum PipeShutdownAction {
    AbortDestination { error: JsValue },
    CancelSource { error: JsValue },
    CloseDestination,
    AbortSignal { error: JsValue },
}

#[derive(Clone, Trace, Finalize)]
enum PipeShutdownRequest {
    Finalize { error: Option<JsValue> },
    WithAction {
        action: PipeShutdownAction,
        original_error: Option<JsValue>,
    },
}

#[derive(Trace, Finalize)]
struct PipeFinalizeCapture {
    state: PipeToState,
    original_error: Option<JsValue>,
}

#[derive(Trace, Finalize)]
struct PipeAbortSignalCapture {
    source: ReadableStream,
    error: JsValue,
    resolvers: ResolvingFunctions,
}

#[derive(Trace, Finalize)]
struct PipeAbortSignalRejectedCapture {
    source: ReadableStream,
    error: JsValue,
    abort_error: JsValue,
    resolvers: ResolvingFunctions,
}

#[derive(Trace, Finalize)]
struct WaitForAllState {
    #[unsafe_ignore_trace]
    remaining: usize,

    #[unsafe_ignore_trace]
    settled: bool,

    #[unsafe_ignore_trace]
    first_rejection_index: Option<usize>,

    first_rejection_reason: Option<JsValue>,

    resolvers: ResolvingFunctions,
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_loop(state: PipeToState, context: &mut Context) -> JsResult<()> {
    if pipe_check_state(state.clone(), context)? {
        return Ok(());
    }

    // Wait for writer to be ready, then read.
    let ready_promise = state.borrow().writer.clone().ready()?;
    let state_for_ready = state.clone();
    let on_ready = NativeFunction::from_copy_closure_with_captures(
        |_, _, state: &PipeToState, context| {
            if pipe_check_state(state.clone(), context)? {
                return Ok(JsValue::undefined());
            }

            // Read one chunk.
            let reader = {
                let s = state.borrow();
                if s.shutting_down {
                    return Ok(JsValue::undefined());
                }

                s.reader.clone()
            };
            let read_promise = reader.read(context)?;

            let state_for_read = state.clone();
            let on_read = NativeFunction::from_copy_closure_with_captures(
                |_, args, state: &PipeToState, context| {
                    if pipe_check_state(state.clone(), context)? {
                        return Ok(JsValue::undefined());
                    }

                    let result = args.get_or_undefined(0);
                    let result_obj = match result.as_object() {
                        Some(obj) => obj,
                        None => {
                            pipe_shutdown(state.clone(), None, context)?;
                            return Ok(JsValue::undefined());
                        }
                    };

                    let done = result_obj.get(js_string!("done"), context)?.to_boolean();
                    if done {
                        pipe_source_closed(state.clone(), context)?;
                        return Ok(JsValue::undefined());
                    }

                    let value = result_obj.get(js_string!("value"), context)?;

                    // Write the chunk.
                    let writer = state.borrow().writer.clone();
                    pipe_note_write_started(state.clone());
                    let write_promise = match writer.write(value, context) {
                        Ok(write_promise) => write_promise,
                        Err(error) => {
                            pipe_note_write_finished(state.clone(), context)?;
                            return Err(error);
                        }
                    };

                    let state_for_write = state.clone();
                    let on_write = NativeFunction::from_copy_closure_with_captures(
                        |_, _, state: &PipeToState, context| {
                            pipe_note_write_finished(state.clone(), context)?;
                            Ok(JsValue::undefined())
                        },
                        state_for_write,
                    )
                    .to_js_function(context.realm());

                    let state_for_err = state.clone();
                    let on_write_error = NativeFunction::from_copy_closure_with_captures(
                        |_, args, state: &PipeToState, context| {
                            pipe_note_write_finished(state.clone(), context)?;

                            if pipe_check_state(state.clone(), context)? {
                                return Ok(JsValue::undefined());
                            }

                            let error = args.get_or_undefined(0).clone();
                            pipe_shutdown(state.clone(), Some(error), context)?;
                            Ok(JsValue::undefined())
                        },
                        state_for_err,
                    )
                    .to_js_function(context.realm());

                    let _ = JsPromise::from_object(write_promise)?
                        .then(Some(on_write), Some(on_write_error), context)?;

                    pipe_loop(state.clone(), context)?;

                    Ok(JsValue::undefined())
                },
                state_for_read,
            )
            .to_js_function(context.realm());

            let state_for_read_err = state.clone();
            let on_read_error = NativeFunction::from_copy_closure_with_captures(
                |_, args, state: &PipeToState, context| {
                    if pipe_check_state(state.clone(), context)? {
                        return Ok(JsValue::undefined());
                    }

                    let error = args.get_or_undefined(0).clone();
                    pipe_shutdown(state.clone(), Some(error), context)?;
                    Ok(JsValue::undefined())
                },
                state_for_read_err,
            )
            .to_js_function(context.realm());

            let _ = JsPromise::from_object(read_promise)?
                .then(Some(on_read), Some(on_read_error), context)?;

            Ok(JsValue::undefined())
        },
        state_for_ready,
    )
    .to_js_function(context.realm());

    let state_for_ready_err = state.clone();
    let on_ready_error = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &PipeToState, context| {
            if pipe_check_state(state.clone(), context)? {
                return Ok(JsValue::undefined());
            }

            let error = args.get_or_undefined(0).clone();
            pipe_shutdown(state.clone(), Some(error), context)?;
            Ok(JsValue::undefined())
        },
        state_for_ready_err,
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(ready_promise)?
        .then(Some(on_ready), Some(on_ready_error), context)?;

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_check_state(
    state: PipeToState,
    context: &mut Context,
) -> JsResult<bool> {
    let (source_state, source_error, dest_state, dest_error, dest_closing) = {
        let s = state.borrow();
        if s.shutting_down {
            return Ok(true);
        }

        let source_state = s.source.state();
        let source_error = if source_state == ReadableStreamState::Errored {
            Some(s.source.stored_error())
        } else {
            None
        };

        let dest_state = s.dest.state();
        let dest_error = if matches!(
            dest_state,
            super::WritableStreamState::Erroring | super::WritableStreamState::Errored
        ) {
            Some(s.dest.stored_error())
        } else {
            None
        };

        (
            source_state,
            source_error,
            dest_state,
            dest_error,
            s.dest.close_queued_or_in_flight(),
        )
    };

    if let Some(error) = source_error {
        pipe_source_errored(state, error, context)?;
        return Ok(true);
    }

    if let Some(error) = dest_error {
        pipe_destination_errored(state, error, context)?;
        return Ok(true);
    }

    if source_state == ReadableStreamState::Closed {
        pipe_source_closed(state, context)?;
        return Ok(true);
    }

    if dest_state == super::WritableStreamState::Closed || dest_closing {
        pipe_destination_closed(state, context)?;
        return Ok(true);
    }

    Ok(false)
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_source_errored(
    state: PipeToState,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    if state.borrow().prevent_abort {
        return pipe_shutdown(state, Some(error), context);
    }

    pipe_shutdown_with_action(
        state,
        PipeShutdownAction::AbortDestination {
            error: error.clone(),
        },
        Some(error),
        context,
    )
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_destination_errored(
    state: PipeToState,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    if state.borrow().prevent_cancel {
        return pipe_shutdown(state, Some(error), context);
    }

    pipe_shutdown_with_action(
        state,
        PipeShutdownAction::CancelSource {
            error: error.clone(),
        },
        Some(error),
        context,
    )
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_source_closed(
    state: PipeToState,
    context: &mut Context,
) -> JsResult<()> {
    if state.borrow().prevent_close {
        return pipe_shutdown(state, None, context);
    }

    pipe_shutdown_with_action(state, PipeShutdownAction::CloseDestination, None, context)
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_destination_closed(
    state: PipeToState,
    context: &mut Context,
) -> JsResult<()> {
    let error = type_error_value(
        "The destination WritableStream closed before the pipe operation completed",
        context,
    )?;

    if state.borrow().prevent_cancel {
        return pipe_shutdown(state, Some(error), context);
    }

    pipe_shutdown_with_action(
        state,
        PipeShutdownAction::CancelSource {
            error: error.clone(),
        },
        Some(error),
        context,
    )
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_note_write_started(state: PipeToState) {
    state.borrow_mut().pending_writes += 1;
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_note_write_finished(
    state: PipeToState,
    context: &mut Context,
) -> JsResult<()> {
    let pending_shutdown = {
        let mut s = state.borrow_mut();
        if s.pending_writes > 0 {
            s.pending_writes -= 1;
        }

        if s.pending_writes == 0 {
            s.pending_shutdown.take()
        } else {
            None
        }
    };

    if let Some(pending_shutdown) = pending_shutdown {
        execute_pipe_shutdown_request(state, pending_shutdown, context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
fn pipe_shutdown_with_action(
    state: PipeToState,
    action: PipeShutdownAction,
    original_error: Option<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    {
        let mut s = state.borrow_mut();
        if s.shutting_down {
            return Ok(());
        }

        s.shutting_down = true;

        if pipe_should_wait_for_pending_writes(&s) {
            s.pending_shutdown = Some(PipeShutdownRequest::WithAction {
                action,
                original_error,
            });
            return Ok(());
        }
    }

    execute_pipe_shutdown_request(
        state,
        PipeShutdownRequest::WithAction {
            action,
            original_error,
        },
        context,
    )
}

/// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown>
fn pipe_shutdown(
    state: PipeToState,
    error: Option<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    {
        let mut s = state.borrow_mut();
        if s.shutting_down {
            return Ok(());
        }

        s.shutting_down = true;

        if pipe_should_wait_for_pending_writes(&s) {
            s.pending_shutdown = Some(PipeShutdownRequest::Finalize { error });
            return Ok(());
        }
    }

    finalize_pipe(state, error, context)
}

fn pipe_should_wait_for_pending_writes(state: &PipeToStateInner) -> bool {
    state.pending_writes > 0
        && state.dest.state() == super::WritableStreamState::Writable
        && !state.dest.close_queued_or_in_flight()
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn execute_pipe_shutdown_request(
    state: PipeToState,
    request: PipeShutdownRequest,
    context: &mut Context,
) -> JsResult<()> {
    match &request {
        PipeShutdownRequest::Finalize { error } => finalize_pipe(state, error.clone(), context),
        PipeShutdownRequest::WithAction {
            action,
            original_error,
        } => {
            let action_promise =
                pipe_shutdown_action_promise(state.clone(), action.clone(), context)?;
            pipe_finalize_after_action(state, action_promise, original_error.clone(), context)
        }
    }
}

/// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
fn pipe_shutdown_action_promise(
    state: PipeToState,
    action: PipeShutdownAction,
    context: &mut Context,
) -> JsResult<JsObject> {
    match &action {
        PipeShutdownAction::AbortDestination { error } => {
            let dest = state.borrow().dest.clone();
            if dest.state() == super::WritableStreamState::Writable {
                return dest.abort_stream(error.clone(), context);
            }

            resolved_promise(JsValue::undefined(), context)
        }
        PipeShutdownAction::CancelSource { error } => {
            let source = state.borrow().source.clone();
            if source.state() == ReadableStreamState::Readable {
                return readable_stream_cancel(source, error.clone(), context);
            }

            resolved_promise(JsValue::undefined(), context)
        }
        PipeShutdownAction::CloseDestination => {
            let writer = state.borrow().writer.clone();
            writer.close(context)
        }
        PipeShutdownAction::AbortSignal { error } => {
            let (source, dest, prevent_abort, prevent_cancel) = {
                let s = state.borrow();
                (
                    s.source.clone(),
                    s.dest.clone(),
                    s.prevent_abort,
                    s.prevent_cancel,
                )
            };

            pipe_abort_signal_actions_promise(
                source,
                dest,
                prevent_abort,
                prevent_cancel,
                error.clone(),
                context,
            )
        }
    }
}

fn pipe_abort_signal_actions_promise(
    source: ReadableStream,
    dest: super::WritableStream,
    prevent_abort: bool,
    prevent_cancel: bool,
    error: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let abort_promise = if !prevent_abort && dest.state() == super::WritableStreamState::Writable {
        dest.abort_stream(error.clone(), context)?
    } else {
        resolved_promise(JsValue::undefined(), context)?
    };

    if prevent_cancel || source.state() != ReadableStreamState::Readable {
        return Ok(abort_promise);
    }

    let (promise, resolvers) = JsPromise::new_pending(context);
    let on_abort_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, capture: &PipeAbortSignalCapture, context| {
            let cancel_promise = readable_stream_cancel(
                capture.source.clone(),
                capture.error.clone(),
                context,
            )?;

            let on_cancel_fulfilled = NativeFunction::from_copy_closure_with_captures(
                |_, _, resolvers: &ResolvingFunctions, context| {
                    resolvers.resolve.call(
                        &JsValue::undefined(),
                        &[JsValue::undefined()],
                        context,
                    )?;
                    Ok(JsValue::undefined())
                },
                capture.resolvers.clone(),
            )
            .to_js_function(context.realm());
            let on_cancel_rejected = NativeFunction::from_copy_closure_with_captures(
                |_, args, resolvers: &ResolvingFunctions, context| {
                    resolvers.reject.call(
                        &JsValue::undefined(),
                        &[args.get_or_undefined(0).clone()],
                        context,
                    )?;
                    Ok(JsValue::undefined())
                },
                capture.resolvers.clone(),
            )
            .to_js_function(context.realm());

            let _ = JsPromise::from_object(cancel_promise)?
                .then(Some(on_cancel_fulfilled), Some(on_cancel_rejected), context)?;
            Ok(JsValue::undefined())
        },
        PipeAbortSignalCapture {
            source: source.clone(),
            error: error.clone(),
            resolvers: resolvers.clone(),
        },
    )
    .to_js_function(context.realm());
    let on_abort_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, capture: &PipeAbortSignalRejectedCapture, context| {
            let abort_error = args.get_or_undefined(0).clone();
            let cancel_promise = readable_stream_cancel(
                capture.source.clone(),
                capture.error.clone(),
                context,
            )?;

            let on_cancel_settled = NativeFunction::from_copy_closure_with_captures(
                |_, _, capture: &PipeAbortSignalRejectedCapture, context| {
                    capture.resolvers.reject.call(
                        &JsValue::undefined(),
                        &[capture.abort_error.clone()],
                        context,
                    )?;
                    Ok(JsValue::undefined())
                },
                PipeAbortSignalRejectedCapture {
                    source: capture.source.clone(),
                    error: capture.error.clone(),
                    abort_error,
                    resolvers: capture.resolvers.clone(),
                },
            )
            .to_js_function(context.realm());

            let _ = JsPromise::from_object(cancel_promise)?
                .then(Some(on_cancel_settled.clone()), Some(on_cancel_settled), context)?;
            Ok(JsValue::undefined())
        },
        PipeAbortSignalRejectedCapture {
            source,
            error,
            abort_error: JsValue::undefined(),
            resolvers,
        },
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(abort_promise)?
        .then(Some(on_abort_fulfilled), Some(on_abort_rejected), context)?;

    Ok(promise.into())
}

/// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
fn pipe_finalize_after_action(
    state: PipeToState,
    action_promise: JsObject,
    original_error: Option<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, capture: &PipeFinalizeCapture, context| {
            finalize_pipe(
                capture.state.clone(),
                capture.original_error.clone(),
                context,
            )?;
            Ok(JsValue::undefined())
        },
        PipeFinalizeCapture {
            state: state.clone(),
            original_error,
        },
    )
    .to_js_function(context.realm());

    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &PipeToState, context| {
            let new_error = args.get_or_undefined(0).clone();
            finalize_pipe(state.clone(), Some(new_error), context)?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm());

    let _ = JsPromise::from_object(action_promise)?
        .then(Some(on_fulfilled), Some(on_rejected), context)?;

    Ok(())
}

/// <https://streams.spec.whatwg.org/#rs-pipeTo-finalize>
fn finalize_pipe(
    state: PipeToState,
    error: Option<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    let (writer, reader, signal, resolvers) = {
        let mut s = state.borrow_mut();
        (
            s.writer.clone(),
            s.reader.clone(),
            s.signal.clone(),
            s.resolvers.take(),
        )
    };

    // Step 1: "Perform ! WritableStreamDefaultWriterRelease(writer)."
    super::writable_stream_default_writer_release(writer, context)?;

    // Step 3: "Otherwise, perform ! ReadableStreamDefaultReaderRelease(reader)."
    super::readable_stream_default_reader_release(reader, context)?;

    // Step 4: "If signal is not undefined, remove abortAlgorithm from signal."
    if let Some(signal) = signal {
        signal.remove_abort_algorithm(&SignalAbortAlgorithm::ReadableStreamPipeTo {
            state: state.clone(),
        });
    }

    match (error, resolvers) {
        // Step 5: "If error was given, reject promise with error."
        (Some(error), Some(resolvers)) => {
            resolvers
                .reject
                .call(&JsValue::undefined(), &[error], context)?;
        }
        // Step 6: "Otherwise, resolve promise with undefined."
        (None, Some(resolvers)) => {
            resolvers.resolve.call(
                &JsValue::undefined(),
                &[JsValue::undefined()],
                context,
            )?;
        }
        _ => {}
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn wait_for_all_promises(
    promises: Vec<JsObject>,
    context: &mut Context,
) -> JsResult<JsObject> {
    if promises.is_empty() {
        return resolved_promise(JsValue::undefined(), context);
    }

    if promises.len() == 1 {
        if let Some(promise) = promises.into_iter().next() {
            return Ok(promise);
        }

        return resolved_promise(JsValue::undefined(), context);
    }

    let (promise, resolvers) = JsPromise::new_pending(context);
    let aggregate = Gc::new(GcRefCell::new(WaitForAllState {
        remaining: promises.len(),
        settled: false,
        first_rejection_index: None,
        first_rejection_reason: None,
        resolvers,
    }));

    for (index, promise) in promises.into_iter().enumerate() {
        let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
            |_, _, aggregate: &Gc<GcRefCell<WaitForAllState>>, context| {
                let resolution = {
                    let mut aggregate = aggregate.borrow_mut();
                    if aggregate.settled {
                        return Ok(JsValue::undefined());
                    }

                    if aggregate.remaining > 0 {
                        aggregate.remaining -= 1;
                    }

                    if aggregate.remaining == 0 {
                        aggregate.settled = true;
                        Some((
                            aggregate.resolvers.clone(),
                            aggregate.first_rejection_reason.clone(),
                        ))
                    } else {
                        None
                    }
                };

                if let Some((resolvers, rejection_reason)) = resolution {
                    if let Some(rejection_reason) = rejection_reason {
                        resolvers.reject.call(
                            &JsValue::undefined(),
                            &[rejection_reason],
                            context,
                        )?;
                    } else {
                        resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::undefined()],
                            context,
                        )?;
                    }
                }

                Ok(JsValue::undefined())
            },
            aggregate.clone(),
        )
        .to_js_function(context.realm());

        let on_rejected = NativeFunction::from_copy_closure_with_captures(
            |_, args, capture: &(usize, Gc<GcRefCell<WaitForAllState>>), context| {
                let reason = args.get_or_undefined(0).clone();
                let resolution = {
                    let (index, aggregate) = capture;
                    let mut aggregate = aggregate.borrow_mut();
                    if aggregate.settled {
                        return Ok(JsValue::undefined());
                    }

                    if aggregate
                        .first_rejection_index
                        .is_none_or(|current_index| *index < current_index)
                    {
                        aggregate.first_rejection_index = Some(*index);
                        aggregate.first_rejection_reason = Some(reason);
                    }

                    if aggregate.remaining > 0 {
                        aggregate.remaining -= 1;
                    }

                    if aggregate.remaining == 0 {
                        aggregate.settled = true;
                        Some((
                            aggregate.resolvers.clone(),
                            aggregate.first_rejection_reason.clone(),
                        ))
                    } else {
                        None
                    }
                };

                if let Some((resolvers, rejection_reason)) = resolution {
                    if let Some(rejection_reason) = rejection_reason {
                        resolvers.reject.call(
                            &JsValue::undefined(),
                            &[rejection_reason],
                            context,
                        )?;
                    } else {
                        resolvers.resolve.call(
                            &JsValue::undefined(),
                            &[JsValue::undefined()],
                            context,
                        )?;
                    }
                }

                Ok(JsValue::undefined())
            },
            (index, aggregate.clone()),
        )
        .to_js_function(context.realm());

        let _ = JsPromise::from_object(promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)?;
    }

    Ok(promise.into())
}
