use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::streams::readable::{
    construct_readable_stream, construct_readable_stream_default_reader,
    with_readable_stream_default_controller_mut, with_readable_stream_default_controller_ref,
    with_readable_stream_default_reader_ref, with_readable_stream_mut,
};
use crate::streams::{
    ReadableStream, ReadableStreamDefaultController, ReadableStreamDefaultReader,
};

impl Class for ReadableStream {
    const NAME: &'static str = "ReadableStream";

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_readable_stream(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
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

fn get_locked(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    with_readable_stream_mut(&stream_object, |stream| JsValue::from(stream.locked()))
}

fn cancel_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let promise = with_readable_stream_mut(&stream_object, |stream| {
        stream.cancel(args.get_or_undefined(0).clone(), context)
    })??;

    Ok(JsValue::from(promise))
}

fn get_reader_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let reader = with_readable_stream_mut(&stream_object, |stream| {
        stream.get_reader(args.get_or_undefined(0), context)
    })??;

    Ok(JsValue::from(reader))
}

fn pipe_through_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    with_readable_stream_mut(&stream_object, |stream| {
        stream.pipe_through(args.get_or_undefined(0), args.get_or_undefined(1), context)
    })?
}

fn pipe_to_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    let promise = with_readable_stream_mut(&stream_object, |stream| {
        stream.pipe_to(args.get_or_undefined(0), args.get_or_undefined(1), context)
    })??;

    Ok(JsValue::from(promise))
}

fn tee_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStream receiver is not an object")
    })?;

    with_readable_stream_mut(&stream_object, |stream| stream.tee(context))?
}

fn from_static(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Err(JsNativeError::typ()
        .with_message("ReadableStream.from() is not implemented yet")
        .into())
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

fn close_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_mut(&controller_object, |controller| {
        controller.close(context)
    })??;
    Ok(JsValue::undefined())
}

fn enqueue_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_mut(&controller_object, |controller| {
        controller.enqueue(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::undefined())
}

fn error_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("ReadableStreamDefaultController receiver is not an object")
    })?;

    with_readable_stream_default_controller_mut(&controller_object, |controller| {
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

fn release_lock_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let reader_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("ReadableStreamDefaultReader receiver is not an object")
    })?;

    with_readable_stream_default_reader_ref(&reader_object, |reader| reader.release_lock(context))??;
    Ok(JsValue::undefined())
}
