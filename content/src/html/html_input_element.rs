use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::html::HTMLElement;

/// <https://html.spec.whatwg.org/#the-input-element>
#[derive(Trace, Finalize, JsData)]
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
        // For the initial implementation, read the value content attribute.
        self.html_element
            .element
            .get_attribute("value")
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-input-value>
    pub(crate) fn set_value(&self, value: &str) {
        // Step 1: Set the element's current value to the given value.
        // For the initial implementation, store in the value content attribute.
        if value.is_empty() {
            self.html_element.element.remove_attribute("value");
        } else {
            self.html_element
                .element
                .set_attribute("value", value);
        }
    }
}
