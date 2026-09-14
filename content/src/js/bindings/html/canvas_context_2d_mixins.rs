type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::{
    CanvasContext2D, CanvasFillStrokeStyles, CanvasRect, CanvasRenderingContext2D, CanvasState,
    OffscreenCanvasRenderingContext2D, RenderingContext2D,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef};

use js_engine::{Completion, ExecutionContext, JsTypes};

// A 2D rendering context receiver, either interface, resolved to its shared
// RenderingContext2D so the mixin member definitions below are shared by both
// interfaces' bindings.
enum CanvasContext2DHandle {
    Canvas(CanvasRenderingContext2D),
    Offscreen(OffscreenCanvasRenderingContext2D),
}

impl CanvasContext2D for CanvasContext2DHandle {
    fn rendering_context_2d(&self) -> &RenderingContext2D {
        match self {
            CanvasContext2DHandle::Canvas(context) => context.rendering_context_2d(),
            CanvasContext2DHandle::Offscreen(context) => context.rendering_context_2d(),
        }
    }
}

fn with_canvas_context<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&CanvasContext2DHandle, &mut dyn ExecutionContext<Types>) -> Completion<R, Types>,
) -> Completion<R, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("2D rendering context receiver is not an object"))?;
    let handle = ec.with_object_any(&object).and_then(|data| {
        data.downcast_ref::<CanvasRenderingContext2D>()
            .cloned()
            .map(CanvasContext2DHandle::Canvas)
            .or_else(|| {
                data.downcast_ref::<OffscreenCanvasRenderingContext2D>()
                    .cloned()
                    .map(CanvasContext2DHandle::Offscreen)
            })
    });
    let Some(handle) = handle else {
        return Err(ec.new_type_error("receiver is not a 2D rendering context"));
    };
    f(&handle, ec)
}

/// <https://html.spec.whatwg.org/#canvasstate>
pub(crate) fn define_canvas_state_members(def: &mut InterfaceDefinition<Types>) {
    def.add_operation(OperationDef {
        id: "save",
        length: 0,
        method: save,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "restore",
        length: 0,
        method: restore,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "reset",
        length: 0,
        method: reset,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
    def.add_operation(OperationDef {
        id: "isContextLost",
        length: 0,
        method: is_context_lost,
        static_: false,
        unforgeable: false,
        promise_type: false,
        exposed: None,
    });
}

fn save(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_canvas_context(this, ec, |context, _ec| {
        context.save();
        Ok(())
    })?;
    Ok(ec.value_undefined())
}

fn restore(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_canvas_context(this, ec, |context, _ec| {
        context.restore();
        Ok(())
    })?;
    Ok(ec.value_undefined())
}

fn reset(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    with_canvas_context(this, ec, |context, ec| context.reset(ec))?;
    Ok(ec.value_undefined())
}

fn is_context_lost(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let lost = with_canvas_context(this, ec, |context, _ec| Ok(context.is_context_lost()))?;
    Ok(ec.value_from_bool(lost))
}

/// <https://html.spec.whatwg.org/#canvasfillstrokestyles>
pub(crate) fn define_canvas_fill_stroke_styles_members(def: &mut InterfaceDefinition<Types>) {
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
}

/// <https://html.spec.whatwg.org/#canvasrect>
pub(crate) fn define_canvas_rect_members(def: &mut InterfaceDefinition<Types>) {
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

fn get_fill_style(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = with_canvas_context(this, ec, |context, _ec| Ok(context.fill_style_value()))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_fill_style(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    with_canvas_context(this, ec, |context, _ec| {
        context.set_fill_style(&value);
        Ok(())
    })?;
    Ok(ec.value_undefined())
}

fn fill_rect(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let x = ec.to_number(args.first().cloned().unwrap_or(undefined.clone()))?;
    let y = ec.to_number(args.get(1).cloned().unwrap_or(undefined.clone()))?;
    let width = ec.to_number(args.get(2).cloned().unwrap_or(undefined.clone()))?;
    let height = ec.to_number(args.get(3).cloned().unwrap_or(undefined))?;
    with_canvas_context(this, ec, |context, ec| {
        context.fill_rect(x, y, width, height, ec)
    })?;
    Ok(ec.value_undefined())
}

fn clear_rect(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let x = ec.to_number(args.first().cloned().unwrap_or(undefined.clone()))?;
    let y = ec.to_number(args.get(1).cloned().unwrap_or(undefined.clone()))?;
    let width = ec.to_number(args.get(2).cloned().unwrap_or(undefined.clone()))?;
    let height = ec.to_number(args.get(3).cloned().unwrap_or(undefined))?;
    with_canvas_context(this, ec, |context, ec| {
        context.clear_rect(x, y, width, height, ec)
    })?;
    Ok(ec.value_undefined())
}
