use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::{
        iterable::create_iter_result_object,
        promise::{PromiseState, ResolvingFunctions},
    },
    class::Class,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::{JsArray, JsFunction, JsPromise}},
    symbol::JsSymbol,
};
use boa_gc::{Finalize, Gc, GcRef, GcRefCell, GcRefMut, Trace};

use crate::boa::with_abort_signal_ref;
use crate::dom::{AbortAlgorithm as SignalAbortAlgorithm, AbortSignal};
use crate::streams::{SizeAlgorithm, extract_high_water_mark, extract_size_algorithm};
use crate::webidl::{
    mark_promise_as_handled, promise_from_value, rejected_promise, resolved_promise,
    transform_promise_to_undefined,
};

use super::{
    CancelAlgorithm, PullAlgorithm, ReadableStreamController, ReadableStreamReader,
    ReadableStreamState, SourceMethod, StartAlgorithm,
    ReadableStreamDefaultReader, ReadableStreamGenericReader, ReadRequest,
    acquire_readable_stream_byob_reader,
    acquire_readable_stream_default_reader,
    readable_stream_default_reader_error_read_requests, rejected_type_error_promise,
    set_up_readable_byte_stream_controller_from_underlying_source,
    set_up_readable_stream_default_controller,
    set_up_readable_stream_default_controller_from_underlying_source,
    type_error_value, with_readable_stream_default_reader_ref,
};

/// <https://streams.spec.whatwg.org/#rs-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStream {
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
    pub(crate) fn new() -> Self {
        Self {
            controller: Gc::new(GcRefCell::new(None)),
            controller_object: Gc::new(GcRefCell::new(None)),
            reader: Gc::new(GcRefCell::new(None)),
            disturbed: Rc::new(Cell::new(false)),
            state: Rc::new(RefCell::new(ReadableStreamState::Readable)),
            stored_error: Gc::new(GcRefCell::new(JsValue::undefined())),
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
        let Some(controller) = self.controller_slot() else {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream is missing its controller")
                .into());
        };
        if controller.as_byte_controller().is_none() {
            return Err(JsNativeError::typ()
                .with_message("Cannot acquire a BYOB reader for a non-byte stream")
                .into());
        }

        acquire_readable_stream_byob_reader(self.clone(), context)
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
        let readable_value = transform_obj.get(js_string!("readable"), context)?;
        let readable_obj = readable_value.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableWritablePair is missing its readable property")
        })?;
        let _ = with_readable_stream_ref(&readable_obj, |stream| stream.clone())?;

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

        let options = normalize_pipe_options(options, context)?;

        // Step 4: "Let promise be ! ReadableStreamPipeTo(this, transform[\"writable\"], options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        let destination = super::with_writable_stream_ref(&writable_obj, |ws| ws.clone())?;
        let promise = readable_stream_pipe_to(
            self.clone(),
            destination,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
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
        let dest_locked = match super::with_writable_stream_ref(&dest_obj, |ws| ws.locked()) {
            Ok(locked) => locked,
            Err(error) => return rejected_promise(error.into_opaque(context)?, context),
        };
        if dest_locked {
            return rejected_type_error_promise(
                "ReadableStream.pipeTo(): destination is locked",
                context,
            );
        }

        let options = match normalize_pipe_options(options, context) {
            Ok(options) => options,
            Err(error) => return rejected_promise(error.into_opaque(context)?, context),
        };

        let dest = match super::with_writable_stream_ref(&dest_obj, |ws| ws.clone()) {
            Ok(dest) => dest,
            Err(error) => return rejected_promise(error.into_opaque(context)?, context),
        };

        // Step 4: "Return ! ReadableStreamPipeTo(this, destination, options[\"preventClose\"], options[\"preventAbort\"], options[\"preventCancel\"], signal)."
        // Note: The Rust helper takes the normalized option members as separate arguments.
        match readable_stream_pipe_to(
            self.clone(),
            dest,
            options.prevent_close,
            options.prevent_abort,
            options.prevent_cancel,
            options.signal,
            context,
        ) {
            Ok(promise) => Ok(promise),
            Err(error) => rejected_promise(error.into_opaque(context)?, context),
        }
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
    fn next_result_promise(&self, context: &mut Context) -> JsResult<JsObject> {
        let next_result = self
            .next_method
            .call(&JsValue::from(self.iterator.clone()), &[], context)?;

        match self.kind {
            ReadableStreamFromIteratorKind::Async => promise_from_value(next_result, context),
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(next_result, context)
            }
        }
    }

    fn return_result_promise(&self, reason: JsValue, context: &mut Context) -> JsResult<Option<JsObject>> {
        let return_method = get_optional_callable_method_value(
            self.iterator.get(js_string!("return"), context)?,
            "ReadableStream.from() iterator.return",
        )?;
        let Some(return_method) = return_method else {
            return Ok(None);
        };

        let return_result = return_method.call(&JsValue::from(self.iterator.clone()), &[reason], context)?;
        let return_promise = match self.kind {
            ReadableStreamFromIteratorKind::Async => promise_from_value(return_result, context)?,
            ReadableStreamFromIteratorKind::Sync => {
                promise_from_sync_iterator_result(return_result, context)?
            }
        };
        Ok(Some(return_promise))
    }
}

