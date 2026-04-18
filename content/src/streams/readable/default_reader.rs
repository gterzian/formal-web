use std::mem;

use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    class::Class, object::{JsObject, builtins::JsPromise},
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use super::{
    ReadRequest, ReadableStream, ReadableStreamReader, ReadableStreamState,
    mark_promise_as_handled, rejected_promise, rejected_type_error_promise,
    type_error_value,
};
use super::stream::{readable_stream_cancel, with_readable_stream_ref};

/// Note: Models the reusable generic-reader algorithms that the Streams standard shares between
/// default readers and BYOB readers.
pub(crate) trait ReadableStreamGenericReader: Clone {
    /// Note: Reads `ReadableStreamGenericReader.[[stream]]`.
    fn stream_slot_value(&self) -> Option<ReadableStream>;

    /// Note: Writes `ReadableStreamGenericReader.[[stream]]`.
    fn set_stream_slot_value(&self, stream: Option<ReadableStream>);

    /// Note: Reads `ReadableStreamGenericReader.[[closedPromise]]`.
    fn closed_promise_slot_value(&self) -> Option<JsObject>;

    /// Note: Writes `ReadableStreamGenericReader.[[closedPromise]]`.
    fn set_closed_promise_slot_value(&self, promise: Option<JsObject>);

    /// Note: Reads the Rust-side promise capability paired with `[[closedPromise]]` when the
    /// promise is still pending.
    fn closed_resolvers_slot_value(&self) -> Option<ResolvingFunctions>;

    /// Note: Writes the Rust-side promise capability paired with `[[closedPromise]]`.
    fn set_closed_resolvers_slot_value(&self, resolvers: Option<ResolvingFunctions>);

    /// Note: Re-embeds the concrete reader carrier back into the shared reader-slot enum.
    fn as_reader_slot(&self) -> ReadableStreamReader;

    /// <https://streams.spec.whatwg.org/#generic-reader-closed>
    fn closed(&self) -> JsResult<JsObject> {
        // Step 1: "Return this.[[closedPromise]]."
        self.closed_promise_slot_value().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream reader is missing its closed promise")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#generic-reader-cancel>
    fn cancel(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        // Step 1: "If this.[[stream]] is undefined, return a promise rejected with a TypeError exception."
        if self.stream_slot_value().is_none() {
            return rejected_type_error_promise(
                "Cannot cancel a stream using a released reader",
                context,
            );
        }

        // Step 2: "Return ! ReadableStreamReaderGenericCancel(this, reason)."
        self.readable_stream_reader_generic_cancel(reason, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-reader-generic-cancel>
    fn readable_stream_reader_generic_cancel(
        &self,
        reason: JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        // Step 1: "Let stream be reader.[[stream]]."
        let stream = self.stream_slot_value().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream reader is not attached to a stream")
        })?;

        // Step 2: "Assert: stream is not undefined."
        debug_assert!(self.stream_slot_value().is_some());

        // Step 3: "Return ! ReadableStreamCancel(stream, reason)."
        readable_stream_cancel(stream, reason, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-reader-generic-initialize>
    fn readable_stream_reader_generic_initialize(
        &self,
        stream: ReadableStream,
        context: &mut Context,
    ) -> JsResult<()> {
        // Step 1: "Set reader.[[stream]] to stream."
        self.set_stream_slot_value(Some(stream.clone()));

        // Step 2: "Set stream.[[reader]] to reader."
        stream.set_reader_slot(Some(self.as_reader_slot()));

        // Step 3: "If stream.[[state]] is \"readable\","
        if stream.state() == ReadableStreamState::Readable {
            // Step 3.1: "Set reader.[[closedPromise]] to a new promise."
            let (promise, resolvers) = JsPromise::new_pending(context);
            self.set_closed_promise_slot_value(Some(promise.into()));
            self.set_closed_resolvers_slot_value(Some(resolvers));
            return Ok(());
        }

        // Step 4: "Otherwise, if stream.[[state]] is \"closed\","
        if stream.state() == ReadableStreamState::Closed {
            // Step 4.1: "Set reader.[[closedPromise]] to a promise resolved with undefined."
            let promise = JsPromise::resolve(JsValue::undefined(), context)?;
            self.set_closed_promise_slot_value(Some(promise.into()));
            self.set_closed_resolvers_slot_value(None);
            return Ok(());
        }

        // Step 5.1: "Assert: stream.[[state]] is \"errored\"."
        debug_assert_eq!(stream.state(), ReadableStreamState::Errored);

        // Step 5.2: "Set reader.[[closedPromise]] to a promise rejected with stream.[[storedError]]."
        let promise = rejected_promise(stream.stored_error(), context)?;
        self.set_closed_promise_slot_value(Some(promise.clone()));
        self.set_closed_resolvers_slot_value(None);

        // Step 5.3: "Set reader.[[closedPromise]].[[PromiseIsHandled]] to true."
        mark_promise_as_handled(&promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-reader-generic-release>
    fn readable_stream_reader_generic_release(&self, context: &mut Context) -> JsResult<()> {
        // Step 1: "Let stream be reader.[[stream]]."
        let stream = self.stream_slot_value().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream reader is not attached to a stream")
        })?;

        // Step 2: "Assert: stream is not undefined."
        debug_assert!(self.stream_slot_value().is_some());

        // Step 3: "Assert: stream.[[reader]] is reader."
        debug_assert!(stream.reader_slot().is_some());

        let release_error = type_error_value("Reader was released", context)?;

        // Step 4: "If stream.[[state]] is \"readable\", reject reader.[[closedPromise]] with a TypeError exception."
        if stream.state() == ReadableStreamState::Readable {
            if let Some(resolvers) = self.closed_resolvers_slot_value() {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[release_error.clone()], context)?;
            }
        } else {
            // Step 5: "Otherwise, set reader.[[closedPromise]] to a promise rejected with a TypeError exception."
            let closed_promise = rejected_promise(release_error.clone(), context)?;
            self.set_closed_promise_slot_value(Some(closed_promise.clone()));
            self.set_closed_resolvers_slot_value(None);
        }

        // Step 6: "Set reader.[[closedPromise]].[[PromiseIsHandled]] to true."
        if let Some(closed_promise) = self.closed_promise_slot_value() {
            mark_promise_as_handled(&closed_promise, context)?;
        }

        // Step 7: "Perform ! stream.[[controller]].[[ReleaseSteps]]()."
        let controller = stream.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream is missing its controller")
        })?;
        controller.release_steps()?;

        // Step 8: "Set stream.[[reader]] to undefined."
        stream.set_reader_slot(None);

        // Step 9: "Set reader.[[stream]] to undefined."
        self.set_stream_slot_value(None);
        Ok(())
    }
}

