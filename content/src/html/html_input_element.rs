use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;

use crate::html::HTMLElement;
use crate::js::Types;
use js_engine::{ExecutionContext, gc_struct};

/// <https://html.spec.whatwg.org/#the-input-element>
#[gc_struct]
pub struct HTMLInputElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLInputElement {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id, ec),
        }
    }

    /// <https://html.spec.whatwg.org/#dom-input-type>
    pub(crate) fn type_(&self) -> String {
        // <https://html.spec.whatwg.org/#reflecting-content-attributes-in-idl-attributes>
        // Step 1: Let element be the result of running this's get the element.
        //         (The reflected target is the input element; element is `self`.)
        // Step 2: Let contentAttributeValue be the result of running this's
        //         get the content attribute.
        let content_attribute_value = self.html_element.element.get_attribute("type");

        // Step 3: Let attributeDefinition be the attribute definition of
        //         element's content attribute whose namespace is null and
        //         local name is the reflected content attribute name.
        //         (The type attribute is an enumerated attribute with the
        //         keywords listed in INPUT_TYPE_KEYWORDS.)
        // Step 4: If attributeDefinition indicates it is an enumerated
        //         attribute and the reflected IDL attribute is defined to be
        //         limited to only known values:
        // Step 4.1: If contentAttributeValue does not correspond to any state
        //           of attributeDefinition (e.g., it is null and there is no
        //           missing value default), or if it is in a state of
        //           attributeDefinition with no associated keyword value,
        //           then return the empty string.
        //         (The type attribute's missing value default and invalid
        //         value default are both the Text state, so every
        //         contentAttributeValue corresponds to a state and this
        //         return is never taken.)
        // Step 4.2: Return the canonical keyword for the state of
        //           attributeDefinition that contentAttributeValue
        //           corresponds to.
        //         (The missing value default and invalid value default are
        //         both the Text state, whose canonical keyword is "text".)
        match content_attribute_value.as_deref() {
            None => String::from("text"),
            Some(value) => {
                let lower = value.to_ascii_lowercase();
                if INPUT_TYPE_KEYWORDS.contains(&lower.as_str()) {
                    lower
                } else {
                    String::from("text")
                }
            }
        }
    }

    /// <https://html.spec.whatwg.org/#dom-input-type>
    pub(crate) fn set_type(&self, value: &str) {
        // Step 1: The setter steps are to run this's set the content attribute
        //         with the given value.
        self.html_element
            .element
            .set_an_attribute_value("type", value, None, None);
    }

    /// <https://html.spec.whatwg.org/#dom-input-value>
    pub(crate) fn value(&self) -> String {
        // Step 1: Return the element's current value.
        //
        // Blitz stores the user-typed text in TextInputData on the DOM
        // node.  Read from there first so that JS sees what the user
        // actually typed.  Fall back to the value content attribute when
        // there is no text-input state (e.g. before the first keystroke).
        let document = self.html_element.element.node.document.borrow();
        let node_id = self.html_element.element.node.node_id;
        document
            .get_node(node_id)
            .and_then(|node| node.element_data())
            .and_then(|element| element.text_input_data())
            .map(|input_data| input_data.editor.raw_text().to_string())
            .or_else(|| self.html_element.element.get_attribute("value"))
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-input-value>
    pub(crate) fn set_value(&self, value: &str) {
        // Step 1: Set the element's current value to the given value.
        let sanitized = value_to_string(value);

        // Update the content attribute.  Blitz's attribute mutation
        // handler (mutator.rs) picks this up and syncs TextInputData.
        if sanitized.is_empty() {
            self.html_element
                .element
                .remove_an_attribute_by_name("value");
        } else {
            self.html_element
                .element
                .set_an_attribute_value("value", &sanitized, None, None);
        }
    }

    /// <https://html.spec.whatwg.org/#concept-input-value-stringification>
    #[allow(dead_code)]
    pub(crate) fn update_current_value(&self, _text: &str) {
        // The actual current value lives in Blitz's TextInputData on the
        // DOM node.  The attribute-set path (set_value) already triggers
        // Blitz's mutator to sync the editor, so this hook is a no-op
        // until we wire up per-keystroke input-event integration.
    }
}

/// The keywords of the `type` attribute's enumerated states, from the input
/// element's type attribute table.
const INPUT_TYPE_KEYWORDS: &[&str] = &[
    "hidden",
    "text",
    "search",
    "tel",
    "url",
    "email",
    "password",
    "date",
    "month",
    "week",
    "time",
    "datetime-local",
    "number",
    "range",
    "color",
    "checkbox",
    "radio",
    "file",
    "submit",
    "image",
    "reset",
    "button",
];

/// <https://html.spec.whatwg.org/#value-sanitization-algorithm>
fn value_to_string(value: &str) -> String {
    // For type=text (the default), the value sanitization algorithm is the
    // identity — strip newlines per spec step "strip newlines from value".
    value.replace(['\n', '\r'], "")
}
