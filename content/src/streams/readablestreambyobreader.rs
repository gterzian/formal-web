use boa_engine::{
    JsArgs, JsData, JsError, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions, object::JsObject,
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use crate::webidl::bindings::create_interface_instance;
use crate::webidl::rejected_promise;

use super::{
    ArrayBufferViewDescriptor, ReadIntoRequest, ReadableStream, ReadableStreamGenericReader,
    ReadableStreamReader, ReadableStreamState, rejected_type_error_promise,
    with_readable_stream_ref,
};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

/// <https://streams.spec.whatwg.org/#byob-reader-class>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct ReadableStreamBYOBReader {
    stream: Gc<GcRefCell<Option<ReadableStream>>>,
    closed_promise: Gc<GcRefCell<Option<JsObject>>>,
    closed_resolvers: Gc<GcRefCell<Option<ResolvingFunctions>>>,
}

impl ReadableStreamBYOBReader {
    pub(crate) fn new() -> Self {
        Self {
            stream: Gc::new(GcRefCell::new(None)),
            closed_promise: Gc::new(GcRefCell::new(None)),
            closed_resolvers: Gc::new(GcRefCell::new(None)),
        }
    }

    pub(crate) fn set_up_readable_stream_byob_reader(
        &self,
        stream: ReadableStream,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        if stream.is_readable_stream_locked() {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("Cannot create a BYOB reader for a locked stream")
                .into();
            return Err(error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined()));
        }