#[derive(Clone, Trace, Finalize)]
struct ReadableStreamFromIterableState {
    iterator_record: ReadableStreamFromIteratorRecord,
    stream: Gc<GcRefCell<Option<ReadableStream>>>,
}

impl ReadableStreamFromIterableState {
    fn new(iterator_record: ReadableStreamFromIteratorRecord) -> Self {
        Self {
            iterator_record,
            stream: Gc::new(GcRefCell::new(None)),
        }
    }

    fn set_stream(&self, stream: ReadableStream) {
        *self.stream.borrow_mut() = Some(stream);
    }

    fn stream(&self) -> JsResult<ReadableStream> {
        self.stream.borrow().clone().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStream.from() is missing its stream")
                .into()
        })
    }
}
/// <https://streams.spec.whatwg.org/#rs-constructor>
pub(crate) fn construct_readable_stream(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStream> {
    let mut stream = ReadableStream::new();

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
    // Note: Boa attaches the returned native carrier to the newly created wrapper after
    // `data_constructor` returns.
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
            let high_water_mark = extract_high_water_mark(&strategy, 0.0, context)?;

            // Step 4.3: "Perform ? SetUpReadableByteStreamControllerFromUnderlyingSource(this, underlyingSource, underlyingSourceDict, highWaterMark)."
            set_up_readable_byte_stream_controller_from_underlying_source(
                stream.clone(),
                underlying_source_object,
                high_water_mark,
                context,
            )?;
            return Ok(stream);
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
) -> JsResult<(ReadableStream, JsObject)> {
    // Step 1: "If highWaterMark was not passed, set it to 1."
    let high_water_mark = high_water_mark.unwrap_or(1.0);

    // Step 2: "If sizeAlgorithm was not passed, set it to an algorithm that returns 1."
    let size_algorithm = size_algorithm.unwrap_or(SizeAlgorithm::ReturnOne);

    // Step 3: "Assert: ! IsNonNegativeNumber(highWaterMark) is true."
    debug_assert!(high_water_mark >= 0.0 && !high_water_mark.is_nan());

    // Step 4: "Let stream be a new ReadableStream."
    let (mut stream, stream_object) = create_readable_stream_object(context)?;

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
    Ok((stream, stream_object))
}
fn create_readable_stream_object(context: &mut Context) -> JsResult<(ReadableStream, JsObject)> {
    let stream = ReadableStream::new();
    let stream_object: JsObject = ReadableStream::from_data(stream.clone(), context)?.into();
    Ok((stream, stream_object))
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
pub(crate) fn readable_stream_from_iterable(
    async_iterable: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let stream be undefined."
    let state = ReadableStreamFromIterableState::new(get_readable_stream_from_iterator_record(
        async_iterable,
        context,
    )?);

    // Step 2: "Let iteratorRecord be ? GetIterator(asyncIterable, async)."
    // Note: `get_readable_stream_from_iterator_record()` normalizes async iterators and the
    // async-from-sync fallback into a record whose `next_result_promise()` matches the spec.

    // Step 3: "Let startAlgorithm be an algorithm that returns undefined."
    let start_algorithm = StartAlgorithm::ReturnUndefined;

    // Step 4: "Let pullAlgorithm be the following steps:"
    let pull_algorithm = PullAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, _, state: &ReadableStreamFromIterableState, context| {
                Ok(JsValue::from(readable_stream_from_iterable_pull_algorithm(
                    state.clone(),
                    context,
                )?))
            },
            state.clone(),
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 5: "Let cancelAlgorithm be the following steps, given reason:"
    let cancel_algorithm = CancelAlgorithm::JavaScript(SourceMethod::new(
        context.global_object(),
        NativeFunction::from_copy_closure_with_captures(
            |_, args, state: &ReadableStreamFromIterableState, context| {
                let reason = args.get_or_undefined(0).clone();
                Ok(JsValue::from(readable_stream_from_iterable_cancel_algorithm(
                    state.clone(),
                    reason,
                    context,
                )?))
            },
            state.clone(),
        )
        .to_js_function(context.realm())
        .into(),
    ));

    // Step 6: "Set stream to ! CreateReadableStream(startAlgorithm, pullAlgorithm, cancelAlgorithm, 0)."
    let (stream, stream_object) = create_readable_stream(
        start_algorithm,
        pull_algorithm,
        cancel_algorithm,
        Some(0.0),
        None,
        context,
    )?;
    state.set_stream(stream);

    // Step 7: "Return stream."
    Ok(stream_object)
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
fn readable_stream_from_iterable_pull_algorithm(
    state: ReadableStreamFromIterableState,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 4.1: "Let nextResult be IteratorNext(iteratorRecord)."
    let next_result = state.iterator_record.next_result_promise(context);

    // Step 4.2: "If nextResult is an abrupt completion, return a promise rejected with nextResult.[[Value]]."
    let next_promise = match next_result {
        Ok(next_promise) => next_promise,
        Err(error) => return rejected_promise(error.into_opaque(context)?, context),
    };

    // Step 4.3: "Let nextPromise be a promise resolved with nextResult.[[Value]]."
    // Note: `next_result_promise()` already returns the promise produced by `PromiseResolve`
    // and applies async-from-sync iterator adaptation for sync iterables.

    // Step 4.4: "Return the result of reacting to nextPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &ReadableStreamFromIterableState, context| {
            let iter_result = args.get_or_undefined(0).clone();

            // Step 4.4.1: "If iterResult is not an Object, throw a TypeError."
            let iter_result_object = iter_result.as_object().ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ReadableStream.from() iterator next() must fulfill with an object")
            })?;

            // Step 4.4.2: "Let done be ? IteratorComplete(iterResult)."
            let done = iter_result_object.get(js_string!("done"), context)?.to_boolean();

            let stream = state.stream()?;
            let controller = stream.controller_slot().ok_or_else(|| {
                JsNativeError::typ().with_message("ReadableStream.from() is missing its controller")
            })?;
            let controller = controller.as_default_controller();

            // Step 4.4.3: "If done is true:"
            if done {
                // Step 4.4.3.1: "Perform ! ReadableStreamDefaultControllerClose(stream.[[controller]])."
                controller.close_steps(context)?;
                return Ok(JsValue::undefined());
            }

            // Step 4.4.4.1: "Let value be ? IteratorValue(iterResult)."
            let value = iter_result_object.get(js_string!("value"), context)?;

            // Step 4.4.4.2: "Perform ! ReadableStreamDefaultControllerEnqueue(stream.[[controller]], value)."
            controller.enqueue_steps(value, context)?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(next_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

/// <https://streams.spec.whatwg.org/#readable-stream-from-iterable>
fn readable_stream_from_iterable_cancel_algorithm(
    state: ReadableStreamFromIterableState,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 5.1: "Let iterator be iteratorRecord.[[Iterator]]."
    // Step 5.2: "Let returnMethod be GetMethod(iterator, \"return\")."
    // Step 5.3: "If returnMethod is an abrupt completion, return a promise rejected with returnMethod.[[Value]]."
    // Step 5.4: "If returnMethod.[[Value]] is undefined, return a promise resolved with undefined."
    // Step 5.5: "Let returnResult be Call(returnMethod.[[Value]], iterator, « reason »)."
    // Step 5.6: "If returnResult is an abrupt completion, return a promise rejected with returnResult.[[Value]]."
    // Step 5.7: "Let returnPromise be a promise resolved with returnResult.[[Value]]."
    // Note: `return_result_promise()` folds those steps together and applies the async-from-sync
    // iterator adaptation required for sync iterables.
    let return_result = state.iterator_record.return_result_promise(reason, context);
    let return_promise = match return_result {
        Ok(Some(return_promise)) => return_promise,
        Ok(None) => return resolved_promise(JsValue::undefined(), context),
        Err(error) => return rejected_promise(error.into_opaque(context)?, context),
    };

    // Step 5.8: "Return the result of reacting to returnPromise with the following fulfillment steps, given iterResult:"
    let on_fulfilled = NativeFunction::from_fn_ptr(|_, args, _| {
        // Step 5.8.1: "If iterResult is not an Object, throw a TypeError."
        if args.get_or_undefined(0).as_object().is_none() {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream.from() iterator return() must fulfill with an object")
                .into());
        }

        // Step 5.8.2: "Return undefined."
        Ok(JsValue::undefined())
    })
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(return_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

fn get_readable_stream_from_iterator_record(
    async_iterable: JsValue,
    context: &mut Context,
) -> JsResult<ReadableStreamFromIteratorRecord> {
    let iterable_object = async_iterable.to_object(context)?;

    if let Some(async_iterator_method) = get_optional_callable_method_value(
        iterable_object.get(JsSymbol::async_iterator(), context)?,
        "ReadableStream.from() iterable[@@asyncIterator]",
    )? {
        let iterator = async_iterator_method
            .call(&async_iterable, &[], context)?
            .as_object()
            .ok_or_else(|| {
                JsNativeError::typ()
                    .with_message("ReadableStream.from() @@asyncIterator must return an object")
            })?
            .clone();
        let next_method = get_required_callable_method(
            &iterator,
            "next",
            "ReadableStream.from() iterator.next must be callable",
            context,
        )?;
        return Ok(ReadableStreamFromIteratorRecord {
            iterator,
            next_method,
            kind: ReadableStreamFromIteratorKind::Async,
        });
    }

    let iterator_method = get_optional_callable_method_value(
        iterable_object.get(JsSymbol::iterator(), context)?,
        "ReadableStream.from() iterable[@@iterator]",
    )?
    .ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream.from() requires an async iterable or iterable")
    })?;
    let iterator = iterator_method
        .call(&async_iterable, &[], context)?
        .as_object()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream.from() @@iterator must return an object")
        })?
        .clone();
    let next_method = get_required_callable_method(
        &iterator,
        "next",
        "ReadableStream.from() iterator.next must be callable",
        context,
    )?;
    Ok(ReadableStreamFromIteratorRecord {
        iterator,
        next_method,
        kind: ReadableStreamFromIteratorKind::Sync,
    })
}

