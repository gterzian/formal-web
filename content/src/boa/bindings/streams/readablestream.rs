use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue, Source,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, JsObject},
    property::Attribute,
    symbol::JsSymbol,
};

use crate::streams::{
    ReadableByteStreamController, ReadableStream, ReadableStreamBYOBReader,
    ReadableStreamBYOBRequest, ReadableStreamDefaultController, ReadableStreamDefaultReader,
    construct_readable_stream, construct_readable_stream_byob_reader,
    construct_readable_stream_default_reader, readable_stream_from_iterable,
    with_readable_byte_stream_controller_ref, with_readable_stream_byob_reader_ref,
    with_readable_stream_byob_request_ref, with_readable_stream_default_reader_ref,
    with_readable_stream_ref,
};
use crate::webidl::{create_value_async_iterator, rejected_promise};
use crate::webidl::binding::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, register_interface,
};

// ── WebIDL interface definitions (§3) ──

impl WebIdlInterface for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "pipeThrough",
            length: 2,
            method: pipe_through_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "getReader",
            length: 1,
            method: get_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "tee",
            length: 0,
            method: tee_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface for ReadableStreamDefaultController {
    const NAME: &'static str = "ReadableStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "enqueue",
            length: 1,
            method: enqueue_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: error_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface for ReadableByteStreamController {
    const NAME: &'static str = "ReadableByteStreamController";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "close",
            length: 0,
            method: close_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "enqueue",
            length: 1,
            method: enqueue_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: error_byte_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface for ReadableStreamDefaultReader {
    const NAME: &'static str = "ReadableStreamDefaultReader";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "read",
            length: 0,
            method: read_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "releaseLock",
            length: 0,
            method: release_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface for ReadableStreamBYOBReader {
    const NAME: &'static str = "ReadableStreamBYOBReader";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "cancel",
            length: 1,
            method: cancel_byob_reader_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "read",
            length: 2,
            method: read_byob_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "releaseLock",
            length: 0,
            method: release_byob_lock_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

impl WebIdlInterface for ReadableStreamBYOBRequest {
    const NAME: &'static str = "ReadableStreamBYOBRequest";

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
        def.add_operation(OperationDef {
            id: "respond",
            length: 1,
            method: respond_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "respondWithNewView",
            length: 1,
            method: respond_with_new_view_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Boa Class glue ──

impl Class for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn data_constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        construct_readable_stream(new_target, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        // Standard interface members via spec-aligned registration
        register_interface::<ReadableStream>(class)?;

        // ── §3.7.7: Static operations (not yet handled by register_interface) ──
        class.static_method(
            js_string!("from"),
            1,
            NativeFunction::from_fn_ptr(from_static),
        );

        // ── Async iterator: values() and @@asyncIterator ──
        // https://streams.spec.whatwg.org/#rs-asynciterator
        let values = FunctionObjectBuilder::new(
            class.context().realm(),
            NativeFunction::from_fn_ptr(values_method),
        )
        .name(js_string!("values"))
        .length(0)
        .constructor(false)
        .build();
        class.property(
            js_string!("values"),
            values.clone(),
            Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        );
        class.property(
            JsSymbol::async_iterator(),
            values,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );

        // ── pipeTo with JS wrapper workaround ──
        let pipe_to_native = FunctionObjectBuilder::new(
            class.context().realm(),
            NativeFunction::from_fn_ptr(pipe_to_native_method),
        )
        .name(js_string!("pipeTo"))
        .length(2)
        .constructor(false)
        .build();
        let pipe_to_wrapper = {
            class.context().eval(Source::from_bytes(
                "(function pipeTo() { return ReadableStream.prototype.__formalWebReadableStreamPipeToNative.call(this, arguments[0], arguments[1]); })",
            ))?
                .as_object()
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("ReadableStream.pipeTo wrapper initialization did not return a function")
                })?
        };
        class.property(
            js_string!("__formalWebReadableStreamPipeToNative"),
            pipe_to_native,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );
        class.property(
            js_string!("pipeTo"),
            pipe_to_wrapper,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );

        Ok(())
    }
}

impl Class for ReadableStreamDefaultController {
    const NAME: &'static str = "ReadableStreamDefaultController";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<ReadableStreamDefaultController>(class)
    }
}

impl Class for ReadableByteStreamController {
    const NAME: &'static str = "ReadableByteStreamController";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<ReadableByteStreamController>(class)
    }
}

impl Class for ReadableStreamDefaultReader {
    const NAME: &'static str = "ReadableStreamDefaultReader";
    const LENGTH: usize = 1;

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_readable_stream_default_reader(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<ReadableStreamDefaultReader>(class)
    }
}

impl Class for ReadableStreamBYOBReader {
    const NAME: &'static str = "ReadableStreamBYOBReader";
    const LENGTH: usize = 1;

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_readable_stream_byob_reader(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<ReadableStreamBYOBReader>(class)
    }
}

impl Class for ReadableStreamBYOBRequest {
    const NAME: &'static str = "ReadableStreamBYOBRequest";
    const LENGTH: usize = 2;

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_interface::<ReadableStreamBYOBRequest>(class)
    }
}

// ── Member getters/setters/methods ──

fn get_locked(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        JsValue::from(stream.locked())
    })
}

fn cancel_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream =
        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
    let promise = stream.cancel(args.get_or_undefined(0).clone(), context)?;

    Ok(JsValue::from(promise))
}

