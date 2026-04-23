use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::Attribute,
    symbol::JsSymbol,
};

use crate::streams::{
    ReadableByteStreamController, ReadableStream, ReadableStreamBYOBReader,
    ReadableStreamBYOBRequest, ReadableStreamDefaultController, ReadableStreamDefaultReader,
    construct_readable_stream, construct_readable_stream_byob_reader,
    construct_readable_stream_default_reader,
    readable_stream_from_iterable,
    with_readable_byte_stream_controller_ref, with_readable_stream_byob_reader_ref,
    with_readable_stream_byob_request_ref, with_readable_stream_default_reader_ref,
    with_readable_stream_ref,
};
use crate::webidl::{create_value_async_iterator, rejected_promise};

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
        let realm = class.context().realm().clone();
        let values = FunctionObjectBuilder::new(
            class.context().realm(),
            NativeFunction::from_fn_ptr(values_method),
        )
        .name(js_string!("values"))
        .length(0)
        .constructor(false)
        .build();
        class
            .static_method(
                js_string!("from"),
                1,
                NativeFunction::from_fn_ptr(from_static),
            )
            .accessor(
                js_string!("locked"),
                Some(NativeFunction::from_fn_ptr(get_locked).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("cancel"),
                1,
                NativeFunction::from_fn_ptr(cancel_method),
            )
            .method(
                js_string!("getReader"),
                1,
                NativeFunction::from_fn_ptr(get_reader_method),
            )
            .method(
                js_string!("pipeThrough"),
                2,
                NativeFunction::from_fn_ptr(pipe_through_method),
            )
            .method(
                js_string!("pipeTo"),
                2,
                NativeFunction::from_fn_ptr(pipe_to_method),
            )
            .method(
                js_string!("tee"),
                0,
                NativeFunction::from_fn_ptr(tee_method),
            )
            .property(
                js_string!("values"),
                values.clone(),
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                JsSymbol::async_iterator(),
                values,
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
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("desiredSize"),
                Some(NativeFunction::from_fn_ptr(get_desired_size).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("close"),
                0,
                NativeFunction::from_fn_ptr(close_method),
            )
            .method(
                js_string!("enqueue"),
                1,
                NativeFunction::from_fn_ptr(enqueue_method),
            )
            .method(
                js_string!("error"),
                1,
                NativeFunction::from_fn_ptr(error_method),
            );
        Ok(())
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
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("byobRequest"),
                Some(NativeFunction::from_fn_ptr(get_byob_request).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("desiredSize"),
                Some(NativeFunction::from_fn_ptr(get_byte_desired_size).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("close"),
                0,
                NativeFunction::from_fn_ptr(close_byte_method),
            )
            .method(
                js_string!("enqueue"),
                1,
                NativeFunction::from_fn_ptr(enqueue_byte_method),
            )
            .method(
                js_string!("error"),
                1,
                NativeFunction::from_fn_ptr(error_byte_method),
            );
        Ok(())
    }
}

impl Class for ReadableStreamDefaultReader {
    const NAME: &'static str = "ReadableStreamDefaultReader";
    const LENGTH: usize = 1;

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_readable_stream_default_reader(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("closed"),
                Some(NativeFunction::from_fn_ptr(get_closed).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("cancel"),
                1,
                NativeFunction::from_fn_ptr(cancel_reader_method),
            )
            .method(
                js_string!("read"),
                0,
                NativeFunction::from_fn_ptr(read_method),
            )
            .method(
                js_string!("releaseLock"),
                0,
                NativeFunction::from_fn_ptr(release_lock_method),
            );
        Ok(())
    }
}

impl Class for ReadableStreamBYOBReader {
    const NAME: &'static str = "ReadableStreamBYOBReader";
    const LENGTH: usize = 1;

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_readable_stream_byob_reader(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("closed"),
                Some(NativeFunction::from_fn_ptr(get_byob_closed).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("cancel"),
                1,
                NativeFunction::from_fn_ptr(cancel_byob_reader_method),
            )
            .method(
                js_string!("read"),
                2,
                NativeFunction::from_fn_ptr(read_byob_method),
            )
            .method(
                js_string!("releaseLock"),
                0,
                NativeFunction::from_fn_ptr(release_byob_lock_method),
            );
        Ok(())
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
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("view"),
                Some(NativeFunction::from_fn_ptr(get_byob_view).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("respond"),
                1,
                NativeFunction::from_fn_ptr(respond_method),
            )
            .method(
                js_string!("respondWithNewView"),
                1,
                NativeFunction::from_fn_ptr(respond_with_new_view_method),
            );
        Ok(())
    }
}

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

    let mut stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        stream.clone()
    })?;
    let promise = stream.cancel(args.get_or_undefined(0).clone(), context)?;

    Ok(JsValue::from(promise))
}

fn get_reader_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        stream.clone()
    })?;
    let reader = stream.get_reader(args.get_or_undefined(0), context)?;

    Ok(JsValue::from(reader))
}

fn pipe_through_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        stream.clone()
    })?;
    stream.pipe_through(args.get_or_undefined(0), args.get_or_undefined(1), context)
}

fn pipe_to_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let promise = (|| {
        let stream_object = this.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("ReadableStream receiver is not an object")
        })?;

        let mut stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
            stream.clone()
        })?;
        stream.pipe_to(args.get_or_undefined(0), args.get_or_undefined(1), context)
    })();

    let promise = match promise {
        Ok(promise) => promise,
        Err(error) => rejected_promise(error.into_opaque(context)?, context)?,
    };

    // TODO(formal-web): WPT `streams/piping/throwing-options.any.js` and some
    // rejected-cancel `pipeTo()` cases still fail with `TypeError: not a callable
    // function` inside harness promise helpers, even though direct local probes of
    // `pipeTo(...).then(...)` work. That points to a Boa promise method/binding
    // issue rather than another stream-layer semantic bug.
    Ok(JsValue::from(promise))
}

fn tee_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let mut stream = with_readable_stream_ref(&stream_object, |stream: &ReadableStream| {
        stream.clone()
    })?;
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

fn get_byte_desired_size(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
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
}

fn get_byob_request(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableByteStreamController receiver is not an object")
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
        JsNativeError::typ()
            .with_message("ReadableByteStreamController receiver is not an object")
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

fn enqueue_byte_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableByteStreamController receiver is not an object")
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
        JsNativeError::typ()
            .with_message("ReadableByteStreamController receiver is not an object")
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

    let closed = with_readable_stream_default_reader_ref(&reader_object, |reader| reader.closed())??;
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

    let promise = with_readable_stream_default_reader_ref(&reader_object, |reader| reader.read(context))??;
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

    with_readable_stream_default_reader_ref(&reader_object, |reader| reader.release_lock(context))??;
    Ok(JsValue::undefined())
}

fn release_byob_lock_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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
    let controller = object.downcast_ref::<ReadableStreamDefaultController>().ok_or_else(|| {
        JsNativeError::typ().with_message("object is not a ReadableStreamDefaultController")
    })?;
    Ok(f(&controller))
}
