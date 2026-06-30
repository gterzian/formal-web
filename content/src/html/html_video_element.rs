use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::html::html_media_element::HTMLMediaElement;

/// <https://html.spec.whatwg.org/#the-video-element>
js_engine::impl_gc_traits! {
    pub struct HTMLVideoElement {
        /// <https://html.spec.whatwg.org/#media-elements>
        pub media_element: HTMLMediaElement,

        /// <https://html.spec.whatwg.org/#dom-video-videowidth>
        video_width: u32,

        /// <https://html.spec.whatwg.org/#dom-video-videoheight>
        video_height: u32,
    }
}

impl HTMLVideoElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            media_element: HTMLMediaElement::new(document, node_id),
            video_width: 0,
            video_height: 0,
        }
    }

    /// <https://html.spec.whatwg.org/#dom-video-videowidth>
    pub(crate) fn video_width(&self) -> u32 {
        // Step 1: If readyState is HAVE_NOTHING, return 0.
        if self.media_element.ready_state == HTMLMediaElement::HAVE_NOTHING {
            return 0;
        }
        // Step 2: Return the natural width of the video in CSS pixels.
        self.video_width
    }

    /// <https://html.spec.whatwg.org/#dom-video-videoheight>
    pub(crate) fn video_height(&self) -> u32 {
        // Step 1: If readyState is HAVE_NOTHING, return 0.
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
