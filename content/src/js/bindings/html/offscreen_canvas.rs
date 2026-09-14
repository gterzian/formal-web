type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::OffscreenCanvas;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::enforce_range_unsigned_long_long;

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for OffscreenCanvas {
    const NAME: &'static str = "OffscreenCanvas";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn constructor_length() -> usize {
        2
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // `width` and `height` are required arguments; Web IDL throws a TypeError when they are
        // missing.
        let (Some(width), Some(height)) = (args.first(), args.get(1)) else {
            return Err(ec.new_type_error("OffscreenCanvas requires 2 arguments"));
        };
        // The arguments are `[EnforceRange] unsigned long long`.
        let width = enforce_range_unsigned_long_long(width, ec)?;
        let height = enforce_range_unsigned_long_long(height, ec)?;
        OffscreenCanvas::constructor(width, height, ec)
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
    let obj = Types::value_as_object(this)
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
    // `contextId` is a required argument; Web IDL throws a TypeError when it
    // is missing.
    let context_id_value = args
        .first()
        .cloned()
        .ok_or_else(|| ec.new_type_error("getContext requires 1 argument"))?;
    let context_id = ec.to_rust_string(context_id_value)?;
    // `contextId` is an `OffscreenRenderingContextId` enum; a value outside the
    // enum throws a TypeError rather than reaching the getContext steps.
    if !OFFSCREEN_RENDERING_CONTEXT_IDS.contains(&context_id.as_str()) {
        return Err(ec.new_type_error(&format!(
            "{context_id} is not a valid OffscreenRenderingContextId"
        )));
    }
    let context =
        try_with_offscreen_canvas_ref(this, ec, |canvas, ec| canvas.get_context(&context_id, ec))??;
    match context {
        Some(context) => {
            let reflector = context.reflector.clone().ok_or_else(|| {
                ec.new_type_error("OffscreenCanvasRenderingContext2D has no reflector")
            })?;
            Ok(Types::value_from_object(reflector))
        }
        None => Ok(ec.value_null()),
    }
}

/// <https://html.spec.whatwg.org/#offscreenrenderingcontextid>
const OFFSCREEN_RENDERING_CONTEXT_IDS: [&str; 5] =
    ["2d", "bitmaprenderer", "webgl", "webgl2", "webgpu"];
