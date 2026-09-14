type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::OffscreenCanvas;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

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
        // Note: the bitmap dimensions are stored as 32-bit values, so a width or height above
        // `u32::MAX` saturates.
        let width = u32::try_from(width).unwrap_or(u32::MAX);
        let height = u32::try_from(height).unwrap_or(u32::MAX);
        // Step 1: "Initialize the bitmap of this to a rectangular array of transparent black pixels of the dimensions specified by width and height."
        // Step 2: "Initialize the width of this to width."
        // Step 3: "Initialize the height of this to height."
        // Note: the bitmap is realized as the graphics-process canvas slot; a canvas created by
        // the constructor has no placeholder canvas element, so it has no embed site.
        // Step 4: "Set this's inherited language to explicitly unknown."
        // Step 5: "Set this's inherited direction to \"ltr\"."
        // Step 6: "Let global be the relevant global object of this."
        // Step 7: "If global is a Window object:"
        // Step 7.1: "Let element be the document element of global's associated Document."
        // Step 7.2: "If element is not null:"
        // Step 7.2.1: "Set the inherited language of this to element's language."
        // Step 7.2.2: "Set the inherited direction of this to element's directionality."
        // Note: inherited language and direction are not modeled.
        let canvas = OffscreenCanvas::new_standalone(width, height, ec);
        canvas.register_canvas_with_graphics(ec)?;
        Ok(canvas)
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
    Ok(match context {
        Some(context) => Types::value_from_object(context),
        None => ec.value_null(),
    })
}

/// <https://html.spec.whatwg.org/#offscreenrenderingcontextid>
const OFFSCREEN_RENDERING_CONTEXT_IDS: [&str; 5] =
    ["2d", "bitmaprenderer", "webgl", "webgl2", "webgpu"];

/// <https://webidl.spec.whatwg.org/#js-to-unsigned-long-long>
fn enforce_range_unsigned_long_long(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u64, Types> {
    // Note: the value is converted with the [EnforceRange] rules
    // (<https://webidl.spec.whatwg.org/#EnforceRange>): the number is rounded toward zero and an
    // out-of-range value throws a TypeError instead of wrapping.
    // Step 1: "Let x be ? ConvertToInt(V, 64, \"unsigned\")."
    let number = ec.to_number(value.clone())?;
    let rounded = number.trunc();
    if !(0.0..18_446_744_073_709_551_616.0).contains(&rounded) {
        return Err(ec.new_type_error("value is outside the unsigned long long range"));
    }
    // Step 2: "Return the IDL unsigned long long value that represents the same numeric value as x."
    Ok(rounded as u64)
}
