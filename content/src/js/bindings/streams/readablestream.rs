use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

use crate::streams::{
    ReadableByteStreamController, ReadableStream, ReadableStreamBYOBReader,
    ReadableStreamBYOBRequest, ReadableStreamDefaultController, ReadableStreamDefaultReader,
    construct_readable_stream, construct_readable_stream_byob_reader,
    construct_readable_stream_default_reader, readable_stream_from_iterable,
    with_readable_byte_stream_controller_ref, with_readable_stream_byob_reader_ref,
    with_readable_stream_byob_request_ref, with_readable_stream_default_reader_ref,
    with_readable_stream_ref,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::create_value_async_iterator;


impl WebIdlInterface<Types> for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn create_platform_object(
        new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        construct_readable_stream(new_target, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "locked",
            getter: get_locked,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "pipeThrough",
            length: 2,
            method: pipe_through_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "getReader",
            length: 1,
            method: get_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "tee",
            length: 0,
            method: tee_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        // https://streams.spec.whatwg.org/#readablestream-static-methods
        def.add_operation(OperationDef {
            id: "from",
            length: 1,
            method: from_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for ReadableStreamDefaultController {
    const NAME: &'static str = "ReadableStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "desiredSize",
            getter: get_desired_size,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "enqueue",
            length: 1,
            method: enqueue_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: error_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for ReadableByteStreamController {
    const NAME: &'static str = "ReadableByteStreamController";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "byobRequest",
            getter: get_byob_request,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "desiredSize",
            getter: get_byte_desired_size,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "enqueue",
            length: 1,
            method: enqueue_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: error_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for ReadableStreamDefaultReader {
    const NAME: &'static str = "ReadableStreamDefaultReader";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        construct_readable_stream_default_reader(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "closed",
            getter: get_closed,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "read",
            length: 0,
            method: read_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "releaseLock",
            length: 0,
            method: release_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for ReadableStreamBYOBReader {
    const NAME: &'static str = "ReadableStreamBYOBReader";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        construct_readable_stream_byob_reader(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "closed",
            getter: get_byob_closed,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_byob_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "read",
            length: 2,
            method: read_byob_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "releaseLock",
            length: 0,
            method: release_byob_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

impl WebIdlInterface<Types> for ReadableStreamBYOBRequest {
    const NAME: &'static str = "ReadableStreamBYOBRequest";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "view",
            getter: get_byob_view,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "respond",
            length: 1,
            method: respond_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "respondWithNewView",
            length: 1,
            method: respond_with_new_view_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}


fn get_locked(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let locked = with_readable_stream_ref(&stream_object, ec, |stream: &ReadableStream| {
        stream.locked()
    })?;
    Ok(JsValue::from(locked))
}

fn cancel_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let promise = stream.cancel(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(JsValue::from(promise))
}

fn get_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let reader = stream.get_reader(
        &args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(JsValue::from(reader))
}

fn pipe_through_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.pipe_through(
        &args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        &args.get(1).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )
}

fn pipe_to_operation(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.pipe_to(
        &args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        &args.get(1).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )
}

pub(crate) fn pipe_to_native_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // <https://streams.spec.whatwg.org/#rs-pipeTo>
    // Step 1: "Let promise be a new promise."
    // Brand-check errors (this/destination not a stream) and option-getter
    // exceptions must reject the promise, not throw synchronously.
    match pipe_to_operation(this, args, ec) {
        Ok(promise) => Ok(JsValue::from(promise)),
        Err(error) => {
            let (promise, resolvers) = ec.new_promise_pending()?;
            let undefined = ec.value_undefined();
            // Call resolvers.reject with the error to reject the promise.
            if let Err(reject_error) = ec.call(&resolvers.reject, &undefined, &[error]) {
                // Spec: If rejecting fails, ignore (edge case).
                let _ = reject_error;
            }
            Ok(JsValue::from(promise))
        }
    }
}

fn tee_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.tee(ec)
}

pub(crate) fn values_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let stream = with_readable_stream_ref(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let iterator = create_value_async_iterator(stream, args, ec)?;
    Ok(JsValue::from(iterator))
}

pub(crate) fn from_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let async_iterable = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    Ok(JsValue::from(readable_stream_from_iterable(
        async_iterable,
        ec,
    )?))
}

fn get_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
    })?;
    let controller =
        with_readable_stream_default_controller_ref(&controller_object, ec, |c| c.clone())?;
    let size = controller.desired_size(ec)?;
    Ok(match size {
        Some(s) => JsValue::from(s),
        None => ec.value_null(),
    })
}

fn get_byte_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableByteStreamController receiver is not an object")
    })?;
    let controller =
        with_readable_byte_stream_controller_ref(&controller_object, ec, |c| c.clone())?;
    let size = controller.desired_size(ec)?;
    Ok(match size {
        Some(s) => JsValue::from(s),
        None => ec.value_null(),
    })
}