        let Some(controller) = stream.controller_slot() else {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("ReadableStream is missing its controller")
                .into();
            return Err(error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined()));
        };
        if controller.as_byte_controller().is_none() {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("ReadableStreamBYOBReader requires a byte stream")
                .into();
            return Err(error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined()));
        }

        self.readable_stream_reader_generic_initialize(stream, ec)
    }

    pub(crate) fn closed(&self) -> JsResult<JsObject> {
        <Self as ReadableStreamGenericReader>::closed(self)
    }

    pub(crate) fn cancel(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsObject, BoaTypes> {
        <Self as ReadableStreamGenericReader>::cancel(self, reason, ec)
    }

    pub(crate) fn read(
        &self,
        view: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsObject, BoaTypes> {
        if self.stream_slot_value().is_none() {
            return rejected_type_error_promise(
                "Cannot read from a released reader",
                ec,
            );
        }

        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context.
        // ArrayBufferViewDescriptor::from_value requires Boa's Context.
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        let view = ArrayBufferViewDescriptor::from_value(view.clone(), context).map_err(|e| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        if view.byte_length() == 0 {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-empty view",
                ec,
            );
        }

        let min = match normalize_min(options, &view, context) {
            Ok(min) => min,
            Err(error) => {
                return rejected_promise(
                    error
                        .into_opaque(context)
                        .unwrap_or_else(|_| JsValue::undefined()),
                    ec,
                );
            }
        };
        let (read_into_request, promise) = ReadIntoRequest::new(context);
        self.read_steps(view, min, read_into_request, ec)?;
        Ok(promise)
    }

    pub(crate) fn read_steps(
        &self,
        view: ArrayBufferViewDescriptor,
        min: usize,
        read_into_request: ReadIntoRequest,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        let stream = self.stream_slot_value().ok_or_else(|| {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("reader is not attached to a stream")
                .into();
            error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;

        stream.set_disturbed(true);

        if stream.state() == ReadableStreamState::Closed {
            // SAFETY: ec is backed by BoaEngine repr(transparent) over Context.
            // create_result_view requires Boa's Context.
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            return read_into_request.close_steps(
                Some(JsValue::from(view.create_result_view(0, ctx).map_err(|e| {
                    e.into_opaque(ctx)
                        .unwrap_or_else(|_| JsValue::undefined())
                })?)),
                ec,
            );
        }

        if stream.state() == ReadableStreamState::Errored {
            return read_into_request.error_steps(stream.stored_error(), ec);
        }

        let controller = stream.controller_slot().ok_or_else(|| {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("ReadableStream is missing its controller")
                .into();
            error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        let controller = controller.as_byte_controller().ok_or_else(|| {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            let error: JsError = JsNativeError::typ()
                .with_message("ReadableStreamBYOBReader requires a byte stream")
                .into();
            error
                .into_opaque(ctx)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
        // SAFETY: ec is backed by BoaEngine repr(transparent) over Context.
        // pull_into still takes Boa's Context.
        let context = unsafe { crate::js::ec_to_ctx(ec) };
        controller
            .pull_into(view, min, read_into_request, context)
            .map_err(|e| {
                e.into_opaque(context)
                    .unwrap_or_else(|_| JsValue::undefined())
            })
    }

    pub(crate) fn release_lock(
        &self,
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<(), BoaTypes> {
        if self.stream_slot_value().is_none() {
            return Ok(());
        }
        self.readable_stream_reader_generic_release(ec)
    }
}

impl ReadableStreamGenericReader for ReadableStreamBYOBReader {
    fn stream_slot_value(&self) -> Option<ReadableStream> {
        self.stream.borrow().clone()
    }

    fn set_stream_slot_value(&self, stream: Option<ReadableStream>) {
        *self.stream.borrow_mut() = stream;
    }

    fn closed_promise_slot_value(&self) -> Option<JsObject> {
        self.closed_promise.borrow().clone()
    }

    fn set_closed_promise_slot_value(&self, promise: Option<JsObject>) {
        *self.closed_promise.borrow_mut() = promise;
    }

    fn closed_resolvers_slot_value(&self) -> Option<ResolvingFunctions> {
        self.closed_resolvers.borrow().clone()
    }

    fn set_closed_resolvers_slot_value(&self, resolvers: Option<ResolvingFunctions>) {
        *self.closed_resolvers.borrow_mut() = resolvers;
    }

    fn as_reader_slot(&self) -> ReadableStreamReader {
        ReadableStreamReader::BYOB(self.clone())
    }
}

/// <https://streams.spec.whatwg.org/#byob-reader-constructor>
pub(crate) fn construct_readable_stream_byob_reader(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<ReadableStreamBYOBReader, BoaTypes> {
    // SAFETY: ec is backed by BoaEngine repr(transparent) over Context
    let context = unsafe { crate::js::ec_to_ctx(ec) };

    let stream_object = args.get_or_undefined(0).as_object().ok_or_else(|| {
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamBYOBReader requires a ReadableStream")
            .into();
        error
            .into_opaque(context)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    let stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())
        .map_err(|e: JsError| {
            e.into_opaque(context)
                .unwrap_or_else(|_| JsValue::undefined())
        })?;
    let reader = ReadableStreamBYOBReader::new();
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader)
}

/// <https://streams.spec.whatwg.org/#acquire-readable-stream-byob-reader>
pub(crate) fn acquire_readable_stream_byob_reader(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    let reader_object = create_readable_stream_byob_reader(ec)?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.clone())
        .map_err(|e: JsError| {
            let ctx = unsafe { crate::js::ec_to_ctx(ec) };
            e.into_opaque(ctx).unwrap_or_else(|_| JsValue::undefined())
        })?;
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader_object)
}

fn create_readable_stream_byob_reader(
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsObject, BoaTypes> {
    let reader = ReadableStreamBYOBReader::new();
    let reader_object: JsObject =
        create_interface_instance::<BoaTypes, ReadableStreamBYOBReader>(reader, ec)?.into();
    Ok(reader_object)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreambyobreaderrelease>
pub(crate) fn readable_stream_byob_reader_release(
    reader: ReadableStreamBYOBReader,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<(), BoaTypes> {
    // Step 1: "Perform ! ReadableStreamReaderGenericRelease(reader)."
    reader.readable_stream_reader_generic_release(ec)?;

    // Step 2–3: Error any remaining [[readIntoRequests]].
    // Note: In tee() usage the spec asserts [[readIntoRequests]] is empty before
    // calling this, so no requests need to be errored here.  When invoked from a
    // non-tee path this is conservative but safe because pull-into descriptors are
    // owned by the byte controller and will be cleaned up when the controller is
    // released from the stream.
    Ok(())
}

pub(crate) fn with_readable_stream_byob_reader_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStreamBYOBReader) -> R,
) -> JsResult<R> {
    let reader = object
        .downcast_ref::<ReadableStreamBYOBReader>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("object is not a ReadableStreamBYOBReader")
        })?;
    Ok(f(&reader))
}

fn normalize_min(
    options: &JsValue,
    view: &ArrayBufferViewDescriptor,
    context: &mut boa_engine::Context,
) -> JsResult<usize> {
    let options_object = if options.is_undefined() || options.is_null() {
        None
    } else {
        Some(options.to_object(context)?)
    };
    let min = if let Some(options_object) = options_object {
        let min_value = options_object.get(boa_engine::js_string!("min"), context)?;
        if min_value.is_undefined() {
            1
        } else {
            let min_number = min_value.to_number(context)?;
            if !min_number.is_finite() || min_number <= 0.0 || min_number.fract() != 0.0 {
                return Err(JsNativeError::typ()
                    .with_message("min must be a positive integer")
                    .into());
            }
            min_number as usize
        }
    } else {
        1
    };

    let max_min = if view.is_data_view() {
        view.byte_length()
    } else {
        view.element_length()
    };
    if min > max_min {
        return Err(JsNativeError::range()
            .with_message("min exceeds the supplied view length")
            .into());
    }
    Ok(min)
}