/// Note: Groups the spec-defined internal slots carried by `ReadableStreamDefaultReader`.
#[derive(Trace, Finalize)]
struct ReadableStreamDefaultReaderSlots {
    /// <https://streams.spec.whatwg.org/#readablestreamgenericreader-stream>
    stream: Option<ReadableStream>,

    /// <https://streams.spec.whatwg.org/#readablestreamgenericreader-closedpromise>
    closed_promise: Option<JsObject>,

    /// Note: Stores the Rust-side promise capability paired with `[[closedPromise]]` while the
    /// promise remains pending.
    closed_resolvers: Option<ResolvingFunctions>,

    /// <https://streams.spec.whatwg.org/#readablestreamdefaultreader-readrequests>
    read_requests: Vec<ReadRequest>,
}

/// <https://streams.spec.whatwg.org/#default-reader-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStreamDefaultReader {
    /// Note: Stores the JavaScript wrapper object that carries the reader's Web IDL brand.
    reflector: Gc<GcRefCell<Option<JsObject>>>,

    /// Note: Shares the reader's spec-defined internal slots across Rust clones.
    slots: Gc<GcRefCell<ReadableStreamDefaultReaderSlots>>,
}

impl ReadableStreamDefaultReader {
    /// Note: Allocates a default-reader carrier with empty spec-defined internal slots.
    pub(crate) fn new(reflector: Option<JsObject>) -> Self {
        Self {
            reflector: Gc::new(GcRefCell::new(reflector)),
            slots: Gc::new(GcRefCell::new(ReadableStreamDefaultReaderSlots {
                stream: None,
                closed_promise: None,
                closed_resolvers: None,
                read_requests: Vec::new(),
            })),
        }
    }

    /// Note: Records the reader's JavaScript wrapper once Boa allocates it.
    pub(crate) fn set_reflector(&self, reflector: JsObject) {
        *self.reflector.borrow_mut() = Some(reflector);
    }

