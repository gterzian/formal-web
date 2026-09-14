use ipc_messages::content::CanvasId;
use js_engine::gc_struct;

use super::rendering_context_2d::RenderingContext2D;

/// <https://html.spec.whatwg.org/#offscreencanvasrenderingcontext2d>
#[gc_struct]
pub struct OffscreenCanvasRenderingContext2D {
    /// The output bitmap and drawing state the mixin member algorithms run on.
    #[ignore_trace]
    rendering_context: RenderingContext2D,
}

impl OffscreenCanvasRenderingContext2D {
    pub fn new(canvas_id: CanvasId, width: u32, height: u32) -> Self {
        Self {
            rendering_context: RenderingContext2D::new(canvas_id, width, height),
        }
    }

    /// The output bitmap and drawing state the mixin member algorithms run on.
    pub(crate) fn rendering_context_2d(&self) -> &RenderingContext2D {
        &self.rendering_context
    }
}
