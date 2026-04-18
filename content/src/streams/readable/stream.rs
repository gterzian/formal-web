use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    class::Class,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::{JsArray, JsFunction, JsPromise}},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};

use super::{
    CancelAlgorithm, PullAlgorithm, ReadableStreamController, ReadableStreamReader,
    ReadableStreamState, SourceMethod, StartAlgorithm,
    ReadableStreamDefaultReader, ReadableStreamGenericReader, ReadRequest,
    acquire_readable_stream_default_reader, create_readable_stream_default_controller,
    readable_stream_default_reader_error_read_requests, rejected_promise,
    rejected_type_error_promise, resolved_promise,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source,
    transform_promise_to_undefined,
    with_readable_stream_default_reader_ref,
};

/// Note: Groups the spec-defined internal slots carried by `ReadableStream`.
#[derive(Trace, Finalize)]
struct ReadableStreamSlots {
    /// <https://streams.spec.whatwg.org/#readablestream-controller>
    controller: Option<ReadableStreamController>,

    /// <https://streams.spec.whatwg.org/#readablestream-reader>
    reader: Option<ReadableStreamReader>,

    /// <https://streams.spec.whatwg.org/#readablestream-disturbed>
    #[unsafe_ignore_trace]
    disturbed: bool,

    /// <https://streams.spec.whatwg.org/#readablestream-state>
    #[unsafe_ignore_trace]
    state: ReadableStreamState,

    /// <https://streams.spec.whatwg.org/#readablestream-storederror>
    stored_error: JsValue,
}

/// <https://streams.spec.whatwg.org/#rs-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStream {
    /// Note: Stores the JavaScript wrapper object that carries the stream's Web IDL brand.
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// Note: Shares the stream's spec-defined internal slots across Rust clones.
    slots: Gc<GcRefCell<ReadableStreamSlots>>,
}

impl ReadableStream {
    /// Note: Allocates a stream carrier with empty spec-defined internal slots.
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            slots: Gc::new(GcRefCell::new(ReadableStreamSlots {
                controller: None,
                reader: None,
                disturbed: false,
                state: ReadableStreamState::Readable,
                stored_error: JsValue::undefined(),
            })),
        }
    }

    /// Note: Records the stream's JavaScript wrapper once Boa allocates it.
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    /// Note: Returns the JavaScript wrapper object for the stream carrier.
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream is missing its JavaScript object")
                .into()
        })
    }

    /// Note: Reads `ReadableStream.[[controller]]`.
    pub(crate) fn controller_slot(&self) -> Option<ReadableStreamController> {
        self.slots.borrow().controller.clone()
    }

    /// Note: Writes `ReadableStream.[[controller]]`.
    pub(crate) fn set_controller_slot(&self, controller: Option<ReadableStreamController>) {
        self.slots.borrow_mut().controller = controller;
    }

    /// Note: Reads `ReadableStream.[[reader]]`.
    pub(crate) fn reader_slot(&self) -> Option<ReadableStreamReader> {
        self.slots.borrow().reader.clone()
    }

    /// Note: Writes `ReadableStream.[[reader]]`.
    pub(crate) fn set_reader_slot(&self, reader: Option<ReadableStreamReader>) {
        self.slots.borrow_mut().reader = reader;
    }

    /// Note: Reads `ReadableStream.[[state]]`.
    pub(crate) fn state(&self) -> ReadableStreamState {
        self.slots.borrow().state
    }

    /// Note: Writes `ReadableStream.[[state]]`.
    pub(crate) fn set_state(&self, state: ReadableStreamState) {
        self.slots.borrow_mut().state = state;
    }

    /// Note: Reads `ReadableStream.[[storedError]]`.
    pub(crate) fn stored_error(&self) -> JsValue {
        self.slots.borrow().stored_error.clone()
    }

    /// Note: Writes `ReadableStream.[[storedError]]`.
    pub(crate) fn set_stored_error(&self, error: JsValue) {
        self.slots.borrow_mut().stored_error = error;
    }

    /// Note: Writes `ReadableStream.[[disturbed]]`.
    pub(crate) fn set_disturbed(&self, disturbed: bool) {
        self.slots.borrow_mut().disturbed = disturbed;
    }

    /// <https://streams.spec.whatwg.org/#initialize-readable-stream>
    fn initialize_readable_stream(&mut self) {
        let mut slots = self.slots.borrow_mut();

        // Step 1: "Set stream.[[state]] to \"readable\"."
        slots.state = ReadableStreamState::Readable;

        // Step 2: "Set stream.[[reader]] and stream.[[storedError]] to undefined."
        slots.reader = None;
        slots.stored_error = JsValue::undefined();

        // Step 3: "Set stream.[[disturbed]] to false."
        slots.disturbed = false;
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
        _transform: &JsValue,
        _options: &JsValue,
        _context: &mut Context,
    ) -> JsResult<JsValue> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, throw a TypeError exception."
        // TODO: Implement `WritableStream`, `ReadableWritablePair`, and `ReadableStreamPipeTo`.

        // Step 2: "If ! IsWritableStreamLocked(transform[\"writable\"]) is true, throw a TypeError exception."
        // TODO: Implement `WritableStream` locking checks.

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        // TODO: Implement `StreamPipeOptions` dictionary conversion.

        // Step 4: "Let promise be ! ReadableStreamPipeTo(this, transform[\"writable\"], options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // TODO: Implement `ReadableStreamPipeTo`.

        // Step 5: "Set promise.[[PromiseIsHandled]] to true."
        // TODO: Mark the pipe promise handled once `ReadableStreamPipeTo` exists.

        // Step 6: "Return transform[\"readable\"]."
        // TODO: Return the readable side of the transform pair.
        Err(JsNativeError::typ()
            .with_message("ReadableStream.pipeThrough() is not implemented yet")
            .into())
    }

    /// <https://streams.spec.whatwg.org/#rs-pipe-to>
    pub(crate) fn pipe_to(
        &mut self,
        _destination: &JsValue,
        _options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        // Step 1: "If ! IsReadableStreamLocked(this) is true, return a promise rejected with a TypeError exception."
        // TODO: Implement the `ReadableStreamPipeTo` algorithm and destination locking checks.

        // Step 2: "If ! IsWritableStreamLocked(destination) is true, return a promise rejected with a TypeError exception."
        // TODO: Implement `WritableStream`.

        // Step 3: "Let signal be options[\"signal\"] if it exists, or undefined otherwise."
        // TODO: Implement `StreamPipeOptions` dictionary conversion.

        // Step 4: "Return ! ReadableStreamPipeTo(this, destination, options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // TODO: Implement `ReadableStreamPipeTo`.
        rejected_type_error_promise("ReadableStream.pipeTo() is not implemented yet", context)
    }

    /// <https://streams.spec.whatwg.org/#rs-tee>
    pub(crate) fn tee(&mut self, context: &mut Context) -> JsResult<JsValue> {
        // Step 1: "Return ? ReadableStreamTee(this, false)."
        readable_stream_tee(self.clone(), false, context)
    }
}

