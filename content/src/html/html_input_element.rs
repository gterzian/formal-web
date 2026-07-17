use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;

use crate::html::HTMLElement;
use js_engine::gc_struct;

/// <https://html.spec.whatwg.org/#the-input-element>
#[gc_struct]
pub struct HTMLInputElement {
    /// <https://html.spec.whatwg.org/#htmlelement>
    pub html_element: HTMLElement,
}

impl HTMLInputElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            html_element: HTMLElement::new(document, node_id),
        }
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
            self.html_element.element.remove_attribute("value");
        } else {
            self.html_element.element.set_attribute("value", &sanitized);
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

/// <https://html.spec.whatwg.org/#value-sanitization-algorithm>
fn value_to_string(value: &str) -> String {
    // For type=text (the default), the value sanitization algorithm is the
    // identity — strip newlines per spec step "strip newlines from value".
    value.replace('\n', "").replace('\r', "")
}
