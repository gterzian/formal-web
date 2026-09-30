use std::{cell::Cell, cell::RefCell, collections::BTreeMap, rc::Rc};

use blitz_dom::BaseDocument;

use crate::dom::event::EventTargetAccess;
use crate::dom::{Element, Event, EventTarget, dispatch_with_path};
use crate::js::Types;
use crate::js::platform_objects::{build_path_from_target_js_object, resolve_element_object};
use crate::webidl::bindings::create_interface_instance;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

/// <https://html.spec.whatwg.org/#htmlelement>
#[gc_struct]
pub struct HTMLElement {
    /// <https://dom.spec.whatwg.org/#interface-element>
    pub element: Element,

    /// <https://html.spec.whatwg.org/#click-in-progress-flag>
    #[ignore_trace]
    click_in_progress_flag: Cell<bool>,
}

impl EventTargetAccess for HTMLElement {
    fn get_event_target(&self, ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.element.get_event_target(ec)
    }
}

impl HTMLElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            element: Element::new(document, node_id, ec),
            click_in_progress_flag: Cell::new(false),
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
        self.element
            .set_an_attribute_value("title", title, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-lang>
    pub(crate) fn lang(&self) -> String {
        // Step 1: "Return the value of the lang content attribute."
        self.element.get_attribute("lang").unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-lang>
    pub(crate) fn set_lang(&self, lang: &str) {
        // Step 1: "Set this's lang content attribute to the given value."
        self.element
            .set_an_attribute_value("lang", lang, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-dir>
    pub(crate) fn dir(&self) -> String {
        // Step 1: "Return the value of the dir content attribute."
        self.element.get_attribute("dir").unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-dir>
    pub(crate) fn set_dir(&self, dir: &str) {
        // Step 1: "Set this's dir content attribute to the given value."
        self.element.set_an_attribute_value("dir", dir, None, None);
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
            self.element
                .set_an_attribute_value("hidden", "", None, None);
        } else {
            self.element.remove_an_attribute_by_name("hidden");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-click>
    pub(crate) fn click(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        // Step 1: If this element is a form control that is disabled, then return.
        // Note: Disabled form-control state is not modeled, so the check is skipped.
        // Step 2: If this element's click in progress flag is set, then return.
        if self.click_in_progress_flag.get() {
            return Ok(());
        }
        // Step 3: Set this element's click in progress flag.
        self.click_in_progress_flag.set(true);

        // Step 4: Fire a synthetic pointer event named click at this element,
        //         with the not trusted flag set.
        // <https://html.spec.whatwg.org/#fire-a-synthetic-pointer-event>
        let reflector = resolve_element_object(self.element.node.node_id, ec)?;
        fire_a_synthetic_pointer_event(&reflector, "click", true, ec)?;

        // Step 5: Unset this element's click in progress flag.
        self.click_in_progress_flag.set(false);
        Ok(())
    }
}

/// <https://html.spec.whatwg.org/#fire-a-synthetic-pointer-event>
pub(crate) fn fire_a_synthetic_pointer_event(
    target_object: &<Types as JsTypes>::JsObject,
    name: &str,
    not_trusted: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    // Step 1: Let event be the result of creating an event using PointerEvent.
    // Note: PointerEvent is not yet modeled; a plain Event with the same
    // type, bubbles, cancelable, composed, and isTrusted state is created
    // instead, and the dispatch path applies activation behavior to it.
    // Step 2: Initialize event's type attribute to e.
    // Step 3: Initialize event's bubbles and cancelable attributes to true.
    // Step 4: Set event's composed flag.
    // Step 5: If the not trusted flag is set, initialize event's isTrusted
    //         attribute to false.
    let event = Event::new(name.to_owned(), true, true, true, !not_trusted, 0.0, ec);
    let event_object = create_interface_instance::<Types, Event>(event, ec)?;
    let event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>().cloned())
        .ok_or_else(|| ec.new_type_error("event creation failed"))?;

    // Step 6: Initialize event's ctrlKey, shiftKey, altKey, and metaKey
    //         attributes according to the current state of the key input
    //         device, if any (false for any keys that are not available).
    // TODO: Not yet implemented — the plain Event exposes no modifier-key
    // properties.
    // Step 7: Initialize event's view attribute to target's node document's
    //         Window object, if any, and null otherwise.
    // TODO: Not yet implemented — the Event domain object has no view
    // attribute.
    // Step 8: event's getModifierState() method is to return values
    //         appropriately describing the current state of the key input
    //         device.
    // TODO: Not yet implemented.

    // Step 9: Return the result of dispatching event at target.
    // <https://dom.spec.whatwg.org/#concept-event-dispatch>
    let path = build_path_from_target_js_object(target_object, ec);
    dispatch_with_path(ec, &path, &event)
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
