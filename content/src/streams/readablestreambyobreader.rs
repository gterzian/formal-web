use boa_engine::{
    Context, JsArgs, JsData, JsNativeError, JsResult, JsValue,
    builtins::promise::ResolvingFunctions,
    class::Class,
    object::JsObject,
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};

use super::{
    ArrayBufferViewDescriptor, ReadIntoRequest, ReadableStream, ReadableStreamGenericReader,
    ReadableStreamReader, ReadableStreamState, rejected_type_error_promise,
    with_readable_stream_ref,
};

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
        context: &mut Context,
    ) -> JsResult<()> {
        if stream.is_readable_stream_locked() {
            return Err(JsNativeError::typ()
                .with_message("Cannot create a BYOB reader for a locked stream")
                .into());
        }

        let Some(controller) = stream.controller_slot() else {
            return Err(JsNativeError::typ()
                .with_message("ReadableStream is missing its controller")
                .into());
        };
        if controller.as_byte_controller().is_none() {
            return Err(JsNativeError::typ()
                .with_message("ReadableStreamBYOBReader requires a byte stream")
                .into());
        }

        self.readable_stream_reader_generic_initialize(stream, context)
    }

    pub(crate) fn closed(&self) -> JsResult<JsObject> {
        <Self as ReadableStreamGenericReader>::closed(self)
    }

    pub(crate) fn cancel(&self, reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
        <Self as ReadableStreamGenericReader>::cancel(self, reason, context)
    }

    pub(crate) fn read(
        &self,
        view: &JsValue,
        options: &JsValue,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        if self.stream_slot_value().is_none() {
            return rejected_type_error_promise("Cannot read from a released reader", context);
        }

        let view = ArrayBufferViewDescriptor::from_value(view.clone(), context)?;
        if view.byte_length() == 0 {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-empty view",
                context,
            );
        }

        let min = normalize_min(options, &view, context)?;
        let (read_into_request, promise) = ReadIntoRequest::new(context);
        self.read_steps(view, min, read_into_request, context)?;
        Ok(promise)
    }

    fn read_steps(
        &self,
        view: ArrayBufferViewDescriptor,
        min: usize,
        read_into_request: ReadIntoRequest,
        context: &mut Context,
    ) -> JsResult<()> {
        let stream = self.stream_slot_value().ok_or_else(|| {
            JsNativeError::typ().with_message("reader is not attached to a stream")
        })?;

        stream.set_disturbed(true);

        if stream.state() == ReadableStreamState::Closed {
            return read_into_request.close_steps(
                Some(JsValue::from(view.create_result_view(0, context)?)),
                context,
            );
        }

        if stream.state() == ReadableStreamState::Errored {
            return read_into_request.error_steps(stream.stored_error(), context);
        }

        let controller = stream.controller_slot().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream is missing its controller")
        })?;
        let controller = controller.as_byte_controller().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamBYOBReader requires a byte stream")
        })?;
        controller.pull_into(view, min, read_into_request, context)
    }

    pub(crate) fn release_lock(&self, context: &mut Context) -> JsResult<()> {
        if self.stream_slot_value().is_none() {
            return Ok(());
        }
        self.readable_stream_reader_generic_release(context)
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

pub(crate) fn construct_readable_stream_byob_reader(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ReadableStreamBYOBReader> {
    let stream_object = args.get_or_undefined(0).as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBReader requires a ReadableStream")
    })?;
    let stream = with_readable_stream_ref(&stream_object, |stream| stream.clone())?;
    let reader = ReadableStreamBYOBReader::new();
    reader.set_up_readable_stream_byob_reader(stream, context)?;
    Ok(reader)
}

pub(crate) fn acquire_readable_stream_byob_reader(
    stream: ReadableStream,
    context: &mut Context,
) -> JsResult<JsObject> {
    let reader_object = create_readable_stream_byob_reader(context)?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.clone())?;
    reader.set_up_readable_stream_byob_reader(stream, context)?;
    Ok(reader_object)
}

fn create_readable_stream_byob_reader(context: &mut Context) -> JsResult<JsObject> {
    let reader = ReadableStreamBYOBReader::new();
    let reader_object: JsObject = ReadableStreamBYOBReader::from_data(reader, context)?.into();
    Ok(reader_object)
}

pub(crate) fn with_readable_stream_byob_reader_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&ReadableStreamBYOBReader) -> R,
) -> JsResult<R> {
    let reader = object.downcast_ref::<ReadableStreamBYOBReader>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableStreamBYOBReader")
    })?;
    Ok(f(&reader))
}

fn normalize_min(
    options: &JsValue,
    view: &ArrayBufferViewDescriptor,
    context: &mut Context,
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
                return Err(JsNativeError::typ().with_message("min must be a positive integer").into());
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