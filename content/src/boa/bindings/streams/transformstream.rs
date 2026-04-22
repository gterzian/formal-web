use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::streams::{
    TransformStream, TransformStreamDefaultController,
    construct_transform_stream,
    with_transform_stream_ref,
    with_transform_stream_default_controller_ref,
};

impl Class for TransformStream {
    const NAME: &'static str = "TransformStream";

    fn data_constructor(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<Self> {
        construct_transform_stream(this, args, context)
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("readable"),
                Some(NativeFunction::from_fn_ptr(get_readable).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("writable"),
                Some(NativeFunction::from_fn_ptr(get_writable).to_js_function(&realm)),
                None,
                Attribute::all(),
            );
        Ok(())
    }
}

impl Class for TransformStreamDefaultController {
    const NAME: &'static str = "TransformStreamDefaultController";

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
                js_string!("enqueue"),
                1,
                NativeFunction::from_fn_ptr(controller_enqueue),
            )
            .method(
                js_string!("error"),
                1,
                NativeFunction::from_fn_ptr(controller_error),
            )
            .method(
                js_string!("terminate"),
                0,
                NativeFunction::from_fn_ptr(controller_terminate),
            );
        Ok(())
    }
}

/// <https://streams.spec.whatwg.org/#ts-readable>
fn get_readable(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("TransformStream.readable called on non-object")
    })?;
    with_transform_stream_ref(&object, |stream| {
        Ok(JsValue::from(stream.readable()?.object()?))
    })?
}

/// <https://streams.spec.whatwg.org/#ts-writable>
fn get_writable(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("TransformStream.writable called on non-object")
    })?;
    with_transform_stream_ref(&object, |stream| {
        Ok(JsValue::from(stream.writable()?.object()?))
    })?
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-desired-size>
fn get_desired_size(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStreamDefaultController.desiredSize called on non-object")
    })?;
    with_transform_stream_default_controller_ref(&object, |controller| {
        match controller.desired_size()? {
            Some(size) => Ok(JsValue::from(size)),
            None => Ok(JsValue::null()),
        }
    })?
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-enqueue>
fn controller_enqueue(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStreamDefaultController.enqueue called on non-object")
    })?;
    let chunk = args.get_or_undefined(0).clone();
    let controller = object
        .downcast_ref::<TransformStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("object is not a TransformStreamDefaultController")
        })?
        .clone();
    controller.enqueue(chunk, context)?;
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-error>
fn controller_error(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStreamDefaultController.error called on non-object")
    })?;
    let reason = args.get_or_undefined(0).clone();
    let controller = object
        .downcast_ref::<TransformStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("object is not a TransformStreamDefaultController")
        })?
        .clone();
    controller.error(reason, context)?;
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-terminate>
fn controller_terminate(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStreamDefaultController.terminate called on non-object")
    })?;
    let controller = object
        .downcast_ref::<TransformStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("object is not a TransformStreamDefaultController")
        })?
        .clone();
    controller.terminate(context)?;
    Ok(JsValue::undefined())
}
