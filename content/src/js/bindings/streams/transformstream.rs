use boa_engine::{JsResult, JsValue};
use std::marker::PhantomData;

use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use crate::streams::{
    TransformStream, TransformStreamDefaultController, construct_transform_stream,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for TransformStream {
    const NAME: &'static str = "TransformStream";

    fn create_platform_object(
        this: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        // Note: keeps ec_to_ctx — construct_transform_stream takes &mut Context.
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        (|| -> JsResult<Self> { construct_transform_stream(this, args, ctx) })()
            .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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

impl WebIdlInterface<crate::js::Types> for TransformStreamDefaultController {
    const NAME: &'static str = "TransformStreamDefaultController";

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

            id: "enqueue",
            length: 1,
            method: controller_enqueue,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "error",
            length: 1,
            method: controller_error,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "terminate",
            length: 0,
            method: controller_terminate,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Binding functions ──

/// <https://streams.spec.whatwg.org/#ts-readable>
fn get_readable(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStream.readable called on non-object")
    })?;
    // Note: keeps ec_to_ctx — readable_object() returns JsResult and
    // JsError→JsValue conversion requires Context.
    let readable = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(stream) = data.downcast_ref::<TransformStream>() {
                stream.readable_object().map(JsValue::from)
            } else {
                return Err(ec.new_type_error("receiver is not a TransformStream"));
            }
        } else {
            return Err(ec.new_type_error("receiver is not a TransformStream"));
        }
    };
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    readable.map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// <https://streams.spec.whatwg.org/#ts-writable>
fn get_writable(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("TransformStream.writable called on non-object")
    })?;
    // Note: keeps ec_to_ctx — writable_object() returns JsResult.
    let writable = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(stream) = data.downcast_ref::<TransformStream>() {
                stream.writable_object().map(JsValue::from)
            } else {
                return Err(ec.new_type_error("receiver is not a TransformStream"));
            }
        } else {
            return Err(ec.new_type_error("receiver is not a TransformStream"));
        }
    };
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    writable.map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-desired-size>
fn get_desired_size(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error(
            "TransformStreamDefaultController.desiredSize called on non-object",
        )
    })?;
    // Note: keeps ec_to_ctx — desired_size() returns JsResult.
    let size_result = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(controller) =
                data.downcast_ref::<TransformStreamDefaultController>()
            {
                match controller.desired_size() {
                    Ok(Some(size)) => Ok(JsValue::from(size)),
                    Ok(None) => Ok(JsValue::null()),
                    Err(e) => Err(e),
                }
            } else {
                return Err(ec
                    .new_type_error("receiver is not a TransformStreamDefaultController"));
            }
        } else {
            return Err(
                ec.new_type_error("receiver is not a TransformStreamDefaultController")
            );
        }
    };
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    size_result.map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-enqueue>
fn controller_enqueue(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error(
            "TransformStreamDefaultController.enqueue called on non-object",
        )
    })?;
    let chunk = args.first().cloned().unwrap_or(value_undefined.clone());
    // Downcast the controller first, releasing ec's borrow.
    let controller = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(controller) =
                data.downcast_ref::<TransformStreamDefaultController>()
            {
                controller.clone()
            } else {
                return Err(ec
                    .new_type_error("receiver is not a TransformStreamDefaultController"));
            }
        } else {
            return Err(
                ec.new_type_error("receiver is not a TransformStreamDefaultController")
            );
        }
    };
    // Note: keeps ec_to_ctx — enqueue() takes &mut Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    controller
        .enqueue(chunk, ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    Ok(value_undefined)
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-error>
fn controller_error(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error(
            "TransformStreamDefaultController.error called on non-object",
        )
    })?;
    let reason = args.first().cloned().unwrap_or(value_undefined.clone());
    // Downcast the controller first, releasing ec's borrow.
    let controller = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(controller) =
                data.downcast_ref::<TransformStreamDefaultController>()
            {
                controller.clone()
            } else {
                return Err(ec
                    .new_type_error("receiver is not a TransformStreamDefaultController"));
            }
        } else {
            return Err(
                ec.new_type_error("receiver is not a TransformStreamDefaultController")
            );
        }
    };
    // Note: keeps ec_to_ctx — error() takes &mut Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    controller
        .error(reason, ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    Ok(value_undefined)
}

/// <https://streams.spec.whatwg.org/#ts-default-controller-terminate>
fn controller_terminate(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let obj = <crate::js::Types as JsTypes>::value_as_object(this).ok_or_else(|| {
        ec.new_type_error(
            "TransformStreamDefaultController.terminate called on non-object",
        )
    })?;
    // Downcast the controller first, releasing ec's borrow.
    let controller = {
        if let Some(data) = ec.with_object_any(&obj) {
            if let Some(controller) =
                data.downcast_ref::<TransformStreamDefaultController>()
            {
                controller.clone()
            } else {
                return Err(ec
                    .new_type_error("receiver is not a TransformStreamDefaultController"));
            }
        } else {
            return Err(
                ec.new_type_error("receiver is not a TransformStreamDefaultController")
            );
        }
    };
    // Note: keeps ec_to_ctx — terminate() takes &mut Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    controller
        .terminate(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined.clone()))?;
    Ok(value_undefined)
}