fn get_byob_request(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableByteStreamController receiver is not an object")
    })?;
    let controller =
        with_readable_byte_stream_controller_ref(&controller_object, ec, |c| c.clone())?;
    let byob_request = controller.byob_request(ec)?;
    Ok(match byob_request {
        Some(req) => JsValue::from(req),
        None => ec.value_null(),
    })
}

fn close_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
    })?;
    let controller =
        with_readable_stream_default_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.close(ec)?;
    Ok(ec.value_undefined())
}

fn close_byte_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableByteStreamController receiver is not an object")
    })?;
    let controller =
        with_readable_byte_stream_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.close(ec)?;
    Ok(ec.value_undefined())
}

fn enqueue_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
    })?;
    let controller =
        with_readable_stream_default_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.enqueue(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn enqueue_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableByteStreamController receiver is not an object")
    })?;
    let controller =
        with_readable_byte_stream_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.enqueue(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn error_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
    })?;
    let controller =
        with_readable_stream_default_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.error(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn error_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let controller_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableByteStreamController receiver is not an object")
    })?;
    let controller =
        with_readable_byte_stream_controller_ref(&controller_object, ec, |c| c.clone())?;
    controller.error(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn get_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, ec, |r| r.clone())?;
    let closed = reader.closed(ec)?;
    Ok(JsValue::from(closed))
}

fn get_byob_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, ec, |r| r.clone())?;
    let closed = reader.closed(ec)?;
    Ok(JsValue::from(closed))
}

fn cancel_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, ec, |r| r.clone())?;
    let promise = reader.cancel(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(JsValue::from(promise))
}

fn read_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, ec, |r| r.clone())?;
    let promise = reader.read(ec)?;
    Ok(JsValue::from(promise))
}

fn cancel_byob_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, ec, |r| r.clone())?;
    let promise = reader.cancel(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(JsValue::from(promise))
}

fn read_byob_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, ec, |r| r.clone())?;
    let promise = reader.read(
        &args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        &args.get(1).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(JsValue::from(promise))
}

fn release_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref(&reader_object, ec, |r| r.clone())?;
    reader.release_lock(ec)?;
    Ok(ec.value_undefined())
}

fn release_byob_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reader_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref(&reader_object, ec, |r| r.clone())?;
    reader.release_lock(ec)?;
    Ok(ec.value_undefined())
}

fn get_byob_view(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let request_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let view =
        with_readable_stream_byob_request_ref(&request_object, ec, |request| request.view())?;
    Ok(match view {
        Some(v) => JsValue::from(v),
        None => ec.value_null(),
    })
}

fn respond_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let request_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let arg = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    let bytes_written = ec.to_uint32(arg)?;
    let request = with_readable_stream_byob_request_ref(&request_object, ec, |r| r.clone())?;
    request.respond(bytes_written as usize, ec)?;
    Ok(ec.value_undefined())
}

fn respond_with_new_view_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let request_object = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let request = with_readable_stream_byob_request_ref(&request_object, ec, |r| r.clone())?;
    request.respond_with_new_view(
        args.get(0).cloned().unwrap_or_else(|| ec.value_undefined()),
        ec,
    )?;
    Ok(ec.value_undefined())
}

fn with_readable_stream_default_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&ReadableStreamDefaultController) -> R,
) -> Completion<R, Types> {
    let ctrl_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamDefaultController>());
    let controller = match ctrl_ref {
        Some(c) => c,
        None => return Err(ec.new_type_error("object is not a ReadableStreamDefaultController")),
    };
    Ok(f(controller))
}
