use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{LocalName, QualName, ns};
use url::Url;

use super::Node;

/// <https://dom.spec.whatwg.org/#interface-document>
#[derive(Trace, Finalize, JsData)]
pub struct Document {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,

    /// Model-local mirror of <https://html.spec.whatwg.org/#concept-environment-creation-url>.
    #[unsafe_ignore_trace]
    pub creation_url: Url,
}

impl Document {
    pub fn new(document: Rc<RefCell<BaseDocument>>, creation_url: Url) -> Self {
        Self {
            node: Node::new(document, 0),
            creation_url,
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

    /// <https://dom.spec.whatwg.org/#dom-parentnode-getelementsbytagname>
    pub(crate) fn get_elements_by_tag_name(
        &self,
        qualified_name: &str,
    ) -> Result<Vec<usize>, String> {
        self.node
            .document
            .borrow()
            .query_selector_all(qualified_name)
            .map(|matches| matches.into_iter().collect())
            .map_err(|error| {
                format!("failed to resolve tag name selector `{qualified_name}`: {error:?}")
            })
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

    /// <https://dom.spec.whatwg.org/#dom-document-createelementns>
    pub(crate) fn create_element_ns(
        &self,
        namespace: Option<&str>,
        qualified_name: &str,
    ) -> Result<usize, String> {
        let namespace = match namespace {
            None | Some("") | Some("http://www.w3.org/1999/xhtml") => ns!(html),
            Some(other) => {
                return Err(format!(
                    "unsupported namespace `{other}` in createElementNS"
                ));
            }
        };

        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        Ok(mutator.create_element(
            QualName::new(None, namespace, LocalName::from(qualified_name)),
            Vec::new(),
        ))
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
        let title_node_id = self
            .node
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.id);
        if let Some(title_node_id) = title_node_id {
            Node::new(Rc::clone(&self.node.document), title_node_id).set_text_content(Some(title));
        }
    }
}