    /// Note: Returns the JavaScript wrapper object for the reader carrier.
    pub(crate) fn object(&self) -> JsResult<JsObject> {
        self.reflector.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultReader is missing its JavaScript object")
                .into()
        })
    }

    /// <https://streams.spec.whatwg.org/#default-reader-constructor>
    pub(crate) fn set_up_readable_stream_default_reader(
        &self,
        stream: ReadableStream,
        context: &mut Context,
    ) -> JsResult<()> {
        // Step 1: "If ! IsReadableStreamLocked(stream) is true, throw a TypeError exception."
        if stream.is_readable_stream_locked() {
            return Err(JsNativeError::typ()
                .with_message("Cannot create a reader for a stream that already has a reader")
                .into());
        }

        // Step 2: "Perform ! ReadableStreamReaderGenericInitialize(reader, stream)."
        self.readable_stream_reader_generic_initialize(stream, context)?;

        // Step 3: "Set reader.[[readRequests]] to a new empty list."
        self.slots.borrow_mut().read_requests = Vec::new();
        Ok(())
    }

    /// Note: Takes ownership of the pending read-request queue and leaves the slot empty.
    pub(crate) fn take_read_requests(&self) -> Vec<ReadRequest> {
        mem::take(&mut self.slots.borrow_mut().read_requests)
    }

    /// Note: Returns the number of pending read requests.
    pub(crate) fn read_requests_len(&self) -> usize {
        self.slots.borrow().read_requests.len()
    }

    /// Note: Appends a new pending read request to `[[readRequests]]`.
    pub(crate) fn push_read_request(&self, read_request: ReadRequest) {
        self.slots.borrow_mut().read_requests.push(read_request);
    }

    /// Note: Removes and returns the oldest pending read request.
    pub(crate) fn shift_read_request(&self) -> Option<ReadRequest> {
        let mut slots = self.slots.borrow_mut();
        if slots.read_requests.is_empty() {
            None
        } else {
            Some(slots.read_requests.remove(0))
        }
    }

    /// <https://streams.spec.whatwg.org/#generic-reader-closed>
    pub(crate) fn closed(&self) -> JsResult<JsObject> {
        <Self as ReadableStreamGenericReader>::closed(self)
    }

    /// <https://streams.spec.whatwg.org/#generic-reader-cancel>
    pub(crate) fn cancel(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        <Self as ReadableStreamGenericReader>::cancel(self, reason, context)
    }

    /// <https://streams.spec.whatwg.org/#default-reader-read>
    pub(crate) fn read(&self, context: &mut Context) -> JsResult<JsObject> {
        // Step 1: "If this.[[stream]] is undefined, return a promise rejected with a TypeError exception."
        if self.stream_slot_value().is_none() {
            return rejected_type_error_promise("Cannot read from a released reader", context);
        }

        // Step 2: "Let promise be a new promise."
        // Step 3: "Let readRequest be a new read request with the following items:"
        let (read_request, promise) = ReadRequest::new(context);

        // Step 4: "Perform ! ReadableStreamDefaultReaderRead(this, readRequest)."
        readable_stream_default_reader_read(self.clone(), read_request, context)?;

        // Step 5: "Return promise."
        Ok(promise)
    }

    /// <https://streams.spec.whatwg.org/#default-reader-release-lock>
    pub(crate) fn release_lock(&self, context: &mut Context) -> JsResult<()> {
        // Step 1: "If this.[[stream]] is undefined, return."
        if self.stream_slot_value().is_none() {
            return Ok(());
        }

        // Step 2: "Perform ! ReadableStreamDefaultReaderRelease(this)."
        readable_stream_default_reader_release(self.clone(), context)
    }
}

impl ReadableStreamGenericReader for ReadableStreamDefaultReader {
    fn stream_slot_value(&self) -> Option<ReadableStream> {
        self.slots.borrow().stream.clone()
    }

    fn set_stream_slot_value(&self, stream: Option<ReadableStream>) {
        self.slots.borrow_mut().stream = stream;
    }

    fn closed_promise_slot_value(&self) -> Option<JsObject> {
        self.slots.borrow().closed_promise.clone()
    }

    fn set_closed_promise_slot_value(&self, promise: Option<JsObject>) {
        self.slots.borrow_mut().closed_promise = promise;
    }

    fn closed_resolvers_slot_value(&self) -> Option<ResolvingFunctions> {
        self.slots.borrow().closed_resolvers.clone()
    }

    fn set_closed_resolvers_slot_value(&self, resolvers: Option<ResolvingFunctions>) {
        self.slots.borrow_mut().closed_resolvers = resolvers;
    }

    fn as_reader_slot(&self) -> ReadableStreamReader {
        ReadableStreamReader::Default(self.clone())
    }
}

