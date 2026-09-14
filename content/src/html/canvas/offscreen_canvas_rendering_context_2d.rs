use ipc_messages::content::CanvasId;
use js_engine::{ExecutionContext, gc_struct};

use crate::js::Types;

use super::rendering_context_2d::{CanvasContext2D, RenderingContext2D};

/// <https://html.spec.whatwg.org/multipage/canvas.html#offscreencanvasrenderingcontext2d>
#[gc_struct]
pub struct OffscreenCanvasRenderingContext2D {
    /// The output bitmap and drawing state shared with the mixin
    /// implementations.
    #[ignore_trace]
    rendering_context: RenderingContext2D,
}

impl CanvasContext2D for OffscreenCanvasRenderingContext2D {
    fn rendering_context_2d(&self) -> &RenderingContext2D {
        &self.rendering_context
    }
}

impl OffscreenCanvasRenderingContext2D {
    pub fn new(
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            rendering_context: RenderingContext2D::new(canvas_id, width, height),
        }
    }
}
