use ipc_messages::content::CanvasId;
use js_engine::gc_struct;

use crate::html::HTMLElement;

use super::rendering_context_2d::RenderingContext2D;

/// <https://html.spec.whatwg.org/#canvasrenderingcontext2d>
#[gc_struct]
pub struct CanvasRenderingContext2D {
    /// The output bitmap and drawing state the mixin member algorithms run on.
    #[ignore_trace]
    rendering_context: RenderingContext2D,

    /// The canvas element this context is permanently bound to (the `canvas`
    /// attribute's value).
    html_element: HTMLElement,
}

impl CanvasRenderingContext2D {
    pub(crate) fn new(
        canvas_id: CanvasId,
        width: u32,
        height: u32,
        html_element: HTMLElement,
    ) -> Self {
        Self {
            rendering_context: RenderingContext2D::new(canvas_id, width, height),
            html_element,
        }
    }

    /// The output bitmap and drawing state the mixin member algorithms run on.
    pub(crate) fn rendering_context_2d(&self) -> &RenderingContext2D {
        &self.rendering_context
    }

    /// The canvas element this context is permanently bound to.
    pub(crate) fn canvas_element(&self) -> HTMLElement {
        self.html_element.clone()
    }
}
