type JsValue = <crate::js::Types as JsTypes>::JsValue;
type Types = crate::js::Types;

use crate::html::CanvasRenderingContext2D;
use crate::js::platform_objects::resolve_element_object;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

use super::canvas_context_2d_mixins::{
    define_canvas_fill_stroke_styles_members, define_canvas_rect_members,
    define_canvas_state_members,
};

impl WebIdlInterface<Types> for CanvasRenderingContext2D {
    const NAME: &'static str = "CanvasRenderingContext2D";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "canvas",
            getter: get_canvas,
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
        define_canvas_state_members(def);
        define_canvas_fill_stroke_styles_members(def);
        define_canvas_rect_members(def);
    }
}

fn get_canvas(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("CanvasRenderingContext2D receiver is not an object"))?;
    let context = ec
        .with_object_any(&object)
        .and_then(|data| data.downcast_ref::<CanvasRenderingContext2D>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not a CanvasRenderingContext2D"))?;
    // The canvas attribute returns the element the context is bound to; the
    // element's platform object is resolved from the document's node cache.
    let element = context.canvas_element();
    let element_object = resolve_element_object(element.element.node.node_id, ec)?;
    Ok(Types::value_from_object(element_object))
}
