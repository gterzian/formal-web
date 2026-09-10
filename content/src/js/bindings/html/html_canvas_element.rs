type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLCanvasElement;
use crate::webidl::bindings::{InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLCanvasElement {
    const NAME: &'static str = "HTMLCanvasElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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

fn transfer_control_to_offscreen(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLCanvasElement receiver is not an object"))?;
    let canvas = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<HTMLCanvasElement>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not an HTMLCanvasElement"))?;
    let offscreen = canvas.transfer_control_to_offscreen(ec)?;
    Ok(crate::js::Types::value_from_object(offscreen))
}
