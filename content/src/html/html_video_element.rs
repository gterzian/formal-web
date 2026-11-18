use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use blitz_traits::NodeId;

use crate::html::html_media_element::HTMLMediaElement;
use crate::js::Types;
use js_engine::{ExecutionContext, gc_struct};

/// The media element tasks a newly reported video intrinsic size queues.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VideoIntrinsicSizeEvents {
    pub(crate) resize: bool,
    pub(crate) loaded_metadata: bool,
}

/// <https://html.spec.whatwg.org/#the-video-element>
#[gc_struct]
pub struct HTMLVideoElement {
    /// <https://html.spec.whatwg.org/#media-elements>
    pub media_element: HTMLMediaElement,

    /// <https://html.spec.whatwg.org/#dom-video-videowidth>
    video_width: u32,

    /// <https://html.spec.whatwg.org/#dom-video-videoheight>
    video_height: u32,
}

impl HTMLVideoElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: NodeId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            media_element: HTMLMediaElement::new(document, node_id, ec),
            video_width: 0,
            video_height: 0,
        }
    }

    /// Applies the natural size the graphics process reported for the video
    /// stream, and moves the element to `HAVE_METADATA` once that size is
    /// known. Returns the media element tasks the change queues.
    pub(crate) fn set_video_intrinsic_size(
        &mut self,
        width: u32,
        height: u32,
    ) -> VideoIntrinsicSizeEvents {
        let size_changed = self.video_width != width || self.video_height != height;
        let became_have_metadata = self.media_element.ready_state == HTMLMediaElement::HAVE_NOTHING;
        self.video_width = width;
        self.video_height = height;
        if became_have_metadata {
            self.media_element.ready_state = HTMLMediaElement::HAVE_METADATA;
        }
        VideoIntrinsicSizeEvents {
            resize: became_have_metadata || size_changed,
            loaded_metadata: became_have_metadata,
        }
    }

    /// <https://html.spec.whatwg.org/#dom-video-videowidth>
    pub(crate) fn video_width(&self) -> u32 {
        // Step 1: If this's readyState attribute is HAVE_NOTHING, then return 0.
        if self.media_element.ready_state == HTMLMediaElement::HAVE_NOTHING {
            return 0;
        }

        // Step 2: Return the natural width of the video in CSS pixels.
        self.video_width
    }

    /// <https://html.spec.whatwg.org/#dom-video-videoheight>
    pub(crate) fn video_height(&self) -> u32 {
        // Step 1: If this's readyState attribute is HAVE_NOTHING, then return 0.
        if self.media_element.ready_state == HTMLMediaElement::HAVE_NOTHING {
            return 0;
        }

        // Step 2: Return the natural height of the video in CSS pixels.
        self.video_height
    }

    /// <https://html.spec.whatwg.org/#dom-video-poster>
    pub(crate) fn poster(&self) -> String {
        // Step 1: Return the value of the poster content attribute.
        self.media_element
            .html_element
            .element
            .get_attribute("poster")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-video-poster>
    pub(crate) fn set_poster(&self, poster: &str) {
        // Step 1: Set this's poster content attribute to the given value.
        self.media_element
            .html_element
            .element
            .set_attribute("poster", poster);
    }

    /// <https://html.spec.whatwg.org/#dom-video-playsinline>
    pub(crate) fn plays_inline(&self) -> bool {
        self.media_element
            .html_element
            .element
            .has_attribute("playsinline")
    }

    /// <https://html.spec.whatwg.org/#dom-video-playsinline>
    pub(crate) fn set_plays_inline(&self, value: bool) {
        if value {
            self.media_element
                .html_element
                .element
                .set_attribute("playsinline", "");
        } else {
            self.media_element
                .html_element
                .element
                .remove_attribute("playsinline");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-video-width>
    pub(crate) fn width(&self) -> String {
        self.media_element
            .html_element
            .element
            .get_attribute("width")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-video-width>
    pub(crate) fn set_width(&self, value: &str) {
        self.media_element
            .html_element
            .element
            .set_attribute("width", value);
    }

    /// <https://html.spec.whatwg.org/#dom-video-height>
    pub(crate) fn height(&self) -> String {
        self.media_element
            .html_element
            .element
            .get_attribute("height")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-video-height>
    pub(crate) fn set_height(&self, value: &str) {
        self.media_element
            .html_element
            .element
            .set_attribute("height", value);
    }
}
