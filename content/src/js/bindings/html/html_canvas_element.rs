type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::HTMLCanvasElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<Types> for HTMLCanvasElement {
    const NAME: &'static str = "HTMLCanvasElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "width",
            getter: get_width,
            setter: Some(set_width),
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
            id: "height",
            getter: get_height,
            setter: Some(set_height),
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
            id: "getContext",
            length: 1,
            method: get_context,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "transferControlToOffscreen",
            length: 0,
            method: transfer_control_to_offscreen,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn get_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let canvas = canvas_from_js_object(this, ec)?;
    Ok(ec.value_from_number(f64::from(canvas.width())))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    let canvas = canvas_from_js_object(this, ec)?;
    canvas.set_width(value, ec)?;
    Ok(ec.value_undefined())
}

fn get_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let canvas = canvas_from_js_object(this, ec)?;
    Ok(ec.value_from_number(f64::from(canvas.height())))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    let canvas = canvas_from_js_object(this, ec)?;
    canvas.set_height(value, ec)?;
    Ok(ec.value_undefined())
}

fn get_context(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // `contextId` is a required argument; Web IDL throws a TypeError when it
    // is missing.
    let context_id_value = args
        .first()
        .cloned()
        .ok_or_else(|| ec.new_type_error("getContext requires 1 argument"))?;
    let context_id = ec.to_rust_string(context_id_value)?;
    let canvas = canvas_from_js_object(this, ec)?;
    let context = canvas.get_context(&context_id, ec)?;
    match context {
        Some(context) => {
            let reflector = context
                .reflector
                .clone()
                .ok_or_else(|| ec.new_type_error("CanvasRenderingContext2D has no reflector"))?;
            Ok(Types::value_from_object(reflector))
        }
        None => Ok(ec.value_null()),
    }
}

fn transfer_control_to_offscreen(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let canvas = canvas_from_js_object(this, ec)?;
    let offscreen = canvas.transfer_control_to_offscreen(ec)?;
    let reflector = offscreen
        .reflector
        .clone()
        .ok_or_else(|| ec.new_type_error("OffscreenCanvas has no reflector"))?;
    Ok(Types::value_from_object(reflector))
}

fn canvas_from_js_object(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<HTMLCanvasElement, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLCanvasElement receiver is not an object"))?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<HTMLCanvasElement>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not an HTMLCanvasElement"))
}