/// Note: Tracks the shared mutable state that `ReadableStreamTee` carries across both branch
/// pull and cancel algorithms.
#[derive(Trace, Finalize)]
struct TeeState {
    /// Note: Stores the original stream being tee'd.
    source_stream: ReadableStream,

    /// Note: Stores the default reader that consumes the source stream.
    reader: ReadableStreamDefaultReader,

    /// Note: Stores the first tee branch once created.
    branch1: Option<ReadableStream>,

    /// Note: Stores the second tee branch once created.
    branch2: Option<ReadableStream>,

    /// Note: Stores the pull algorithm closure shared by both tee branches.
    pull_function: Option<JsObject>,

    /// Note: Stores the promise returned once both branches cancel.
    cancel_promise: JsObject,

    /// Note: Stores the resolving functions paired with `cancel_promise`.
    cancel_resolvers: boa_engine::builtins::promise::ResolvingFunctions,

    /// Note: Tracks whether the source reader currently has an in-flight read.
    #[unsafe_ignore_trace]
    reading: bool,

    /// Note: Tracks whether another branch requested a pull while a read was already in flight.
    #[unsafe_ignore_trace]
    read_again: bool,

    /// Note: Tracks whether branch 1 already canceled.
    #[unsafe_ignore_trace]
    canceled1: bool,

    /// Note: Tracks whether branch 2 already canceled.
    #[unsafe_ignore_trace]
    canceled2: bool,

    /// Note: Stores branch 1's cancel reason until both branches cancel.
    reason1: JsValue,

    /// Note: Stores branch 2's cancel reason until both branches cancel.
    reason2: JsValue,
}

