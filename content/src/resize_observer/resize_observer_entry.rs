use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};

use super::observer::ResizeObserverBoxOptions;
use super::processing_model::calculate_box_size;
use super::resize_observer_size::ResizeObserverSize;
use crate::dom::Element;
use crate::geometry::DOMRectReadOnly;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://drafts.csswg.org/resize-observer/#resizeobserverentry>
#[gc_struct]
pub struct ResizeObserverEntry {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-target>
    pub(crate) target: Element,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-contentrect>
    pub(crate) content_rect: DOMRectReadOnly,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-borderboxsize>
    pub(crate) border_box_size: Vec<ResizeObserverSize>,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-contentboxsize>
    pub(crate) content_box_size: Vec<ResizeObserverSize>,

    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-devicepixelcontentboxsize>
    pub(crate) device_pixel_content_box_size: Vec<ResizeObserverSize>,

    pub(crate) reflector: Option<JsObject>,
}

impl ResizeObserverEntry {
    /// <https://drafts.csswg.org/resize-observer/#dom-resizeobserverentry-resizeobserverentry>
    pub(crate) fn new(
        target: Element,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<ResizeObserverEntry, Types> {
        // Step 1: "Let this be a new ResizeObserverEntry."
        // Step 2: "Set this.target slot to target."
        // Step 3: "Set this.borderBoxSize slot to result of calculating box size given target and observedBox of "border-box"."
        let border_box_size = calculate_box_size(&target, ResizeObserverBoxOptions::BorderBox);

        // Step 4: "Set this.contentBoxSize slot to result of calculating box size given target and observedBox of "content-box"."
        let content_box_size = calculate_box_size(&target, ResizeObserverBoxOptions::ContentBox);

        // Step 5: "Set this.devicePixelContentBoxSize slot to result of calculating box size given target and observedBox of "device-pixel-content-box"."
        let device_pixel_content_box_size =
            calculate_box_size(&target, ResizeObserverBoxOptions::DevicePixelContentBox);

        // Step 6: "Set this.contentRect to logical this.contentBoxSize given target and observedBox of "content-box"."
        // Step 7: "If target is not an SVG element or target is an SVG element with an associated CSS layout box do these steps:"
        // Step 7.1: "Set this.contentRect.top to target.padding top."
        // Step 7.2: "Set this.contentRect.left to target.padding left."
        // Step 8: "If target is an SVG element without an associated CSS layout box do these steps:"
        // Step 8.1: "Set this.contentRect.top and this.contentRect.left to 0."
        // Note: every element here has a CSS layout box, so step 8 does not
        // apply; the horizontal writing mode maps inline to width and block
        // to height.
        let metrics = target.box_metrics().unwrap_or_default();
        let content_rect = DOMRectReadOnly::new(
            metrics.padding_left,
            metrics.padding_top,
            content_box_size.inline_size,
            content_box_size.block_size,
            ec,
        )?;

        let entry = ResizeObserverEntry {
            target,
            content_rect,
            border_box_size: vec![ResizeObserverSize::new(border_box_size, ec)?],
            content_box_size: vec![ResizeObserverSize::new(content_box_size, ec)?],
            device_pixel_content_box_size: vec![ResizeObserverSize::new(
                device_pixel_content_box_size,
                ec,
            )?],
            reflector: None,
        };
        let object = create_interface_instance::<Types, ResizeObserverEntry>(entry, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<ResizeObserverEntry>().cloned())
            .ok_or_else(|| {
                ec.new_type_error("ResizeObserverEntry instance is not a ResizeObserverEntry")
            })
    }
}
