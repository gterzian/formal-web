use boa_engine::{Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use std::marker::PhantomData;

use crate::streams::{
    ReadableByteStreamController, ReadableStream, ReadableStreamBYOBReader,
    ReadableStreamBYOBRequest, ReadableStreamDefaultController, ReadableStreamDefaultReader,
    construct_readable_stream_byob_reader, construct_readable_stream_default_reader,
    construct_readable_stream, readable_stream_from_iterable,
    with_readable_byte_stream_controller_ref, with_readable_byte_stream_controller_ref_ec,
    with_readable_stream_byob_reader_ref, with_readable_stream_byob_reader_ref_ec,
    with_readable_stream_byob_request_ref, with_readable_stream_byob_request_ref_ec,
    with_readable_stream_default_reader_ref, with_readable_stream_default_reader_ref_ec,
    with_readable_stream_ref_ec,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{create_value_async_iterator, rejected_promise};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definitions (§3) ──

impl WebIdlInterface<crate::js::Types> for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn create_platform_object(
        new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_readable_stream(new_target, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "pipeThrough",
            length: 2,
            method: pipe_through_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "cancel",
            length: 1,
            method: cancel_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "getReader",
            length: 1,
            method: get_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "tee",
            length: 0,
            method: tee_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        // https://streams.spec.whatwg.org/#readablestream-static-methods
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "from",
            length: 1,
            method: from_static,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for ReadableStreamDefaultController {
    const NAME: &'static str = "ReadableStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "close",
            length: 0,
            method: close_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "enqueue",
            length: 1,
            method: enqueue_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "error",
            length: 1,
            method: error_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for ReadableByteStreamController {
    const NAME: &'static str = "ReadableByteStreamController";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "close",
            length: 0,
            method: close_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "enqueue",
            length: 1,
            method: enqueue_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "error",
            length: 1,
            method: error_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for ReadableStreamDefaultReader {
    const NAME: &'static str = "ReadableStreamDefaultReader";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_readable_stream_default_reader(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "cancel",
            length: 1,
            method: cancel_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "read",
            length: 0,
            method: read_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "releaseLock",
            length: 0,
            method: release_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for ReadableStreamBYOBReader {
    const NAME: &'static str = "ReadableStreamBYOBReader";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_readable_stream_byob_reader(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "cancel",
            length: 1,
            method: cancel_byob_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "read",
            length: 2,
            method: read_byob_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "releaseLock",
            length: 0,
            method: release_byob_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for ReadableStreamBYOBRequest {
    const NAME: &'static str = "ReadableStreamBYOBRequest";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "respond",
            length: 1,
            method: respond_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "respondWithNewView",
            length: 1,
            method: respond_with_new_view_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Member getters/setters/methods ──

fn get_locked(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let stream = stream_object
        .downcast_ref::<ReadableStream>()
        .ok_or_else(|| ec.new_type_error("object is not a ReadableStream"))?;
    Ok(JsValue::from(stream.locked()))
}

fn cancel_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream =
        with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let promise = stream.cancel_ec(args.get_or_undefined(0).clone(), ec)?;
    Ok(JsValue::from(promise))
}

fn get_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream =
        with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let reader = stream.get_reader_ec(args.get_or_undefined(0), ec)?;
    Ok(JsValue::from(reader))
}

fn pipe_through_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream =
        with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.pipe_through_ec(args.get_or_undefined(0), args.get_or_undefined(1), ec)
}

fn pipe_to_operation(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream =
        with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.pipe_to_ec(args.get_or_undefined(0), args.get_or_undefined(1), ec)
}

pub(crate) fn pipe_to_native_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let promise = pipe_to_operation(this, args, ec)?;
    Ok(JsValue::from(promise))
}

// Adapter for host_hooks.rs which still uses NativeFunction::from_fn_ptr with Context.
pub(crate) fn pipe_to_native_method_adapter(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ec = js_engine::boa::context_as_ec(context);
    pipe_to_native_method(this, args, ec).map_err(JsError::from_opaque)
}

fn tee_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let mut stream =
        with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    stream.tee(ec)
}

pub(crate) fn values_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let stream_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStream receiver is not an object"))?;
    let stream = with_readable_stream_ref_ec(&stream_object, ec, |s: &ReadableStream| s.clone())?;
    let iterator = create_value_async_iterator(stream, args, ec)?;
    Ok(JsValue::from(iterator))
}

// Adapter for host_hooks.rs which still uses NativeFunction::from_fn_ptr with Context.
pub(crate) fn values_method_adapter(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ec = js_engine::boa::context_as_ec(context);
    values_method(this, args, ec).map_err(JsError::from_opaque)
}

pub(crate) fn from_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let async_iterable = args.get(0).cloned().unwrap_or_else(|| ec.value_undefined());
    Ok(JsValue::from(readable_stream_from_iterable(
        async_iterable,
        ec,
    )?))
}

fn get_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
        })?;
    let controller =
        with_readable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    let size = controller.desired_size_ec(ec)?;
    Ok(match size {
        Some(s) => JsValue::from(s),
        None => JsValue::null(),
    })
}

