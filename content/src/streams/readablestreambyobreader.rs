use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::{
    ArrayBufferViewDescriptor, ReadIntoRequest, ReadableStream, ReadableStreamGenericReader,
    ReadableStreamReader, ReadableStreamState, rejected_type_error_promise, type_error_value,
    with_readable_stream_ref,
};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://streams.spec.whatwg.org/#byob-reader-class>
#[gc_struct]
pub struct ReadableStreamBYOBReader {
    stream: GcCell<Option<ReadableStream>>,
    closed_promise: GcCell<Option<JsObject>>,
    closed_resolvers: GcCell<Option<PromiseResolvers<Types>>>,
}

impl ReadableStreamBYOBReader {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            stream: gc_cell_new(None, ec),
            closed_promise: gc_cell_new(None, ec),
            closed_resolvers: gc_cell_new(None, ec),
        }
    }

    pub(crate) fn set_up_readable_stream_byob_reader(
        &self,
        stream: ReadableStream,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if stream.is_readable_stream_locked(ec) {
            return Err(ec.new_type_error("Cannot create a BYOB reader for a locked stream"));
        }

        let Some(controller) = stream.controller_slot(ec) else {
            return Err(ec.new_type_error("ReadableStream is missing its controller"));
        };
        if controller.as_byte_controller().is_none() {
            return Err(ec.new_type_error("ReadableStreamBYOBReader requires a byte stream"));
        }

        self.readable_stream_reader_generic_initialize(stream, ec)
    }

    pub(crate) fn closed(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        <Self as ReadableStreamGenericReader>::closed(self, ec)
    }

    pub(crate) fn cancel(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        <Self as ReadableStreamGenericReader>::cancel(self, reason, ec)
    }

    pub(crate) fn read(
        &self,
        view: &JsValue,
        options: &JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        let view = match ArrayBufferViewDescriptor::from_value(view.clone(), ec) {
            Ok(view) => view,
            // The spec checks the detached-buffer and zero-length cases below
            // and returns a rejected promise for them, not a synchronous throw.
            Err(_) => return rejected_type_error_promise("Invalid ArrayBufferView", ec),
        };

        // Step 1: If view.[[ByteLength]] is 0, return a promise rejected with a TypeError exception.
        if view.byte_length() == 0 {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-empty view",
                ec,
            );
        }

        // Step 2: If view.[[ViewedArrayBuffer]].[[ByteLength]] is 0, return a promise rejected with a TypeError exception.
        if view.buffer_byte_length() == 0 {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-zero-length buffer",
                ec,
            );
        }

        // Step 3: If ! IsDetachedBuffer(view.[[ViewedArrayBuffer]]) is true, return a promise rejected with a TypeError exception.
        if ec.array_buffer_data(view.buffer()).is_none() {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-detached view buffer",
                ec,
            );
        }

        // Step 4: If options["min"] is 0, return a promise rejected with a TypeError exception.
        // Steps 5-6: If min > view length, return a promise rejected with a RangeError exception.
        let min = match normalize_min(options, &view, ec) {
            Ok(min) => min,
            Err(error) => return crate::webidl::rejected_promise(error, ec),
        };

        // Step 7: If this.[[stream]] is undefined, return a promise rejected with a TypeError exception.
        if self.stream_slot_value(ec).is_none() {
            return rejected_type_error_promise("Cannot read from a released reader", ec);
        }

        // Step 8: Let promise be a new promise.
        let (read_into_request, promise) = ReadIntoRequest::new(ec)?;

        // Step 10: Perform ! ReadableStreamBYOBReaderRead(this, view, options["min"], readIntoRequest).
        self.read_steps(view, min, read_into_request, ec)?;

        // Step 11: Return promise.
        Ok(promise)
    }

    pub(crate) fn read_steps(
        &self,
        view: ArrayBufferViewDescriptor,
        min: usize,
        read_into_request: ReadIntoRequest,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let not_attached = ec.new_type_error("reader is not attached to a stream");
        let stream = self.stream_slot_value(ec).ok_or(not_attached)?;

        // Step 3: Set stream.[[disturbed]] to true.
        stream.set_disturbed(true);

        // Step 4: If stream.[[state]] is "errored",
        if stream.state() == ReadableStreamState::Errored {
            // perform readIntoRequest's error steps, given stream.[[storedError]], and return.
            return read_into_request.error_steps(stream.stored_error(ec), ec);
        }

        // Step 6: If stream.[[state]] is "closed",
        if stream.state() == ReadableStreamState::Closed {
            // Step 6.1: Let emptyView be ! ConstructEmptyArrayBufferView(view.[[ArrayBufferByteLength]],
            //           view.[[ByteOffset]], view.[[ViewedArrayBuffer]]).
            let transferred_buffer =
                crate::streams::readablebytestreamcontroller::transfer_array_buffer(
                    view.buffer().clone(),
                    ec,
                )?;
            let result_view = view.create_result_view_on(transferred_buffer, 0, ec)?;
            // Step 6.2: Perform readIntoRequest's close steps, given emptyView.
            return read_into_request.close_steps(Some(JsValue::from(result_view)), ec);
        }

        let no_ctrl = ec.new_type_error("ReadableStream is missing its controller");
        let controller = stream.controller_slot(ec).ok_or_else(|| no_ctrl.clone())?;
        let not_byte = ec.new_type_error("ReadableStreamBYOBReader requires a byte stream");
        let controller = controller.as_byte_controller().ok_or(not_byte)?;
        controller.pull_into(view, min, read_into_request, ec)
    }

    pub(crate) fn release_lock(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if self.stream_slot_value(ec).is_none() {
            return Ok(());
        }
        // <https://streams.spec.whatwg.org/#abstract-opdef-readablestreambyobreaderrelease>
        readable_stream_byob_reader_release(self.clone(), ec)
    }
}

