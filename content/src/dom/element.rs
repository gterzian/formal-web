use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{LocalName, QualName, ns};

use super::Node;

/// <https://dom.spec.whatwg.org/#interface-element>
#[derive(Trace, Finalize, JsData)]
pub struct Element {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,
}

impl Element {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            node: Node::new(document, node_id),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-element-id>
    pub(crate) fn id(&self) -> String {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.attr(blitz_dom::local_name!("id")))
            .unwrap_or_default()
            .to_owned()
    }

    /// <https://dom.spec.whatwg.org/#dom-element-tagname>
    pub(crate) fn tag_name(&self) -> String {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .map(|element| element.name.local.to_string().to_ascii_uppercase())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-element-innerhtml>
    pub(crate) fn inner_html(&self) -> String {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .map(|node| {
                node.children
                    .iter()
                    .map(|child_id| document.tree()[*child_id].outer_html())
                    .collect::<String>()
            })
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#dom-element-innerhtml>
    pub(crate) fn set_inner_html(&self, html: &str) {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.set_inner_html(self.node.node_id, html);
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattribute>
    pub(crate) fn get_attribute(&self, qualified_name: &str) -> Option<String> {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.attr(LocalName::from(qualified_name)))
            .map(ToOwned::to_owned)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattribute>
    pub(crate) fn set_attribute(&self, qualified_name: &str, value: &str) {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.set_attribute(
            self.node.node_id,
            QualName::new(None, ns!(html), LocalName::from(qualified_name)),
            value,
        );
    }
}