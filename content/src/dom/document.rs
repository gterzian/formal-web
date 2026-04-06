use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{LocalName, QualName, ns};

use super::Node;

/// <https://dom.spec.whatwg.org/#interface-document>
#[derive(Trace, Finalize, JsData)]
pub struct Document {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,
}

impl Document {
    pub fn new(document: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            node: Node::new(document, 0),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-nonelementparentnode-getelementbyid>
    pub(crate) fn get_element_by_id(&self, id: &str) -> Option<usize> {
        self.node.document.borrow().get_element_by_id(id)
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselector>
    pub(crate) fn query_selector(&self, selectors: &str) -> Result<Option<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselectorall>
    pub(crate) fn query_selector_all(&self, selectors: &str) -> Result<Vec<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector_all(selectors)
            .map(|matches| matches.into_iter().collect())
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createelement>
    pub(crate) fn create_element(&self, local_name: &str) -> usize {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_element(
            QualName::new(None, ns!(html), LocalName::from(local_name)),
            Vec::new(),
        )
    }

    /// <https://dom.spec.whatwg.org/#dom-document-createtextnode>
    pub(crate) fn create_text_node(&self, data: &str) -> usize {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_text_node(data)
    }

    /// <https://html.spec.whatwg.org/#dom-document-body>
    pub(crate) fn body(&self) -> Result<Option<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector("body")
            .map_err(|error| format!("failed to resolve body selector: {error:?}"))
    }

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn title(&self) -> String {
        self.node
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.text_content())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn set_title(&self, title: &str) {
        let title_node_id = self.node.document.borrow().find_title_node().map(|node| node.id);
        if let Some(title_node_id) = title_node_id {
            Node::new(Rc::clone(&self.node.document), title_node_id).set_text_content(Some(title));
        }
    }
}