fn promise_from_sync_iterator_result(iter_result: JsValue, context: &mut Context) -> JsResult<JsObject> {
    let iter_result_object = match iter_result.as_object() {
        Some(iter_result_object) => iter_result_object.clone(),
        None => {
            return rejected_promise(
                JsNativeError::typ()
                    .with_message("ReadableStream.from() iterator result must be an object")
                    .into_opaque(context)
                    .into(),
                context,
            );
        }
    };

    let done = match iter_result_object.get(js_string!("done"), context) {
        Ok(done) => done.to_boolean(),
        Err(error) => return rejected_promise(error.into_opaque(context)?, context),
    };
    let value = match iter_result_object.get(js_string!("value"), context) {
        Ok(value) => value,
        Err(error) => return rejected_promise(error.into_opaque(context)?, context),
    };
    let value_promise = match promise_from_value(value, context) {
        Ok(value_promise) => value_promise,
        Err(error) => return rejected_promise(error.into_opaque(context)?, context),
    };
    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, args, done: &bool, context| {
            Ok(create_iter_result_object(
                args.get_or_undefined(0).clone(),
                *done,
                context,
            ))
        },
        done,
    )
    .to_js_function(context.realm());

    Ok(JsPromise::from_object(value_promise)?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

fn get_optional_callable_method_value(
    value: JsValue,
    description: &'static str,
) -> JsResult<Option<JsObject>> {
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }

    let method = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message(format!(
            "{description} must be callable when provided"
        ))
    })?;
    if !method.is_callable() {
        return Err(JsNativeError::typ()
            .with_message(format!(
                "{description} must be callable when provided"
            ))
            .into());
    }

    Ok(Some(method.clone()))
}