fn get_reader_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream =
        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
    let reader = stream.get_reader(args.get_or_undefined(0), context)?;

    Ok(JsValue::from(reader))
}

fn pipe_through_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream =
        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
    stream.pipe_through(args.get_or_undefined(0), args.get_or_undefined(1), context)
}

fn pipe_to_operation(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsObject> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream =
        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
    Ok(stream.pipe_to(args.get_or_undefined(0), args.get_or_undefined(1), context))
}

fn pipe_to_native_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let promise = match pipe_to_operation(this, args, context) {
        Ok(promise) => promise,
        Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
    };
    Ok(JsValue::from(promise))
}

fn tee_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream =
        with_readable_stream_ref(&stream_object, |stream: &ReadableStream| stream.clone())?;
    stream.tee(context)
}

fn values_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let iterator = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        create_value_async_iterator(stream.clone(), args, context)
    })??;
    Ok(JsValue::from(iterator))
}

fn from_static(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(readable_stream_from_iterable(
        args.get_or_undefined(0).clone(),
        context,
    )?))
}

fn get_desired_size(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
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
}

fn get_byte_desired_size(
    this: &JsValue,
    _: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableByteStreamController receiver is not an object")
    })?;

    match with_readable_byte_stream_controller_ref(&controller_object, |controller| {
        controller.desired_size()
    })?? {
        Some(size) => Ok(JsValue::from(size)),
        None => Ok(JsValue::null()),
    }
}

fn get_byob_request(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableByteStreamController receiver is not an object")
    })?;

    match with_readable_byte_stream_controller_ref(&controller_object, |controller| {
        controller.byob_request(context)
    })?? {
        Some(byob_request) => Ok(JsValue::from(byob_request)),
        None => Ok(JsValue::null()),
    }
}

fn close_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_ref(&controller_object, |controller| {
        controller.close(context)
    })??;
    Ok(JsValue::undefined())
}

fn close_byte_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableByteStreamController receiver is not an object")
    })?;

    with_readable_byte_stream_controller_ref(&controller_object, |controller| {
        controller.close(context)
    })??;
    Ok(JsValue::undefined())
}

fn enqueue_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_ref(&controller_object, |controller| {
        controller.enqueue(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
}

fn enqueue_byte_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableByteStreamController receiver is not an object")
    })?;

    with_readable_byte_stream_controller_ref(&controller_object, |controller| {
        controller.enqueue(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
}

fn error_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_ref(&controller_object, |controller| {
        controller.error(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
}

fn error_byte_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableByteStreamController receiver is not an object")
    })?;

    with_readable_byte_stream_controller_ref(&controller_object, |controller| {
        controller.error(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
}

fn get_closed(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;

    let closed =
        with_readable_stream_default_reader_ref(&reader_object, |reader| reader.closed())??;
    Ok(JsValue::from(closed))
}

fn get_byob_closed(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBReader receiver is not an object")
    })?;

    let closed = with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.closed())??;
    Ok(JsValue::from(closed))
}

fn cancel_reader_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;

    let promise = with_readable_stream_default_reader_ref(&reader_object, |reader| {
        reader.cancel(args.get_or_undefined(0).clone(), context)
    })??;

    Ok(JsValue::from(promise))
}

fn read_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;

    let promise =
        with_readable_stream_default_reader_ref(&reader_object, |reader| reader.read(context))??;
    Ok(JsValue::from(promise))
}

fn cancel_byob_reader_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBReader receiver is not an object")
    })?;

    let promise = with_readable_stream_byob_reader_ref(&reader_object, |reader| {
        reader.cancel(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::from(promise))
}

fn read_byob_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBReader receiver is not an object")
    })?;

    let promise = with_readable_stream_byob_reader_ref(&reader_object, |reader| {
        reader.read(args.get_or_undefined(0), args.get_or_undefined(1), context)
    })??;
    Ok(JsValue::from(promise))
}

fn release_lock_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;

    with_readable_stream_default_reader_ref(&reader_object, |reader| {
        reader.release_lock(context)
    })??;
    Ok(JsValue::undefined())
}

fn release_byob_lock_method(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBReader receiver is not an object")
    })?;

    with_readable_stream_byob_reader_ref(&reader_object, |reader| reader.release_lock(context))??;
    Ok(JsValue::undefined())
}

fn get_byob_view(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let request_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
    })?;

    match with_readable_stream_byob_request_ref(&request_object, |request| request.view())? {
        Some(view) => Ok(JsValue::from(view)),
        None => Ok(JsValue::null()),
    }
}

fn respond_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let request_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
    })?;
    let bytes_written = args.get_or_undefined(0).to_number(context)?;
    if !bytes_written.is_finite() || bytes_written < 0.0 || bytes_written.fract() != 0.0 {
        return Err(JsNativeError::typ()
            .with_message("bytesWritten must be a non-negative integer")
            .into());
    }
    with_readable_stream_byob_request_ref(&request_object, |request| {
        request.respond(bytes_written as usize, context)
    })??;
    Ok(JsValue::undefined())
}

fn respond_with_new_view_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let request_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamBYOBRequest receiver is not an object")
    })?;
    with_readable_stream_byob_request_ref(&request_object, |request| {
        request.respond_with_new_view(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
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
