type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::OffscreenCanvas;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for OffscreenCanvas {
    const NAME: &'static str = "OffscreenCanvas";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            id: "width",
            getter: get_width,
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
            id: "height",
            getter: get_height,
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
            id: "getContext",
            length: 1,
            method: get_context,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn try_with_offscreen_canvas_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&OffscreenCanvas, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("OffscreenCanvas receiver is not an object"))?;
    let canvas = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<OffscreenCanvas>().cloned());
    let Some(canvas) = canvas else {
        return Err(ec.new_type_error("receiver is not an OffscreenCanvas"));
    };
    Ok(f(&canvas, ec))
}

fn get_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let width = try_with_offscreen_canvas_ref(this, ec, |canvas, _ec| canvas.width())?;
    Ok(ec.value_from_number(f64::from(width)))
}

fn get_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let height = try_with_offscreen_canvas_ref(this, ec, |canvas, _ec| canvas.height())?;
    Ok(ec.value_from_number(f64::from(height)))
}

fn get_context(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let context_id = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let context =
        try_with_offscreen_canvas_ref(this, ec, |canvas, ec| canvas.get_context(&context_id, ec))??;
    Ok(match context {
        Some(context) => crate::js::Types::value_from_object(context),
        None => ec.value_null(),
    })
}
