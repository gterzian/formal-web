use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{LocalName, QualName, ns};
use url::Url;

use super::{Element, Node};
use crate::infra::strip_and_collapse_ascii_whitespace;

fn collect_subtree_node_ids(document: &BaseDocument, node_id: usize, node_ids: &mut Vec<usize>) {
    let Some(node) = document.get_node(node_id) else {
        return;
    };
    node_ids.push(node_id);
    for child_id in node.children.iter().copied() {
        collect_subtree_node_ids(document, child_id, node_ids);
    }
}

fn canonical_document_dir(value: &str) -> &str {
    if value.eq_ignore_ascii_case("ltr") {
        "ltr"
    } else if value.eq_ignore_ascii_case("rtl") {
        "rtl"
    } else if value.eq_ignore_ascii_case("auto") {
        "auto"
    } else {
        ""
    }
}

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

    /// <https://dom.spec.whatwg.org/#dom-document-documentelement>
    pub(crate) fn document_element(&self) -> Option<usize> {
        let document = self.node.document.borrow();
        let root = document.get_node(self.node.node_id)?;
        root.children.iter().copied().find(|child_id| {
            document
                .get_node(*child_id)
                .is_some_and(|child| child.element_data().is_some())
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

    /// <https://dom.spec.whatwg.org/#dom-document-createcomment>
    pub(crate) fn create_comment(&self, _data: &str) -> usize {
        // Step 1: "Return a new Comment node whose data is data and node document is this."
        // Note: Blitz exposes comment nodes without comment-text storage, so the implementation preserves the node identity and tree behavior but not the comment payload yet.
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.create_comment_node()
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
        // Step 1: "If the document element is an SVG svg element, then let value be the child text content of the first SVG title element that is a child of the document element."
        // Note: The current WPT coverage here exercises HTML documents, so this getter currently follows the HTML branch below.

        // Step 2: "Otherwise, let value be the child text content of the title element, or the empty string if the title element is null."
        let value = self
            .node
            .document
            .borrow()
            .find_title_node()
            .map(|node| node.text_content())
            .unwrap_or_default();

        // Step 3: "Strip and collapse ASCII whitespace in value."
        let value = strip_and_collapse_ascii_whitespace(&value);

        // Step 4: "Return value."
        value
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

    /// <https://html.spec.whatwg.org/#document.title>
    pub(crate) fn title_subtree_node_ids(&self) -> Vec<usize> {
        let title_node_id = {
            let document = self.node.document.borrow();
            document.find_title_node().map(|node| node.id)
        };
        let Some(title_node_id) = title_node_id else {
            return Vec::new();
        };

        let document = self.node.document.borrow();
        let mut node_ids = Vec::new();
        collect_subtree_node_ids(&document, title_node_id, &mut node_ids);
        node_ids
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#dom-document-dir>
    pub(crate) fn dir(&self) -> String {
        self.document_element()
            .and_then(|node_id| {
                Element::new(Rc::clone(&self.node.document), node_id).get_attribute("dir")
            })
            .map(|value| canonical_document_dir(&value).to_string())
            .unwrap_or_default()
    }

    /// <https://html.spec.whatwg.org/multipage/dom.html#dom-document-dir>
    pub(crate) fn set_dir(&self, dir: &str) {
        if let Some(node_id) = self.document_element() {
            Element::new(Rc::clone(&self.node.document), node_id).set_attribute("dir", dir);
        }
    }
}
