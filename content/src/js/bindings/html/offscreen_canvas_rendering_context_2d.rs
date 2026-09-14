type Types = crate::js::Types;

use crate::html::OffscreenCanvasRenderingContext2D;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};

use super::canvas_context_2d_mixins::{
    define_canvas_fill_stroke_styles_members, define_canvas_rect_members,
    define_canvas_state_members,
};

impl WebIdlInterface<Types> for OffscreenCanvasRenderingContext2D {
    const NAME: &'static str = "OffscreenCanvasRenderingContext2D";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        define_canvas_state_members(def);
        define_canvas_fill_stroke_styles_members(def);
        define_canvas_rect_members(def);
    }
}