/// Note: Constructs a default reader from the JavaScript `new ReadableStreamDefaultReader(...)`
/// entry point.
pub(crate) fn construct_readable_stream_default_reader(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStreamDefaultReader> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let stream_object = args.get_or_undefined(0).as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader requires a ReadableStream")
    })?;
    let stream = with_readable_stream_ref(&stream_object, |stream| stream.clone())?;
    let reader = ReadableStreamDefaultReader::new(Some(reader_object.clone()));

    // Step 1: "Perform ? SetUpReadableStreamDefaultReader(this, stream)."
    reader.set_up_readable_stream_default_reader(stream, context)?;
    Ok(reader)
}

/// <https://streams.spec.whatwg.org/#acquire-readable-stream-reader>
pub(crate) fn acquire_readable_stream_default_reader(
    stream: ReadableStream,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let reader be a new ReadableStreamDefaultReader."
    let reader = create_readable_stream_default_reader(context)?;

    // Step 2: "Perform ? SetUpReadableStreamDefaultReader(reader, stream)."
    reader.set_up_readable_stream_default_reader(stream, context)?;

    // Step 3: "Return reader."
    reader.object()
}

/// Note: Allocates a default-reader carrier and its JavaScript wrapper together.
fn create_readable_stream_default_reader(
    context: &mut Context,
) -> JsResult<ReadableStreamDefaultReader> {
    let reader = ReadableStreamDefaultReader::new(None);
    let reader_object = ReadableStreamDefaultReader::from_data(reader.clone(), context)?;
    reader.set_reflector(reader_object);
    Ok(reader)
}

/// Note: Borrows a default reader carrier from a JavaScript object without mutating it.
pub(crate) fn with_readable_stream_default_reader_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStreamDefaultReader) -> R,
) -> JsResult<R> {
    let reader = object.downcast_ref::<ReadableStreamDefaultReader>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableStreamDefaultReader")
    })?;
    Ok(f(&reader))
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaultreadererrorreadrequests>
pub(crate) fn readable_stream_default_reader_error_read_requests(
    reader: ReadableStreamDefaultReader,
    error: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let readRequests be reader.[[readRequests]]."
    let read_requests = reader.take_read_requests();

    // Step 2: "Set reader.[[readRequests]] to a new empty list."
    // Note: `take_read_requests()` empties the list before iteration.

    // Step 3: "For each readRequest of readRequests,"
    for read_request in read_requests {
        // Step 3.1: "Perform readRequest's error steps, given e."
        read_request.error_steps(error.clone(), context)?;
    }

    Ok(())
}

/// <https://streams.spec.whatwg.org/#readable-stream-default-reader-read>
fn readable_stream_default_reader_read(
    reader: ReadableStreamDefaultReader,
    read_request: ReadRequest,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Let stream be reader.[[stream]]."
    let stream = reader.stream_slot_value().ok_or_else(|| {
        JsNativeError::typ().with_message("reader is not attached to a stream")
    })?;

    // Step 2: "Assert: stream is not undefined."
    debug_assert!(reader.stream_slot_value().is_some());

    // Step 3: "Set stream.[[disturbed]] to true."
    stream.set_disturbed(true);

    // Step 4: "If stream.[[state]] is \"closed\", perform readRequest's close steps."
    if stream.state() == ReadableStreamState::Closed {
        return read_request.close_steps(context);
    }

    // Step 5: "Otherwise, if stream.[[state]] is \"errored\", perform readRequest's error steps given stream.[[storedError]]."
    if stream.state() == ReadableStreamState::Errored {
        return read_request.error_steps(stream.stored_error(), context);
    }

    // Step 6.1: "Assert: stream.[[state]] is \"readable\"."
    debug_assert_eq!(stream.state(), ReadableStreamState::Readable);

    // Step 6.2: "Perform ! stream.[[controller]].[[PullSteps]](readRequest)."
    let controller = stream.controller_slot().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream is missing its controller")
    })?;
    controller.pull_steps(read_request, context)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreamdefaultreaderrelease>
fn readable_stream_default_reader_release(
    reader: ReadableStreamDefaultReader,
    context: &mut Context,
) -> JsResult<()> {
    // Step 1: "Perform ! ReadableStreamReaderGenericRelease(reader)."
    reader.readable_stream_reader_generic_release(context)?;

    // Step 2: "Let e be a new TypeError exception."
    let error = type_error_value("Reader was released", context)?;

    // Step 3: "Perform ! ReadableStreamDefaultReaderErrorReadRequests(reader, e)."
    readable_stream_default_reader_error_read_requests(reader, error, context)
}