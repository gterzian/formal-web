use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use blitz_dom::BaseDocument;

use crate::dom::{Element, EventTarget};
use crate::dom::event::EventTargetAccess;
use js_engine::gc_struct;

/// <https://html.spec.whatwg.org/#htmlelement>
#[gc_struct]
pub struct HTMLElement {
    /// <https://dom.spec.whatwg.org/#interface-element>
    pub element: Element,
}

impl EventTargetAccess for HTMLElement {
    fn get_event_target(&self) -> EventTarget {
        self.element.get_event_target()
    }
}

impl HTMLElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            element: Element::new(document, node_id),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-title>
    pub(crate) fn title(&self) -> String {
        // Step 1: "Return the value of the title content attribute."
        self.element.get_attribute("title").unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-title>
    pub(crate) fn set_title(&self, title: &str) {
        // Step 1: "Set this's title content attribute to the given value."
        self.element.set_attribute("title", title);
    }

    /// <https://html.spec.whatwg.org/#dom-lang>
    pub(crate) fn lang(&self) -> String {
        // Step 1: "Return the value of the lang content attribute."
        self.element.get_attribute("lang").unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-lang>
    pub(crate) fn set_lang(&self, lang: &str) {
        // Step 1: "Set this's lang content attribute to the given value."
        self.element.set_attribute("lang", lang);
    }

    /// <https://html.spec.whatwg.org/#dom-dir>
    pub(crate) fn dir(&self) -> String {
        // Step 1: "Return the value of the dir content attribute."
        self.element.get_attribute("dir").unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-dir>
    pub(crate) fn set_dir(&self, dir: &str) {
        // Step 1: "Set this's dir content attribute to the given value."
        self.element.set_attribute("dir", dir);
    }

    /// <https://html.spec.whatwg.org/#dom-hidden>
    pub(crate) fn hidden(&self) -> bool {
        // Step 1: "Return true if the hidden attribute is in the Hidden State or Until Found State; otherwise false."
        // Note: The current DOM implementation exposes the attribute as a presence bit, so both states collapse to whether `hidden` is present.
        self.element.has_attribute("hidden")
    }

    /// <https://html.spec.whatwg.org/#dom-hidden>
    pub(crate) fn set_hidden(&self, hidden: bool) {
        // Step 1: "Set the hidden attribute to the Hidden State if the given value is true; otherwise remove it."
        if hidden {
            self.element.set_attribute("hidden", "");
        } else {
            self.element.remove_attribute("hidden");
        }
    }
}

/// <https://drafts.csswg.org/cssom/#dom-htmlelement-style>
pub(crate) fn inline_style_properties_for_element(element: &Element) -> BTreeMap<String, String> {
    // Step 1 of the ElementCSSInlineStyle getter is handled at the binding layer, which creates a
    // CSSStyleProperties-shaped object whose owner node is the element.
    let mut properties = BTreeMap::new();

    // Step 1 of "When a CSS declaration block object is created": "Let owner node be the owner
    // node."
    // Step 2: "If owner node is null, or the computed flag is set, then return."
    // Note: This helper is only called for an element-backed, non-computed declaration block.
    let Some(style_attribute) = element.get_attribute("style") else {
        return properties;
    };

    // Step 3: "Let value be the result of getting an attribute given null, \"style\", and owner
    // node."
    // Step 4: "If value is not null, let the declarations be the result of parse a CSS declaration
    // block from a string value."
    //
    // Step 1 of "parse a CSS declaration block from a string": obtain the declaration items from
    // the style attribute source text.
    // Step 2: "Let parsed declarations be a new empty list."
    // Step 3: "For each item declaration in declarations..."
    //
    // Note: The implementation does not yet route style attributes through a full CSS Syntax
    // parser. It accepts the subset used by the targeted tests by splitting declarations on `;`
    // and the first `:` while normalizing property names to ASCII lowercase.
    for declaration in style_attribute.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let normalized_name = name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() {
            continue;
        }
        properties.insert(normalized_name, value.trim().to_owned());
    }

    properties
}

/// <https://drafts.csswg.org/cssom/#resolved-values>
pub(crate) fn resolved_style_properties_for_element(element: &Element) -> BTreeMap<String, String> {
    // This helper materializes the subset of longhand resolved values that the current DOM and
    // layout implementations can source for Window.getComputedStyle().
    let mut properties = inline_style_properties_for_element(element);
    let metrics = element.box_metrics().unwrap_or_default();

    properties.insert(
        String::from("display"),
        if element.has_attribute("hidden") {
            String::from("none")
        } else {
            properties
                .get("display")
                .cloned()
                .unwrap_or_else(|| default_display_for_tag_name(&element.tag_name()).to_owned())
        },
    );

    if !properties.contains_key("visibility") {
        properties.insert(String::from("visibility"), String::from("visible"));
    }
    if !properties.contains_key("opacity") {
        properties.insert(String::from("opacity"), String::from("1"));
    }
    if !properties.contains_key("transform") {
        properties.insert(String::from("transform"), String::from("none"));
    }
    if !properties.contains_key("pointer-events") {
        properties.insert(String::from("pointer-events"), String::from("auto"));
    }
    if !properties.contains_key("position") {
        properties.insert(String::from("position"), String::from("static"));
    }
    if !properties.contains_key("white-space") {
        properties.insert(String::from("white-space"), String::from("normal"));
    }
    if !properties.contains_key("cursor") {
        properties.insert(String::from("cursor"), String::from("auto"));
    }
    if !properties.contains_key("content") {
        properties.insert(String::from("content"), String::from("none"));
    }

    if !properties.contains_key("overflow") {
        properties.insert(String::from("overflow"), String::from("visible"));
    }
    let overflow = properties
        .get("overflow")
        .cloned()
        .unwrap_or_else(|| String::from("visible"));
    if !properties.contains_key("overflow-x") {
        properties.insert(String::from("overflow-x"), overflow.clone());
    }
    if !properties.contains_key("overflow-y") {
        properties.insert(String::from("overflow-y"), overflow);
    }

    properties
        .entry(String::from("border-top-width"))
        .or_insert_with(|| format_css_px(metrics.border_top));
    properties
        .entry(String::from("border-right-width"))
        .or_insert_with(|| format_css_px(metrics.border_right));
    properties
        .entry(String::from("border-bottom-width"))
        .or_insert_with(|| format_css_px(metrics.border_bottom));
    properties
        .entry(String::from("border-left-width"))
        .or_insert_with(|| format_css_px(metrics.border_left));
    properties
        .entry(String::from("padding-top"))
        .or_insert_with(|| format_css_px(metrics.padding_top));
    properties
        .entry(String::from("padding-right"))
        .or_insert_with(|| format_css_px(metrics.padding_right));
    properties
        .entry(String::from("padding-bottom"))
        .or_insert_with(|| format_css_px(metrics.padding_bottom));
    properties
        .entry(String::from("padding-left"))
        .or_insert_with(|| format_css_px(metrics.padding_left));
    properties
        .entry(String::from("margin-top"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-right"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-bottom"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-left"))
        .or_insert_with(|| String::from("0px"));

    properties
}

fn format_css_px(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}px")
    } else {
        format!("{value}px")
    }
}

fn default_display_for_tag_name(tag_name: &str) -> &'static str {
    match tag_name {
        "BODY" | "DIV" | "FORM" | "H1" | "H2" | "H3" | "H4" | "H5" | "H6" | "HEADER" | "HTML"
        | "IFRAME" | "LI" | "MAIN" | "NAV" | "OL" | "P" | "SECTION" | "TABLE" | "UL" => "block",
        "BUTTON" => "inline-block",
        _ => "inline",
    }
}
