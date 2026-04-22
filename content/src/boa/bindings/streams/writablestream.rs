use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::streams::{
    WritableStream, WritableStreamDefaultController, WritableStreamDefaultWriter,
    construct_writable_stream, construct_writable_stream_default_writer,
    with_writable_stream_default_controller_ref,
    with_writable_stream_default_writer_ref, with_writable_stream_ref,
};

impl Class for WritableStream {
    const NAME: &'static str = "WritableStream";

    fn data_constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        construct_writable_stream(new_target, args, context)
    }

    fn object_constructor(
        instance: &boa_engine::object::JsObject<Self>,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<()> {
        instance.borrow().data().set_reflector(instance.clone().upcast());
        Ok(())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("locked"),
                Some(NativeFunction::from_fn_ptr(get_locked).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("abort"),
                1,
                NativeFunction::from_fn_ptr(abort_method),
            )
            .method(
                js_string!("close"),
                0,
                NativeFunction::from_fn_ptr(close_method),
            )
            .method(
                js_string!("getWriter"),
                0,
                NativeFunction::from_fn_ptr(get_writer_method),
            );
        Ok(())
    }
}

impl Class for WritableStreamDefaultController {
    const NAME: &'static str = "WritableStreamDefaultController";

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
                js_string!("signal"),
                Some(NativeFunction::from_fn_ptr(get_signal).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("error"),
                1,
                NativeFunction::from_fn_ptr(error_method),
            );
        Ok(())
    }
}

impl Class for WritableStreamDefaultWriter {
    const NAME: &'static str = "WritableStreamDefaultWriter";
    const LENGTH: usize = 1;

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_writable_stream_default_writer(this, args, context)
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
            .accessor(
                js_string!("desiredSize"),
                Some(NativeFunction::from_fn_ptr(get_desired_size).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("ready"),
                Some(NativeFunction::from_fn_ptr(get_ready).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .method(
                js_string!("abort"),
                1,
                NativeFunction::from_fn_ptr(abort_writer_method),
            )
            .method(
                js_string!("close"),
                0,
                NativeFunction::from_fn_ptr(close_writer_method),
            )
            .method(
                js_string!("releaseLock"),
                0,
                NativeFunction::from_fn_ptr(release_lock_method),
            )
            .method(
                js_string!("write"),
                1,
                NativeFunction::from_fn_ptr(write_method),
            );
        Ok(())
    }
}

fn get_locked(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    with_writable_stream_ref(&stream_object, |stream| JsValue::from(stream.locked()))
}

fn abort_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let promise = stream.abort(args.get_or_undefined(0).clone(), context)?;
    Ok(JsValue::from(promise))
}

fn close_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let promise = stream.close(context)?;
    Ok(JsValue::from(promise))
}

fn get_writer_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let stream_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStream receiver is not an object")
    })?;

    let stream = with_writable_stream_ref(&stream_object, |stream| stream.clone())?;
    let writer = stream.get_writer(context)?;
    Ok(JsValue::from(writer))
}

fn get_signal(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WritableStreamDefaultController receiver is not an object")
    })?;

    let signal = with_writable_stream_default_controller_ref(&controller_object, |controller| {
        controller.signal_value()
    })??;
    Ok(JsValue::from(signal))
}

fn error_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let controller_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WritableStreamDefaultController receiver is not an object")
    })?;

    let controller = with_writable_stream_default_controller_ref(&controller_object, |controller| {
        controller.clone()
    })?;
    controller.error(args.get_or_undefined(0).clone(), context)?;
    Ok(JsValue::undefined())
}

fn get_closed(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| writer.closed())??;
    Ok(JsValue::from(promise))
}

fn get_desired_size(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    match with_writable_stream_default_writer_ref(&writer_object, |writer| writer.desired_size())?? {
        Some(size) => Ok(JsValue::from(size)),
        None => Ok(JsValue::null()),
    }
}

fn get_ready(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| writer.ready())??;
    Ok(JsValue::from(promise))
}

fn abort_writer_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.abort(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::from(promise))
}

fn close_writer_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.close(context)
    })??;
    Ok(JsValue::from(promise))
}

fn release_lock_method(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.release_lock(context)
    })??;
    Ok(JsValue::undefined())
}

fn write_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let writer_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("WritableStreamDefaultWriter receiver is not an object")
    })?;

    let promise = with_writable_stream_default_writer_ref(&writer_object, |writer| {
        writer.write(args.get_or_undefined(0).clone(), context)
    })??;
    Ok(JsValue::from(promise))
}