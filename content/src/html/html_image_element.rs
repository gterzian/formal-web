use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use js_engine::{Completion, ExecutionContext, JsTypes, gc_struct};
use url::Url;

use crate::dom::DOMException;
use crate::html::HTMLElement;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::rejected_promise;

/// <https://html.spec.whatwg.org/#htmlimageelement>
#[gc_struct]
pub struct HTMLImageElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLImageElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
        }
    }

    fn src_attribute(&self) -> String {
        self.html_element
            .element
            .get_attribute("src")
            .unwrap_or_default()
    }

    /// The decoded image's dimensions, or `None` when the image is not
    /// available.
    fn natural_dimensions(&self) -> Option<(u32, u32)> {
        let document = self.html_element.element.node.document.borrow();
        let node = document.get_node(self.html_element.element.node.node_id)?;
        let raster = node.element_data()?.raster_image_data()?;
        Some((raster.width, raster.height))
    }

    /// <https://html.spec.whatwg.org/#dom-img-alt>
    pub(crate) fn alt(&self) -> String {
        // Step 1: "Return the value of the alt content attribute."
        self.html_element
            .element
            .get_attribute("alt")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-alt>
    pub(crate) fn set_alt(&self, alt: &str) {
        // Step 1: "Set this's alt content attribute to the given value."
        self.html_element.element.set_attribute("alt", alt);
    }

    /// <https://html.spec.whatwg.org/#dom-img-src>
    pub(crate) fn src(&self, document_creation_url: &Url) -> String {
        // Step 1: "Reinitialize url."
        // Step 2: "Let url be this's url."
        // Step 3: "If url is null and this has no src content attribute, return the empty string."
        let value = self.src_attribute();
        if value.is_empty() {
            return String::new();
        }

        // Step 4: "Otherwise, if url is null, return this's src content attribute's value."
        // Step 5: "Return url, serialized."
        document_creation_url
            .join(&value)
            .map(|url| url.to_string())
            .unwrap_or(value)
    }

    /// <https://html.spec.whatwg.org/#dom-img-src>
    pub(crate) fn set_src(&self, src: &str) {
        // Step 1: "Set this's src content attribute to the given value."
        self.html_element.element.set_attribute("src", src);
    }

    /// <https://html.spec.whatwg.org/#dom-img-srcset>
    pub(crate) fn srcset(&self) -> String {
        // Step 1: "Return the value of the srcset content attribute."
        self.html_element
            .element
            .get_attribute("srcset")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-srcset>
    pub(crate) fn set_srcset(&self, srcset: &str) {
        // Step 1: "Set this's srcset content attribute to the given value."
        self.html_element.element.set_attribute("srcset", srcset);
    }

    /// <https://html.spec.whatwg.org/#dom-img-sizes>
    pub(crate) fn sizes(&self) -> String {
        // Step 1: "Return the value of the sizes content attribute."
        self.html_element
            .element
            .get_attribute("sizes")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-sizes>
    pub(crate) fn set_sizes(&self, sizes: &str) {
        // Step 1: "Set this's sizes content attribute to the given value."
        self.html_element.element.set_attribute("sizes", sizes);
    }

    /// <https://html.spec.whatwg.org/#dom-img-crossorigin>
    pub(crate) fn cross_origin(&self) -> Option<String> {
        // Step 1: "Return the value of the crossorigin content attribute, limited to only known values."
        let value = self.html_element.element.get_attribute("crossorigin")?;
        if value.eq_ignore_ascii_case("anonymous") {
            Some(String::from("anonymous"))
        } else if value.eq_ignore_ascii_case("use-credentials") {
            Some(String::from("use-credentials"))
        } else {
            None
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-crossorigin>
    pub(crate) fn set_cross_origin(&self, value: Option<&str>) {
        // Step 1: "Set the crossorigin content attribute to the given value, limited to only known values."
        match value {
            Some(value) => self
                .html_element
                .element
                .set_attribute("crossorigin", value),
            None => self.html_element.element.remove_attribute("crossorigin"),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-usemap>
    pub(crate) fn use_map(&self) -> String {
        // Step 1: "Return the value of the usemap content attribute."
        self.html_element
            .element
            .get_attribute("usemap")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-usemap>
    pub(crate) fn set_use_map(&self, use_map: &str) {
        // Step 1: "Set this's usemap content attribute to the given value."
        self.html_element.element.set_attribute("usemap", use_map);
    }

    /// <https://html.spec.whatwg.org/#dom-img-ismap>
    pub(crate) fn is_map(&self) -> bool {
        // Step 1: "Return true if the ismap attribute is present; otherwise false."
        self.html_element.element.has_attribute("ismap")
    }

    /// <https://html.spec.whatwg.org/#dom-img-ismap>
    pub(crate) fn set_is_map(&self, value: bool) {
        // Step 1: "Set the ismap attribute to the empty string if the given value is true; otherwise remove it."
        if value {
            self.html_element.element.set_attribute("ismap", "");
        } else {
            self.html_element.element.remove_attribute("ismap");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-controls>
    pub(crate) fn controls(&self) -> bool {
        // Step 1: "Return true if the controls attribute is present; otherwise false."
        self.html_element.element.has_attribute("controls")
    }

    /// <https://html.spec.whatwg.org/#dom-img-controls>
    pub(crate) fn set_controls(&self, value: bool) {
        // Step 1: "Set the controls attribute to the empty string if the given value is true; otherwise remove it."
        if value {
            self.html_element.element.set_attribute("controls", "");
        } else {
            self.html_element.element.remove_attribute("controls");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-width>
    pub(crate) fn width(&self) -> u32 {
        // Step 1: "Return the width of this's dimensions."
        // Note: The element's rendered dimensions are not tracked; when the
        // width content attribute is present its value is returned, otherwise
        // the density-corrected natural width.
        if let Some(width) = self
            .html_element
            .element
            .get_attribute("width")
            .and_then(|value| value.trim().parse::<u32>().ok())
        {
            return width;
        }
        self.natural_width()
    }

    /// <https://html.spec.whatwg.org/#dom-img-width>
    pub(crate) fn set_width(&self, value: u32) {
        // Step 1: "Set this's width content attribute to the given value."
        self.html_element
            .element
            .set_attribute("width", &value.to_string());
    }

    /// <https://html.spec.whatwg.org/#dom-img-height>
    pub(crate) fn height(&self) -> u32 {
        // Step 1: "Return the height of this's dimensions."
        // Note: The element's rendered dimensions are not tracked; when the
        // height content attribute is present its value is returned, otherwise
        // the density-corrected natural height.
        if let Some(height) = self
            .html_element
            .element
            .get_attribute("height")
            .and_then(|value| value.trim().parse::<u32>().ok())
        {
            return height;
        }
        self.natural_height()
    }

    /// <https://html.spec.whatwg.org/#dom-img-height>
    pub(crate) fn set_height(&self, value: u32) {
        // Step 1: "Set this's height content attribute to the given value."
        self.html_element
            .element
            .set_attribute("height", &value.to_string());
    }

    /// <https://html.spec.whatwg.org/#dom-img-naturalwidth>
    pub(crate) fn natural_width(&self) -> u32 {
        // Step 1: If the image is not available, then return 0.
        // Step 2: Return the respective component of the image's density-corrected natural width and height, in CSS pixels.
        // Note: Density correction is not modeled; the decoded pixel width is returned.
        self.natural_dimensions()
            .map(|(width, _)| width)
            .unwrap_or(0)
    }

    /// <https://html.spec.whatwg.org/#dom-img-naturalheight>
    pub(crate) fn natural_height(&self) -> u32 {
        // Step 1: If the image is not available, then return 0.
        // Step 2: Return the respective component of the image's density-corrected natural width and height, in CSS pixels.
        // Note: Density correction is not modeled; the decoded pixel height is returned.
        self.natural_dimensions()
            .map(|(_, height)| height)
            .unwrap_or(0)
    }

    /// <https://html.spec.whatwg.org/#dom-img-complete>
    pub(crate) fn complete(&self) -> bool {
        // Step 1: If any of the following are true: both the src attribute and the srcset attribute are omitted; the srcset attribute is omitted and the src attribute's value is the empty string; the img element's current request's state is completely available and its pending request is null; or the img element's current request's state is broken and its pending request is null, then return true.
        // Note: The current request's state is not modeled separately; the
        // image is treated as completely available once its decoded data is
        // present on the element, and broken is never entered.
        let has_src = self
            .html_element
            .element
            .get_attribute("src")
            .is_some_and(|value| !value.is_empty());
        let has_srcset = self
            .html_element
            .element
            .get_attribute("srcset")
            .is_some_and(|value| !value.is_empty());
        if !has_src && !has_srcset {
            return true;
        }

        self.natural_dimensions().is_some()
    }

    /// <https://html.spec.whatwg.org/#dom-img-currentsrc>
    pub(crate) fn current_src(&self, document_creation_url: &Url) -> String {
        // Step 1: Return the current request's current URL.
        // Note: The current request's URL is approximated by the resolved src
        // content attribute; srcset selection is not modeled.
        let value = self.src_attribute();
        if value.is_empty() {
            return String::new();
        }
        document_creation_url
            .join(&value)
            .map(|url| url.to_string())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-referrerpolicy>
    pub(crate) fn referrer_policy(&self) -> String {
        // Step 1: "Return the value of the referrerpolicy content attribute, limited to only known values."
        self.html_element
            .element
            .get_attribute("referrerpolicy")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-img-referrerpolicy>
    pub(crate) fn set_referrer_policy(&self, value: &str) {
        // Step 1: "Set this's referrerpolicy content attribute to the given value."
        self.html_element
            .element
            .set_attribute("referrerpolicy", value);
    }

    /// <https://html.spec.whatwg.org/#dom-img-decoding>
    pub(crate) fn decoding(&self) -> String {
        // Step 1: "Return the value of the decoding content attribute, limited to only known values."
        // Note: The missing value default is "auto".
        let value = self.html_element.element.get_attribute("decoding");
        match value.as_deref() {
            Some("sync") => String::from("sync"),
            Some("async") => String::from("async"),
            _ => String::from("auto"),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-decoding>
    pub(crate) fn set_decoding(&self, value: &str) {
        // Step 1: "Set this's decoding content attribute to the given value."
        self.html_element.element.set_attribute("decoding", value);
    }

    /// <https://html.spec.whatwg.org/#dom-img-loading>
    pub(crate) fn loading(&self) -> String {
        // Step 1: "Return the value of the loading content attribute, limited to only known values."
        // Note: The missing value default is "eager".
        let value = self.html_element.element.get_attribute("loading");
        match value.as_deref() {
            Some("lazy") => String::from("lazy"),
            Some("eager") => String::from("eager"),
            _ => String::from("eager"),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-loading>
    pub(crate) fn set_loading(&self, value: &str) {
        // Step 1: "Set this's loading content attribute to the given value."
        self.html_element.element.set_attribute("loading", value);
    }

    /// <https://html.spec.whatwg.org/#dom-img-fetchpriority>
    pub(crate) fn fetch_priority(&self) -> String {
        // Step 1: "Return the value of the fetchpriority content attribute, limited to only known values."
        // Note: The missing value default is "auto".
        let value = self.html_element.element.get_attribute("fetchpriority");
        match value.as_deref() {
            Some("high") => String::from("high"),
            Some("low") => String::from("low"),
            _ => String::from("auto"),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-img-fetchpriority>
    pub(crate) fn set_fetch_priority(&self, value: &str) {
        // Step 1: "Set this's fetchpriority content attribute to the given value."
        self.html_element
            .element
            .set_attribute("fetchpriority", value);
    }

    /// <https://html.spec.whatwg.org/#dom-img-decode>
    pub(crate) fn decode(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
        // Step 1: "Let promise be a new promise."
        // Step 2: "Queue a microtask to perform the following steps:"
        // Note: The image's decoded data is already synchronously present on
        // the element (blitz decodes on load), so the queued microtask
        // resolves or rejects without waiting. Steps 2.1-2.3 (fully active
        // check, in-parallel decode wait, animation-frame retention) are not
        // modeled.
        // Step 3: "Return promise."
        if self.natural_dimensions().is_some() {
            let promise = crate::webidl::resolved_promise(ec.value_undefined(), ec)?;
            Ok(<crate::js::Types as JsTypes>::value_from_object(promise))
        } else {
            let exception = create_interface_instance::<crate::js::Types, DOMException>(
                DOMException::new(
                    String::from("The source image cannot be decoded."),
                    String::from("EncodingError"),
                ),
                ec,
            )?;
            let reason = <crate::js::Types as JsTypes>::value_from_object(exception);
            let promise = rejected_promise(reason, ec)?;
            Ok(<crate::js::Types as JsTypes>::value_from_object(promise))
        }
    }
}
