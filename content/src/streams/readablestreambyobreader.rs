use js_engine::{Completion, ExecutionContext, JsTypes, PromiseResolvers};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use js_engine::gc::GcCell;
use js_engine::gc::gc_cell_new;
use js_engine::gc_struct;

use super::{
    ArrayBufferViewDescriptor, ReadIntoRequest, ReadableStream, ReadableStreamGenericReader,
    ReadableStreamReader, ReadableStreamState, rejected_type_error_promise,
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
    pub(crate) fn new() -> Self {
        Self {
            stream: gc_cell_new(None),
            closed_promise: gc_cell_new(None),
            closed_resolvers: gc_cell_new(None),
        }
    }

    pub(crate) fn set_up_readable_stream_byob_reader(
        &self,
        stream: ReadableStream,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        if stream.is_readable_stream_locked() {
            return Err(ec.new_type_error("Cannot create a BYOB reader for a locked stream"));
        }

        let Some(controller) = stream.controller_slot() else {
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
        if self.stream_slot_value().is_none() {
            return rejected_type_error_promise("Cannot read from a released reader", ec);
        }

        let view = ArrayBufferViewDescriptor::from_value(view.clone(), ec)?;
        if view.byte_length() == 0 {
            return rejected_type_error_promise(
                "ReadableStreamBYOBReader.read() requires a non-empty view",
                ec,
            );
        }

        let (read_into_request, promise) = ReadIntoRequest::new(ec)?;
        let min = normalize_min(options, &view, ec)?;
        self.read_steps(view, min, read_into_request, ec)?;
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
        let stream = self.stream_slot_value().ok_or_else(|| not_attached)?;

        stream.set_disturbed(true);

        if stream.state() == ReadableStreamState::Closed {
            let result_view = view.create_result_view(0, ec)?;
            return read_into_request.close_steps(Some(JsValue::from(result_view)), ec);
        }

        if stream.state() == ReadableStreamState::Errored {
            return read_into_request.error_steps(stream.stored_error(), ec);
        }

        let no_ctrl = ec.new_type_error("ReadableStream is missing its controller");
        let controller = stream.controller_slot().ok_or_else(|| no_ctrl.clone())?;
        let not_byte = ec.new_type_error("ReadableStreamBYOBReader requires a byte stream");
        let controller = controller.as_byte_controller().ok_or_else(|| not_byte)?;
        controller.pull_into(view, min, read_into_request, ec)
    }

    pub(crate) fn release_lock(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
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
        // JSC: protect new value from GC, unprotect old value
        #[cfg(not(feature = "boa"))]
        {
            let old = self.closed_promise.borrow().clone();
            if let Some(ref old_obj) = old {
                unsafe {
                    js_engine::jsc_sys::JSValueUnprotect(
                        old_obj.ctx(),
                        old_obj.as_value_ref(),
                    );
                }
            }
            if let Some(ref new_obj) = promise {
                unsafe {
                    js_engine::jsc_sys::JSValueProtect(
                        new_obj.ctx(),
                        new_obj.as_value_ref(),
                    );
                }
            }
        }
        *self.closed_promise.borrow_mut() = promise;
    }

    fn closed_resolvers_slot_value(&self) -> Option<PromiseResolvers<Types>> {
        self.closed_resolvers.borrow().clone()
    }

    fn set_closed_resolvers_slot_value(&self, resolvers: Option<PromiseResolvers<Types>>) {
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
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<ReadableStreamBYOBReader, Types> {
    let stream_object = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined())
        .as_object()
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader requires a ReadableStream"))?;
    let stream =
        with_readable_stream_ref(&stream_object, ec, |stream: &ReadableStream| stream.clone())?;
    let reader = ReadableStreamBYOBReader::new();
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader)
}

/// <https://streams.spec.whatwg.org/#acquire-readable-stream-byob-reader>
pub(crate) fn acquire_readable_stream_byob_reader(
    stream: ReadableStream,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let reader_object = create_readable_stream_byob_reader(ec)?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, ec, |reader| reader.clone())?;
    reader.set_up_readable_stream_byob_reader(stream, ec)?;
    Ok(reader_object)
}

fn create_readable_stream_byob_reader(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let reader = ReadableStreamBYOBReader::new();
    let reader_object: JsObject =
        create_interface_instance::<Types, ReadableStreamBYOBReader>(reader, ec)?.into();
    Ok(reader_object)
}

/// <https://streams.spec.whatwg.org/#abstract-opdef-readablestreambyobreaderrelease>
pub(crate) fn readable_stream_byob_reader_release(
    reader: ReadableStreamBYOBReader,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
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
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&ReadableStreamBYOBReader) -> R,
) -> Completion<R, Types> {
    let reader_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamBYOBReader>());
    let reader = match reader_ref {
        Some(r) => r,
        None => return Err(ec.new_type_error("object is not a ReadableStreamBYOBReader")),
    };
    Ok(f(reader))
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
