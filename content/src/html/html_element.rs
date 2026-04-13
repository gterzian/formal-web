use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use crate::dom::{Element, Node};

/// <https://html.spec.whatwg.org/#htmlelement>
#[derive(Trace, Finalize, JsData)]
pub struct HTMLElement {
    /// <https://dom.spec.whatwg.org/#interface-element>
    pub element: Element,
}

impl HTMLElement {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            element: Element::new(document, node_id),
        }
    }

    pub(crate) fn node(&self) -> &Node {
        &self.element.node
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
        // Note: The current DOM carrier exposes the attribute as a presence bit, so both states collapse to whether `hidden` is present.
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