impl ReadableStreamGenericReader for ReadableStreamBYOBReader {
    fn stream_slot_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<ReadableStream> {
        self.stream.borrow(ec).clone()
    }

    fn set_stream_slot_value(
        &self,
        stream: Option<ReadableStream>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        *self.stream.borrow_mut(ec) = stream;
    }

    fn closed_promise_slot_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<JsObject> {
        self.closed_promise.borrow(ec).clone()
    }

    fn set_closed_promise_slot_value(
        &self,
        promise: Option<JsObject>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // JSC: protect new value from GC, unprotect old value
        #[cfg(feature = "jsc")]
        {
            let old = self.closed_promise.borrow(ec).clone();
            if let Some(ref old_obj) = old {
                unsafe {
                    js_engine::jsc_sys::JSValueUnprotect(old_obj.ctx(), old_obj.as_value_ref());
                }
            }
            if let Some(ref new_obj) = promise {
                unsafe {
                    js_engine::jsc_sys::JSValueProtect(new_obj.ctx(), new_obj.as_value_ref());
                }
            }
        }
        *self.closed_promise.borrow_mut(ec) = promise;
    }

    fn closed_resolvers_slot_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<PromiseResolvers<Types>> {
        self.closed_resolvers.borrow(ec).clone()
    }

    fn set_closed_resolvers_slot_value(
        &self,
        resolvers: Option<PromiseResolvers<Types>>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        *self.closed_resolvers.borrow_mut(ec) = resolvers;
    }

    fn as_reader_slot(&self) -> ReadableStreamReader {
        ReadableStreamReader::BYOB(self.clone())
    }
}

/// <https://streams.spec.whatwg.org/#byob-reader-constructor>
pub(crate) fn construct_readable_stream_byob_reader(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<ReadableStreamBYOBReader, Types> {
    let stream_object = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined())
        .as_object()
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader requires a ReadableStream"))?;
    let stream = with_readable_stream_ref(&stream_object, ec, |stream: &ReadableStream, _ec| {
        stream.clone()
    })?;
    let reader = ReadableStreamBYOBReader::new(ec);
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader)
}

/// <https://streams.spec.whatwg.org/#acquire-readable-stream-byob-reader>
pub(crate) fn acquire_readable_stream_byob_reader(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let reader_object = create_readable_stream_byob_reader(ec)?;
    let reader =
        with_readable_stream_byob_reader_ref(&reader_object, ec, |reader, _ec| reader.clone())?;
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader_object)
}

fn create_readable_stream_byob_reader(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let reader = ReadableStreamBYOBReader::new(ec);
    let reader_object: JsObject =
        create_interface_instance::<Types, ReadableStreamBYOBReader>(reader, ec)?;
    Ok(reader_object)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreambyobreaderrelease>
pub(crate) fn readable_stream_byob_reader_release(
    reader: ReadableStreamBYOBReader,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Steps 2-3: Let e be a new TypeError exception.  Perform !
    //            ReadableStreamBYOBReaderErrorReadIntoRequests(reader, e).
    // Note: In this implementation the read-into requests live inside the
    // pending pull-into descriptors, so they are errored before generic
    // release marks the first descriptor's reader type as "none".
    let release_error = type_error_value("Reader was released", ec)?;
    if let Some(controller) = reader
        .stream_slot_value(ec)
        .and_then(|stream| stream.controller_slot(ec))
        .and_then(|controller| controller.as_byte_controller())
    {
        controller.error_pending_read_into_requests(release_error, ec)?;
    }

    // Step 1: "Perform ! ReadableStreamReaderGenericRelease(reader)."
    reader.readable_stream_reader_generic_release(ec)
}

pub(crate) fn with_readable_stream_byob_reader_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&ReadableStreamBYOBReader, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let reader = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamBYOBReader>().cloned());
    let Some(reader) = reader else {
        return Err(ec.new_type_error("object is not a ReadableStreamBYOBReader"));
    };
    Ok(f(&reader, ec))
}

fn normalize_min(
    options: &JsValue,
    view: &ArrayBufferViewDescriptor,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<usize, Types> {
    use js_engine::EcmascriptHost;
    let options_object = if options.is_undefined() || options.is_null() {
        None
    } else {
        Some(ec.to_object(options.clone())?)
    };
    let min = if let Some(options_object) = options_object {
        let min_value = EcmascriptHost::get(ec, &options_object, "min")?;
        let undefined_value = ec.value_undefined();
        if ec.same_value(&min_value, &undefined_value) {
            1
        } else {
            let min_number = ec.to_number(min_value)?;
            if !min_number.is_finite() || min_number <= 0.0 || min_number.fract() != 0.0 {
                return Err(ec.new_type_error("min must be a positive integer"));
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
        return Err(ec.new_range_error("min exceeds the supplied view length"));
    }
    Ok(min)
}