fn get_required_callable_method(
    object: &JsObject,
    property: &'static str,
    message: &'static str,
    context: &mut Context,
) -> JsResult<JsObject> {
    get_optional_callable_method_value(object.get(js_string!(property), context)?, message)?
        .ok_or_else(|| JsNativeError::typ().with_message(message).into())
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
    // Note: The byte-stream controller's cancel steps own any pending BYOB read-into cleanup.
    let _ = reader;

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

    match &reader {
        ReadableStreamReader::Default(reader) => {
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
        }
        ReadableStreamReader::BYOB(reader) => {
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                resolvers
                    .resolve
                    .call(&JsValue::undefined(), &[JsValue::undefined()], context)?;
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

    match &reader {
        ReadableStreamReader::Default(reader) => {
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
            readable_stream_default_reader_error_read_requests(reader.clone(), error, context)
        }
        ReadableStreamReader::BYOB(reader) => {
            if let Some(resolvers) = reader.closed_resolvers_slot_value() {
                resolvers
                    .reject
                    .call(&JsValue::undefined(), &[error.clone()], context)?;
                reader.set_closed_resolvers_slot_value(None);
            }

            if let Some(closed_promise) = reader.closed_promise_slot_value() {
                mark_promise_as_handled(&closed_promise, context)?;
            }

            Ok(())
        }
    }
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
            reader.read_with_reaction(on_fulfilled, on_rejected, context)?;
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

    let (branch1, branch1_object) = create_readable_stream(
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
    let (branch2, branch2_object) = create_readable_stream(
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
        [branch1_object, branch2_object].into_iter().map(JsValue::from),
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

struct PipeOptions {
    prevent_abort: bool,
    prevent_cancel: bool,
    prevent_close: bool,
    signal: Option<AbortSignal>,
}

fn normalize_pipe_options(options: &JsValue, context: &mut Context) -> JsResult<PipeOptions> {
    let options_object = if options.is_undefined() || options.is_null() {
        None
    } else {
        Some(options.to_object(context)?)
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

    let prevent_close = match options_object.as_ref() {
        Some(options_object) => options_object
            .get(js_string!("preventClose"), context)?
            .to_boolean(),
        None => false,
    };

    let signal = extract_abort_signal(options_object.as_ref(), context)?;

    Ok(PipeOptions {
        prevent_abort,
        prevent_cancel,
        prevent_close,
        signal,
    })
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
    // Note: The pipe progress below follows a single typed state machine and advances from Boa promise reactions at each microtask.
    let state = PipeToState::new(PipeToStateInner {
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
            state.run_abort_algorithm(context)?;
            return Ok(pipe_promise_obj);
        }

        // Step 14.3: "Add abortAlgorithm to signal."
        signal.add_abort_algorithm(abort_algorithm);
    }

    // Step 16: "Return promise."
    state.check_and_propagate_errors_forward(context)?;
    state.check_and_propagate_errors_backward(context)?;
    state.check_and_propagate_closing_forward(context)?;
    state.check_and_propagate_closing_backward(context)?;

    if state.is_shutting_down() {
        return Ok(pipe_promise_obj);
    }

    state.wait_for_writer_ready(context)?;

    Ok(pipe_promise_obj)
}

#[derive(Clone, Trace, Finalize)]
pub(crate) struct PipeToState(Gc<GcRefCell<PipeToStateInner>>);

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
    resolvers: Option<ResolvingFunctions>,

    #[unsafe_ignore_trace]
    shutting_down: bool,
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
                    JsNativeError::typ().with_message(
                        "ReadableStreamPipeTo abort algorithm ran without an attached AbortSignal",
                    )
                })?
        };

        self.set_shutdown_error(Some(error));
        self.shutdown(Some(PipeShutdownAction::Abort), context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_for_writer_ready(&self, context: &mut Context) -> JsResult<()> {
        self.set_state(PipePumpState::PendingReady);

        let (writer, reader) = {
            let state = self.borrow();
            (state.writer.clone(), state.reader.clone())
        };
        let ready_promise = writer.ready()?;
        let reader_closed_promise = reader.closed()?;

        if matches!(
            JsPromise::from_object(ready_promise.clone())?.state(),
            PromiseState::Fulfilled(_)
        ) {
            return self.read_chunk(context);
        }

        self.append_reaction(ready_promise, context)?;
        self.append_reaction(reader_closed_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn read_chunk(&self, context: &mut Context) -> JsResult<()> {
        self.set_state(PipePumpState::PendingRead);

        let (reader, writer) = {
            let state = self.borrow();
            (state.reader.clone(), state.writer.clone())
        };
        let on_fulfilled = pipe_reaction_function(self.clone(), context);
        let on_rejected = pipe_reaction_function(self.clone(), context);
        reader.read_with_reaction(on_fulfilled, on_rejected, context)?;
        let writer_closed_promise = writer.closed()?;

        self.append_reaction(writer_closed_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn write_chunk(&self, result: JsValue, context: &mut Context) -> JsResult<bool> {
        let Some(result_object) = result.as_object() else {
            return Ok(false);
        };

        if !result_object.has_property(js_string!("done"), context)? {
            return Ok(false);
        }

        if result_object
            .get(js_string!("done"), context)?
            .to_boolean()
        {
            return Ok(false);
        }

        let value = result_object.get(js_string!("value"), context)?;
        let writer = {
            let state = self.borrow();
            state.writer.clone()
        };
        let write_promise = writer.write(value, context)?;
        self.borrow_mut().pending_writes.push_back(write_promise);
        Ok(true)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn wait_on_pending_write(&self, promise: JsObject, context: &mut Context) -> JsResult<()> {
        self.append_reaction(promise, context)
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_forward(&self, context: &mut Context) -> JsResult<()> {
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
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::AbortDestination), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_errors_backward(&self, context: &mut Context) -> JsResult<()> {
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
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_forward(&self, context: &mut Context) -> JsResult<()> {
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
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CloseDestination), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
    fn check_and_propagate_closing_backward(&self, context: &mut Context) -> JsResult<()> {
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
            context,
        )?;
        self.set_shutdown_error(Some(error));
        if prevent_cancel {
            self.shutdown(None, context)
        } else {
            self.shutdown(Some(PipeShutdownAction::CancelSource), context)
        }
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    /// Note: This also covers <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown> when `action` is `None`.
    fn shutdown(
        &self,
        action: Option<PipeShutdownAction>,
        context: &mut Context,
    ) -> JsResult<()> {
        let pending_write = {
            let mut state = self.borrow_mut();
            if state.shutting_down {
                return Ok(());
            }

            state.shutting_down = true;

            let should_wait = state
                .writer
                .stream_slot_value()
                .is_some_and(|dest| {
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
            return self.wait_on_pending_write(pending_write, context);
        }

        if let Some(action) = action {
            return self.perform_action(action, context);
        }

        self.finalize(context)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-shutdown-with-action>
    fn perform_action(&self, action: PipeShutdownAction, context: &mut Context) -> JsResult<()> {
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
                    .unwrap_or_else(JsValue::undefined),
                state.prevent_abort,
                state.prevent_cancel,
            )
        };

        let action_promise = match action {
            PipeShutdownAction::AbortDestination => match dest {
                Some(dest) => dest.abort_stream(error, context)?,
                None => resolved_promise(JsValue::undefined(), context)?,
            },
            PipeShutdownAction::CancelSource => match source {
                Some(source) => readable_stream_cancel(source, error, context)?,
                None => resolved_promise(JsValue::undefined(), context)?,
            },
            PipeShutdownAction::CloseDestination => match dest {
                Some(dest)
                    if dest.state() == super::WritableStreamState::Closed
                        || dest.close_queued_or_in_flight() =>
                {
                    resolved_promise(JsValue::undefined(), context)?
                }
                _ => writer.close(context)?,
            },
            PipeShutdownAction::Abort => {
                let abort_promise = if !prevent_abort {
                    match dest {
                        Some(dest) if dest.state() == super::WritableStreamState::Writable => {
                            Some(dest.abort_stream(error.clone(), context)?)
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
                        abort_destination_then_cancel_source(
                            abort_promise,
                            source,
                            error,
                            context,
                        )?
                    }
                    (Some(abort_promise), None) => abort_promise,
                    (None, Some(source)) => readable_stream_cancel(source, error, context)?,
                    (None, None) => resolved_promise(JsValue::undefined(), context)?,
                }
            }
        };

        self.borrow_mut().shutdown_action_promise = Some(action_promise.clone());
        self.append_reaction(action_promise, context)
    }

    /// <https://streams.spec.whatwg.org/#rs-pipeTo-finalize>
    fn finalize(&self, context: &mut Context) -> JsResult<()> {
        if self.current_state() == PipePumpState::Finalized {
            return Ok(());
        }

        let (writer, reader, signal, error, resolvers) = {
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

        super::writable_stream_default_writer_release(writer, context)?;
        super::readable_stream_default_reader_release(reader, context)?;

        if let Some(signal) = signal {
            signal.remove_abort_algorithm(&SignalAbortAlgorithm::ReadableStreamPipeTo {
                state: self.clone(),
            });
        }

        if let Some(resolvers) = resolvers {
            match error {
                Some(error) => {
                    resolvers
                        .reject
                        .call(&JsValue::undefined(), &[error], context)?;
                }
                None => {
                    resolvers.resolve.call(
                        &JsValue::undefined(),
                        &[JsValue::undefined()],
                        context,
                    )?;
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
        context: &mut Context,
    ) -> JsResult<Option<PipeShutdownAction>> {
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
            return Ok((!prevent_cancel && source_is_readable)
                .then_some(PipeShutdownAction::CancelSource));
        }

        if dest.state() == super::WritableStreamState::Closed || dest.close_queued_or_in_flight() {
            if !source_is_readable {
                return Ok(None);
            }

            let error = type_error_value(
                "The destination WritableStream closed before the pipe operation completed",
                context,
            )?;
            self.set_shutdown_error(Some(error));
            return Ok((!prevent_cancel && source_is_readable)
                .then_some(PipeShutdownAction::CancelSource));
        }

        Ok(action)
    }

    fn pending_write_front(&self) -> Option<JsObject> {
        self.borrow().pending_writes.front().cloned()
    }

    fn shutdown_action_promise_state(&self) -> JsResult<Option<PromiseState>> {
        self.borrow()
            .shutdown_action_promise
            .clone()
            .map(|promise| Ok(JsPromise::from_object(promise)?.state()))
            .transpose()
    }

    fn prune_settled_pending_writes(&self, context: &mut Context) -> JsResult<()> {
        let mut handled = Vec::new();
        {
            let mut state = self.borrow_mut();
            state.pending_writes.retain(|promise_object| {
                let promise = match JsPromise::from_object(promise_object.clone()) {
                    Ok(promise) => promise,
                    Err(_) => {
                        debug_assert!(false, "pipeTo tracked a non-promise write handle");
                        return false;
                    }
                };
                let pending = matches!(promise.state(), PromiseState::Pending);
                if !pending {
                    handled.push(promise_object.clone());
                }
                pending
            });
        }

        for promise in handled {
            mark_promise_as_handled(&promise, context)?;
        }

        Ok(())
    }

    fn append_reaction(&self, promise: JsObject, context: &mut Context) -> JsResult<()> {
        let on_fulfilled = pipe_reaction_function(self.clone(), context);
        let on_rejected = pipe_reaction_function(self.clone(), context);
        let _ = JsPromise::from_object(promise)?
            .then(Some(on_fulfilled), Some(on_rejected), context)?;
        Ok(())
    }
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

#[derive(Trace, Finalize)]
struct AbortThenCancelState {
    source: Option<ReadableStream>,
    error: JsValue,
    abort_rejection: Option<JsValue>,
    resolvers: ResolvingFunctions,
}

/// <https://streams.spec.whatwg.org/#readable-stream-pipe-to>
fn pipe_to_on_promise_settled(
    state: PipeToState,
    result: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    state.prune_settled_pending_writes(context)?;

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
                        let Some(done) = pipe_read_result_done(&result, context)? else {
                            return Ok(());
                        };

                        if !done {
                            let _ = state.write_chunk(result.clone(), context)?;
                        }
                    }
                }
            }
        }
    }

    state.check_and_propagate_errors_forward(context)?;
    state.check_and_propagate_errors_backward(context)?;
    state.check_and_propagate_closing_forward(context)?;
    state.check_and_propagate_closing_backward(context)?;

    let current_state = state.current_state();
    if current_state != state_before_checks {
        return Ok(());
    }

    match current_state {
        PipePumpState::Starting => {
            debug_assert!(false, "ReadableStream pipeTo callback reached the Starting state");
        }
        PipePumpState::PendingReady => {
            state.read_chunk(context)?;
        }
        PipePumpState::PendingRead => {
            let _ = state.write_chunk(result, context)?;
            if state.is_shutting_down() {
                return Ok(());
            }
            state.wait_for_writer_ready(context)?;
        }
        PipePumpState::ShuttingDownWithPendingWrites(action) => {
            let action = state.update_pending_shutdown_action(action, context)?;
            state.set_state(PipePumpState::ShuttingDownWithPendingWrites(action));

            if let Some(pending_write) = state.pending_write_front() {
                state.wait_on_pending_write(pending_write, context)?;
            } else if let Some(action) = action {
                state.perform_action(action, context)?;
            } else {
                state.finalize(context)?;
            }
        }
        PipePumpState::ShuttingDownPendingAction(action) => {
            match state.shutdown_action_promise_state()? {
                Some(PromiseState::Pending) => return Ok(()),
                Some(PromiseState::Rejected(error)) => state.set_shutdown_error(Some(error)),
                Some(PromiseState::Fulfilled(value)) => {
                    if action != PipeShutdownAction::Abort && !value.is_undefined() {
                        state.set_shutdown_error(Some(value));
                    }
                }
                None => {}
            }

            state.finalize(context)?;
        }
        PipePumpState::Finalized => {}
    }

    Ok(())
}

fn pipe_reaction_function(state: PipeToState, context: &mut Context) -> JsFunction {
    NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &PipeToState, context| {
            pipe_to_on_promise_settled(state.clone(), args.get_or_undefined(0).clone(), context)?;
            Ok(JsValue::undefined())
        },
        state,
    )
    .to_js_function(context.realm())
}

fn pipe_read_result_done(result: &JsValue, context: &mut Context) -> JsResult<Option<bool>> {
    let Some(result_object) = result.as_object() else {
        return Ok(None);
    };

    if !result_object.has_property(js_string!("done"), context)? {
        return Ok(None);
    }

    Ok(Some(
        result_object
            .get(js_string!("done"), context)?
            .to_boolean(),
    ))
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

fn abort_destination_then_cancel_source(
    abort_promise: JsObject,
    source: ReadableStream,
    error: JsValue,
    context: &mut Context,
) -> JsResult<JsObject> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    let state = Gc::new(GcRefCell::new(AbortThenCancelState {
        source: Some(source),
        error,
        abort_rejection: None,
        resolvers,
    }));

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, state: &Gc<GcRefCell<AbortThenCancelState>>, context| {
            start_abort_cancel_source(state.clone(), None, context)
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &Gc<GcRefCell<AbortThenCancelState>>, context| {
            start_abort_cancel_source(
                state.clone(),
                Some(args.get_or_undefined(0).clone()),
                context,
            )
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(abort_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;

    Ok(promise.into())
}

fn start_abort_cancel_source(
    state: Gc<GcRefCell<AbortThenCancelState>>,
    abort_rejection: Option<JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (source, error) = {
        let mut state_ref = state.borrow_mut();
        state_ref.abort_rejection = abort_rejection;
        (state_ref.source.take(), state_ref.error.clone())
    };

    let cancel_promise = match source {
        Some(source) => readable_stream_cancel(source, error, context)?,
        None => resolved_promise(JsValue::undefined(), context)?,
    };

    let on_fulfilled = NativeFunction::from_copy_closure_with_captures(
        |_, _, state: &Gc<GcRefCell<AbortThenCancelState>>, context| {
            finalize_abort_cancel_source(state.clone(), None, context)
        },
        state.clone(),
    )
    .to_js_function(context.realm());
    let on_rejected = NativeFunction::from_copy_closure_with_captures(
        |_, args, state: &Gc<GcRefCell<AbortThenCancelState>>, context| {
            finalize_abort_cancel_source(
                state.clone(),
                Some(args.get_or_undefined(0).clone()),
                context,
            )
        },
        state,
    )
    .to_js_function(context.realm());
    let _ = JsPromise::from_object(cancel_promise)?.then(Some(on_fulfilled), Some(on_rejected), context)?;
    Ok(JsValue::undefined())
}

fn finalize_abort_cancel_source(
    state: Gc<GcRefCell<AbortThenCancelState>>,
    cancel_rejection: Option<JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (abort_rejection, resolvers) = {
        let state_ref = state.borrow();
        (state_ref.abort_rejection.clone(), state_ref.resolvers.clone())
    };

    if let Some(reason) = abort_rejection.or(cancel_rejection) {
        resolvers.reject.call(&JsValue::undefined(), &[reason], context)?;
    } else {
        resolvers.resolve.call(
            &JsValue::undefined(),
            &[JsValue::undefined()],
            context,
        )?;
    }

    Ok(JsValue::undefined())
}
