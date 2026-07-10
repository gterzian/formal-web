use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use crate::streams::{
    TransformStream, TransformStreamDefaultController, construct_transform_stream,
    with_transform_stream_default_controller_ref, with_transform_stream_ref,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for TransformStream {
    const NAME: &'static str = "TransformStream";

    fn create_platform_object(
        this: &<crate::js::Types as JsTypes>::JsValue,
        args: &[<crate::js::Types as JsTypes>::JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        construct_transform_stream(this, args, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            exposed: None,
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
            exposed: None,
        });
    }
}

impl WebIdlInterface<crate::js::Types> for TransformStreamDefaultController {
    const NAME: &'static str = "TransformStreamDefaultController";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            id: "enqueue",
            length: 1,
            method: controller_enqueue,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "error",
            length: 1,
            method: controller_error,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "terminate",
            length: 0,
            method: controller_terminate,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}


/// <https://streams.spec.whatwg.org/#ts-readable>
fn get_readable(
    this: &<crate::js::Types as JsTypes>::JsValue,
    _: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TransformStream.readable called on non-object"))?;
    let stream = with_transform_stream_ref(&obj, ec, |s| s.clone())?;
    let readable = stream.readable_object(ec)?;
    Ok(readable.into())
}

/// <https://streams.spec.whatwg.org/#ts-writable>
fn get_writable(
    this: &<crate::js::Types as JsTypes>::JsValue,
    _: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("TransformStream.writable called on non-object"))?;
    let stream = with_transform_stream_ref(&obj, ec, |s| s.clone())?;
    let writable = stream.writable_object(ec)?;
    Ok(writable.into())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-desired-size>
fn get_desired_size(
    this: &<crate::js::Types as JsTypes>::JsValue,
    _: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStreamDefaultController.desiredSize called on non-object")
    })?;
    let controller = with_transform_stream_default_controller_ref(&obj, ec, |c| c.clone())?;
    let size = controller.desired_size(ec)?;
    match size {
        Some(s) => Ok(s.into()),
        None => Ok(ec.value_null()),
    }
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-enqueue>
fn controller_enqueue(
    this: &<crate::js::Types as JsTypes>::JsValue,
    args: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStreamDefaultController.enqueue called on non-object")
    })?;
    let chunk = args.first().cloned().unwrap_or(ec.value_undefined());
    let controller = with_transform_stream_default_controller_ref(&obj, ec, |c| c.clone())?;
    controller.enqueue(chunk, ec)?;
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-error>
fn controller_error(
    this: &<crate::js::Types as JsTypes>::JsValue,
    args: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStreamDefaultController.error called on non-object")
    })?;
    let reason = args.first().cloned().unwrap_or(ec.value_undefined());
    let controller = with_transform_stream_default_controller_ref(&obj, ec, |c| c.clone())?;
    controller.error(reason, ec)?;
    Ok(ec.value_undefined())
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-terminate>
fn controller_terminate(
    this: &<crate::js::Types as JsTypes>::JsValue,
    _: &[<crate::js::Types as JsTypes>::JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStreamDefaultController.terminate called on non-object")
    })?;
    let controller = with_transform_stream_default_controller_ref(&obj, ec, |c| c.clone())?;
    controller.terminate(ec)?;
    Ok(ec.value_undefined())
}
