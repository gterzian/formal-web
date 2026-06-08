use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
};

use crate::webidl::binding::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

use crate::streams::{
    TransformStream, TransformStreamDefaultController, construct_transform_stream,
    with_transform_stream_default_controller_ref, with_transform_stream_ref,
};

impl WebIdlInterface for TransformStream {
    const NAME: &'static str = "TransformStream";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        construct_transform_stream(this, args, context)
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(AttributeDef {
            id: "readable",
            getter: get_readable,
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
            id: "writable",
            getter: get_writable,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

impl WebIdlInterface for TransformStreamDefaultController {
    const NAME: &'static str = "TransformStreamDefaultController";

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
            id: "enqueue",
            length: 1,
            method: controller_enqueue,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: controller_error,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "terminate",
            length: 0,
            method: controller_terminate,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

/// <https://streams.spec.whatwg.org/#ts-readable>
fn get_readable(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("TransformStream.readable called on non-object")
    })?;
    with_transform_stream_ref(&object, |stream| {
        Ok(JsValue::from(stream.readable_object()?))
    })?
}

/// <https://streams.spec.whatwg.org/#ts-writable>
fn get_writable(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("TransformStream.writable called on non-object")
    })?;
    with_transform_stream_ref(&object, |stream| {
        Ok(JsValue::from(stream.writable_object()?))
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
            JsNativeError::typ().with_message("object is not a TransformStreamDefaultController")
        })?
        .clone();
    controller.enqueue(chunk, context)?;
    Ok(JsValue::undefined())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-error>
fn controller_error(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let object = _this.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("TransformStreamDefaultController.error called on non-object")
    })?;
    let reason = args.get_or_undefined(0).clone();
    let controller = object
        .downcast_ref::<TransformStreamDefaultController>()
        .ok_or_else(|| {
            JsNativeError::typ().with_message("object is not a TransformStreamDefaultController")
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
            JsNativeError::typ().with_message("object is not a TransformStreamDefaultController")
        })?
        .clone();
    controller.terminate(context)?;
    Ok(JsValue::undefined())
}