/// Note: Constructs a `ReadableStream` carrier from the JavaScript `new ReadableStream(...)`
/// entry point.
pub(crate) fn construct_readable_stream(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStream> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;
    let mut stream = ReadableStream::new(Some(stream_object.clone()));

    // Step 1: "If underlyingSource is missing, set it to null."
    let underlying_source = if args.is_empty() {
        JsValue::null()
    } else {
        args[0].clone()
    };

    // Step 2: "Let underlyingSourceDict be underlyingSource, converted to an IDL value of type UnderlyingSource."
    // Note: The current runtime keeps the original JavaScript object so it can invoke the underlying source callbacks directly.
    let underlying_source_object = if underlying_source.is_null() || underlying_source.is_undefined()
    {
        None
    } else {
        Some(underlying_source.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream underlyingSource must be an object")
        })?)
    };

    // Step 3: "Perform ! InitializeReadableStream(this)."
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
            return Err(JsNativeError::range()
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
fn create_readable_stream(
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
    debug_assert!(high_water_mark.is_finite() && high_water_mark >= 0.0);

    // Step 4: "Let stream be a new ReadableStream."
    let mut stream = create_readable_stream_object(context)?;

    // Step 5: "Perform ! InitializeReadableStream(stream)."
    stream.initialize_readable_stream();

    // Step 6: "Let controller be a new ReadableStreamDefaultController."
    let controller = create_readable_stream_default_controller(context)?;

    // Step 7: "Perform ? SetUpReadableStreamDefaultController(stream, controller, startAlgorithm, pullAlgorithm, cancelAlgorithm, highWaterMark, sizeAlgorithm)."
    set_up_readable_stream_default_controller(
        stream.clone(),
        controller,
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

/// Note: Allocates a stream carrier and its JavaScript wrapper together.
fn create_readable_stream_object(context: &mut Context) -> JsResult<ReadableStream> {
    let stream = ReadableStream::new(None);
    let stream_object = ReadableStream::from_data(stream.clone(), context)?;
    stream.set_reflector(stream_object);
    Ok(stream)
}

/// Note: Borrows a stream carrier from a JavaScript object without mutating it.
pub(crate) fn with_readable_stream_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStream) -> R,
) -> JsResult<R> {
    let stream = object
        .downcast_ref::<ReadableStream>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not a ReadableStream"))?;
    Ok(f(&stream))
}

/// Note: Borrows a stream carrier mutably from a JavaScript object.
pub(crate) fn with_readable_stream_mut<R>(
    object: &JsObject,
    f: impl FnOnce(&mut ReadableStream) -> R,
) -> JsResult<R> {
    let Some(mut stream) = object.downcast_mut::<ReadableStream>() else {
        return Err(JsNativeError::typ()
            .with_message("object is not a ReadableStream")
            .into());
    };
    Ok(f(&mut stream))
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
        super::mark_promise_as_handled(&closed_promise, context)?;
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
                    let (branch1, branch2, canceled1, canceled2, should_read_again, pull_function) = {
                        let mut tee_state = tee_state.borrow_mut();
                        tee_state.reading = false;
                        let should_read_again = tee_state.read_again;
                        if should_read_again {
                            tee_state.read_again = false;
                        }
                        (
                            tee_state.branch1.clone(),
                            tee_state.branch2.clone(),
                            tee_state.canceled1,
                            tee_state.canceled2,
                            should_read_again,
                            tee_state.pull_function.clone(),
                        )
                    };

                    if !canceled1 {
                        if let Some(branch1) = branch1 {
                            if let Some(mut controller) = branch1
                                .controller_slot()
                                .map(|controller| controller.as_default_controller())
                            {
                                controller.enqueue(value.clone(), context)?;
                            }
                        }
                    }
                    if !canceled2 {
                        if let Some(branch2) = branch2 {
                            if let Some(mut controller) = branch2
                                .controller_slot()
                                .map(|controller| controller.as_default_controller())
                            {
                                controller.enqueue(value, context)?;
                            }
                        }
                    }

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
                tee_state.clone(),
            )
            .to_js_function(context.realm());
            let on_rejected = NativeFunction::from_copy_closure_with_captures(
                |_, _: &[JsValue], tee_state: &Gc<GcRefCell<TeeState>>, _: &mut Context| {
                    tee_state.borrow_mut().reading = false;
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
        PullAlgorithm::JavaScript(SourceMethod {
            this_value: context.global_object(),
            callback: pull_function.clone().into(),
        }),
        CancelAlgorithm::JavaScript(SourceMethod {
            this_value: context.global_object(),
            callback: cancel1_function.into(),
        }),
        Some(1.0),
        Some(SizeAlgorithm::ReturnOne),
        context,
    )?;
    let branch2 = create_readable_stream(
        StartAlgorithm::ReturnUndefined,
        PullAlgorithm::JavaScript(SourceMethod {
            this_value: context.global_object(),
            callback: pull_function.clone().into(),
        }),
        CancelAlgorithm::JavaScript(SourceMethod {
            this_value: context.global_object(),
            callback: cancel2_function.into(),
        }),
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

/// Note: Reads `underlyingSource.type` from the original JavaScript object kept for callback
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

/// Note: Reports whether a queuing strategy provides a custom `size` member.
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