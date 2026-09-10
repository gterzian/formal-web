type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::OffscreenCanvasRenderingContext2D;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for OffscreenCanvasRenderingContext2D {
    const NAME: &'static str = "OffscreenCanvasRenderingContext2D";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            id: "fillStyle",
            getter: get_fill_style,
            setter: Some(set_fill_style),
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
            id: "fillRect",
            length: 4,
            method: fill_rect,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "clearRect",
            length: 4,
            method: clear_rect,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn try_with_context_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(
        &OffscreenCanvasRenderingContext2D,
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<R, crate::js::Types>,
) -> Completion<R, crate::js::Types> {
    let obj = Types::value_as_object(this).ok_or_else(|| {
        ec.new_type_error("OffscreenCanvasRenderingContext2D receiver is not an object")
    })?;
    let context = ec.with_object_any(&obj).and_then(|data| {
        data.downcast_ref::<OffscreenCanvasRenderingContext2D>()
            .cloned()
    });
    let Some(context) = context else {
        return Err(ec.new_type_error("receiver is not an OffscreenCanvasRenderingContext2D"));
    };
    f(&context, ec)
}

fn get_fill_style(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_context_ref(this, ec, |context, _ec| Ok(context.fill_style_value()))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_fill_style(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_context_ref(this, ec, |context, _ec| {
        context.set_fill_style(&value);
        Ok(())
    })?;
    Ok(ec.value_undefined())
}

fn fill_rect(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let x = ec.to_number(args.first().cloned().unwrap_or(undef.clone()))?;
    let y = ec.to_number(args.get(1).cloned().unwrap_or(undef.clone()))?;
    let width = ec.to_number(args.get(2).cloned().unwrap_or(undef.clone()))?;
    let height = ec.to_number(args.get(3).cloned().unwrap_or(undef))?;
    try_with_context_ref(this, ec, |context, ec| {
        context.fill_rect(x, y, width, height, ec)
    })?;
    Ok(ec.value_undefined())
}

fn clear_rect(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let x = ec.to_number(args.first().cloned().unwrap_or(undef.clone()))?;
    let y = ec.to_number(args.get(1).cloned().unwrap_or(undef.clone()))?;
    let width = ec.to_number(args.get(2).cloned().unwrap_or(undef.clone()))?;
    let height = ec.to_number(args.get(3).cloned().unwrap_or(undef))?;
    try_with_context_ref(this, ec, |context, ec| {
        context.clear_rect(x, y, width, height, ec)
    })?;
    Ok(ec.value_undefined())
}
