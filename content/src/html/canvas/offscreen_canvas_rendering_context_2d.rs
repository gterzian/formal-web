use ipc_messages::content::CanvasId;
use js_engine::JsTypes;
use js_engine::gc_struct;

use crate::js::Types;

use super::rendering_context_2d::RenderingContext2D;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#offscreencanvasrenderingcontext2d>
#[gc_struct]
pub struct OffscreenCanvasRenderingContext2D {
    /// The JS object implementing this interface, set by the Web IDL layer.
    pub(crate) reflector: Option<JsObject>,

    /// The output bitmap and drawing state the mixin member algorithms run on.
    #[ignore_trace]
    rendering_context: RenderingContext2D,
}

impl OffscreenCanvasRenderingContext2D {
    pub fn new(canvas_id: CanvasId, width: u32, height: u32) -> Self {
        Self {
            reflector: None,
            rendering_context: RenderingContext2D::new(canvas_id, width, height),
        }
    }

    /// The output bitmap and drawing state the mixin member algorithms run on.
    pub(crate) fn rendering_context_2d(&self) -> &RenderingContext2D {
        &self.rendering_context
    }
}
