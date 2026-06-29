use boa_engine::{Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use std::marker::PhantomData;

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
use crate::webidl::{create_value_async_iterator, rejected_promise};

use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definitions (§3) ──

impl WebIdlInterface<crate::js::Types> for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn create_platform_object(
        new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        (|| -> JsResult<Self> { construct_readable_stream(new_target, args, ctx) })()
            .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
            JsValue::from(stream.locked())
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn cancel_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream =
            with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
        let promise = stream.cancel(args.get_or_undefined(0).clone(), ctx)?;

        Ok(JsValue::from(promise))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream =
            with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
        let reader = stream.get_reader(args.get_or_undefined(0), ctx)?;

        Ok(JsValue::from(reader))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn pipe_through_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream =
            with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
        stream.pipe_through(args.get_or_undefined(0), args.get_or_undefined(1), ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn pipe_to_operation(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsObject> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream =
            with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
        Ok(stream.pipe_to(args.get_or_undefined(0), args.get_or_undefined(1), ctx))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

pub(crate) fn pipe_to_native_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ec = js_engine::boa::context_as_ec(context);
    let promise = match pipe_to_operation(this, args, ec) {
        Ok(promise) => promise,
        Err(error) => rejected_promise(error, js_engine::boa::context_as_ec(context))
            .map_err(boa_engine::JsError::from_opaque)?,
    };
    Ok(JsValue::from(promise))
}

fn tee_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream =
            with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
        stream.tee(ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

pub(crate) fn values_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let iterator = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        crate::js::completion_to_js_result(create_value_async_iterator(
            stream.clone(),
            args,
            js_engine::boa::context_as_ec(context),
        ))
    })??;
    Ok(JsValue::from(iterator))
}

pub(crate) fn from_static(
    _: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    Ok(JsValue::from(
        readable_stream_from_iterable(args.get_or_undefined(0).clone(), ctx)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))?,
    ))
}

fn get_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController receiver is not an object")
        })?;

        match with_readable_stream_default_controller_ref(&controller_object, |controller| {
            controller.desired_size()
        })?? {
            Some(size) => Ok(JsValue::from(size)),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_byte_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController receiver is not an object")
        })?;

        match with_readable_byte_stream_controller_ref(&controller_object, |controller| {
            controller.desired_size()
        })?? {
            Some(size) => Ok(JsValue::from(size)),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_byob_request(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController receiver is not an object")
        })?;

        match with_readable_byte_stream_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(
                controller.byob_request(js_engine::boa::context_as_ec(ctx)),
            )
        })?? {
            Some(byob_request) => Ok(JsValue::from(byob_request)),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn close_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController receiver is not an object")
        })?;

        with_readable_stream_default_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.close(js_engine::boa::context_as_ec(ctx)))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn close_byte_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController receiver is not an object")
        })?;

        with_readable_byte_stream_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.close(js_engine::boa::context_as_ec(ctx)))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn enqueue_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController receiver is not an object")
        })?;

        with_readable_stream_default_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.enqueue(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(ctx),
            ))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn enqueue_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController receiver is not an object")
        })?;

        with_readable_byte_stream_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.enqueue(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(ctx),
            ))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn error_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultController receiver is not an object")
        })?;

        with_readable_stream_default_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.error(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(ctx),
            ))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn error_byte_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let controller_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableByteStreamController receiver is not an object")
        })?;

        with_readable_byte_stream_controller_ref(&controller_object, |controller| {
            crate::js::completion_to_js_result(controller.error(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(ctx),
            ))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let reader_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("ReadableStreamDefaultReader receiver is not an object")
        })?;

        let closed =
            with_readable_stream_default_reader_ref(&reader_object, |reader| reader.closed())??;
        Ok(JsValue::from(closed))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_byob_closed(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let reader_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamBYOBReader receiver is not an object")
        })?;

        let closed =
            with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.closed())??;
        Ok(JsValue::from(closed))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn cancel_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamDefaultReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_default_reader_ref(&reader_object, |reader| {
        reader
            .cancel(args.get_or_undefined(0).clone(), ec)
            .map(JsValue::from)
    })
    .map_err(|e| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        e.into_opaque(ctx).unwrap_or(JsValue::undefined())
    })?
}

fn read_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamDefaultReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_default_reader_ref(&reader_object, |reader| {
        reader.read(ec).map(JsValue::from)
    })
    .map_err(|e| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        e.into_opaque(ctx).unwrap_or(JsValue::undefined())
    })?
}

fn cancel_byob_reader_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamBYOBReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_byob_reader_ref(&reader_object, |reader| {
        reader
            .cancel(args.get_or_undefined(0).clone(), ec)
            .map(JsValue::from)
    })
    .map_err(|e| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        e.into_opaque(ctx).unwrap_or(JsValue::undefined())
    })?
}

fn read_byob_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamBYOBReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_byob_reader_ref(&reader_object, |reader| {
        reader
            .read(args.get_or_undefined(0), args.get_or_undefined(1), ec)
            .map(JsValue::from)
    })
    .map_err(|e| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        e.into_opaque(ctx).unwrap_or(JsValue::undefined())
    })?
}

fn release_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamDefaultReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_default_reader_ref(&reader_object, |reader| reader.release_lock(ec))
        .map_err(|e| {
            let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
            e.into_opaque(ctx).unwrap_or(JsValue::undefined())
        })??;
    Ok(JsValue::undefined())
}

fn release_byob_lock_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let reader_object = this.as_object().ok_or_else(|| {
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let error: JsError = JsNativeError::typ()
            .with_message("ReadableStreamBYOBReader receiver is not an object")
            .into();
        error
            .into_opaque(ctx)
            .unwrap_or_else(|_| JsValue::undefined())
    })?;
    with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.release_lock(ec))
        .map_err(|e| {
            let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
            e.into_opaque(ctx).unwrap_or(JsValue::undefined())
        })??;
    Ok(JsValue::undefined())
}

fn get_byob_view(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let request_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
        })?;

        match with_readable_stream_byob_request_ref(&request_object, |request| request.view())? {
            Some(view) => Ok(JsValue::from(view)),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn respond_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let request_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
        })?;
        let bytes_written = args.get_or_undefined(0).to_number(ctx)?;
        if !bytes_written.is_finite() || bytes_written < 0.0 || bytes_written.fract() != 0.0 {
            return Err(JsNativeError::typ()
                .with_message("bytesWritten must be a non-negative integer")
                .into());
        }
        with_readable_stream_byob_request_ref(&request_object, |request| {
            crate::js::completion_to_js_result(
                request.respond(bytes_written as usize, js_engine::boa::context_as_ec(ctx)),
            )
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn respond_with_new_view_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let request_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
        })?;
        with_readable_stream_byob_request_ref(&request_object, |request| {
            crate::js::completion_to_js_result(request.respond_with_new_view(
                args.get_or_undefined(0).clone(),
                js_engine::boa::context_as_ec(ctx),
            ))
        })??;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