fn get_byte_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableByteStreamController receiver is not an object")
        })?;
    let controller =
        with_readable_byte_stream_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    let size = controller.desired_size(ec)?;
    Ok(match size {
        Some(s) => JsValue::from(s),
        None => JsValue::null(),
    })
}

fn get_byob_request(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableByteStreamController receiver is not an object")
        })?;
    let controller =
        with_readable_byte_stream_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    let byob_request = controller.byob_request(ec)?;
    Ok(match byob_request {
        Some(req) => JsValue::from(req),
        None => JsValue::null(),
    })
}

fn close_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
        })?;
    let controller =
        with_readable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.close(ec)?;
    Ok(ec.value_undefined())
}

fn close_byte_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableByteStreamController receiver is not an object")
        })?;
    let controller =
        with_readable_byte_stream_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.close(ec)?;
    Ok(ec.value_undefined())
}

fn enqueue_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
        })?;
    let controller =
        with_readable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.enqueue(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn enqueue_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableByteStreamController receiver is not an object")
        })?;
    let controller =
        with_readable_byte_stream_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.enqueue(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn error_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableStreamDefaultController receiver is not an object")
        })?;
    let controller =
        with_readable_stream_default_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.error(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn error_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let controller_object =
        <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
            ec.new_type_error("ReadableByteStreamController receiver is not an object")
        })?;
    let controller =
        with_readable_byte_stream_controller_ref_ec(&controller_object, ec, |c| c.clone())?;
    controller.error(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn get_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let closed = reader.closed_ec(ec)?;
    Ok(JsValue::from(closed))
}

fn get_byob_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let closed = reader.closed_ec(ec)?;
    Ok(JsValue::from(closed))
}

fn cancel_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let promise = reader.cancel(args.get_or_undefined(0).clone(), ec)?;
    Ok(JsValue::from(promise))
}

fn read_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let promise = reader.read(ec)?;
    Ok(JsValue::from(promise))
}

fn cancel_byob_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let promise = reader.cancel(args.get_or_undefined(0).clone(), ec)?;
    Ok(JsValue::from(promise))
}

fn read_byob_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    let promise = reader.read(args.get_or_undefined(0), args.get_or_undefined(1), ec)?;
    Ok(JsValue::from(promise))
}

fn release_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("ReadableStreamDefaultReader receiver is not an object")
    })?;
    let reader = with_readable_stream_default_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    reader.release_lock(ec)?;
    Ok(ec.value_undefined())
}

fn release_byob_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBReader receiver is not an object"))?;
    let reader = with_readable_stream_byob_reader_ref_ec(&reader_object, ec, |r| r.clone())?;
    reader.release_lock(ec)?;
    Ok(ec.value_undefined())
}

fn get_byob_view(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let request_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let view =
        with_readable_stream_byob_request_ref_ec(&request_object, ec, |request| request.view())?;
    Ok(match view {
        Some(v) => JsValue::from(v),
        None => ec.value_null(),
    })
}

fn respond_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let request_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let bytes_written = ec.to_uint32(args.get_or_undefined(0).clone())?;
    let request = with_readable_stream_byob_request_ref_ec(&request_object, ec, |r| r.clone())?;
    request.respond(bytes_written as usize, ec)?;
    Ok(ec.value_undefined())
}

fn respond_with_new_view_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let request_object = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("ReadableStreamBYOBRequest receiver is not an object"))?;
    let request = with_readable_stream_byob_request_ref_ec(&request_object, ec, |r| r.clone())?;
    request.respond_with_new_view(args.get_or_undefined(0).clone(), ec)?;
    Ok(ec.value_undefined())
}

fn with_readable_stream_default_controller_ref<R>(
    object: &boa_engine::object::JsObject,
    f: impl FnOnce(&ReadableStreamDefaultController) -> R,
) -> JsResult<R> {
    let controller = object
        .downcast_ref::<ReadableStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("object is not a ReadableStreamDefaultController")
        })?;
    Ok(f(&controller))
}

fn with_readable_stream_default_controller_ref_ec<R>(
    object: &boa_engine::object::JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&ReadableStreamDefaultController) -> R,
) -> Completion<R, crate::js::Types> {
    let ctrl_ref = ec
        .with_object_any(object)
        .and_then(|a| a.downcast_ref::<ReadableStreamDefaultController>());
    let controller = match ctrl_ref {
        Some(c) => c,
        None => return Err(ec.new_type_error("object is not a ReadableStreamDefaultController")),
    };
    Ok(f(controller))
}
