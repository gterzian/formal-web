use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};
use html5ever::{LocalName, QualName, ns};
use style::dom_apis::{
    MayUseInvalidation, QueryAll, QueryFirst, QuerySelectorAllResult,
    query_selector as style_query_selector,
};

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

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselector>
    pub(crate) fn query_selector(&self, selectors: &str) -> Result<Option<usize>, String> {
        let document = self.node.document.borrow();
        let selector_list = document
            .try_parse_selector_list(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))?;
        let Some(root_node) = document.get_node(self.node.node_id) else {
            return Ok(None);
        };

        let mut result = None;
        style_query_selector::<&blitz_dom::Node, QueryFirst>(
            root_node,
            &selector_list,
            &mut result,
            MayUseInvalidation::Yes,
        );
        Ok(result.map(|node| node.id))
    }

    /// <https://dom.spec.whatwg.org/#dom-parentnode-queryselectorall>
    pub(crate) fn query_selector_all(&self, selectors: &str) -> Result<Vec<usize>, String> {
        let document = self.node.document.borrow();
        let selector_list = document
            .try_parse_selector_list(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))?;
        let Some(root_node) = document.get_node(self.node.node_id) else {
            return Ok(Vec::new());
        };

        let mut results = QuerySelectorAllResult::new();
        style_query_selector::<&blitz_dom::Node, QueryAll>(
            root_node,
            &selector_list,
            &mut results,
            MayUseInvalidation::Yes,
        );
        Ok(results.into_iter().map(|node| node.id).collect())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-insertadjacenttext>
    pub(crate) fn insert_adjacent_text(&self, where_: &str, data: &str) -> Result<(), String> {
        let (parent_id, first_child_id) = {
            let document = self.node.document.borrow();
            let Some(node) = document.get_node(self.node.node_id) else {
                return Ok(());
            };
            (node.parent, node.children.first().copied())
        };

        // Step 1: "Let text be a new Text node whose data is data and node document is this's node document."
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        let text_node_id = mutator.create_text_node(data);

        // Step 2: "Run insert adjacent, given this, where, and text."
        match where_ {
            "beforebegin" => {
                if parent_id.is_some() {
                    mutator.insert_nodes_before(self.node.node_id, &[text_node_id]);
                }
            }
            "afterbegin" => {
                if let Some(first_child_id) = first_child_id {
                    mutator.insert_nodes_before(first_child_id, &[text_node_id]);
                } else {
                    mutator.append_children(self.node.node_id, &[text_node_id]);
                }
            }
            "beforeend" => {
                mutator.append_children(self.node.node_id, &[text_node_id]);
            }
            "afterend" => {
                if parent_id.is_some() {
                    mutator.insert_nodes_after(self.node.node_id, &[text_node_id]);
                }
            }
            _ => {
                return Err(format!(
                    "insertAdjacentText position must be one of beforebegin, afterbegin, beforeend, or afterend; got `{where_}`"
                ));
            }
        }

        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattribute>
    pub(crate) fn get_attribute(&self, qualified_name: &str) -> Option<String> {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.attr(LocalName::from(qualified_name)))
            .map(ToOwned::to_owned)
    }

    pub(crate) fn has_attribute(&self, qualified_name: &str) -> bool {
        self.get_attribute(qualified_name).is_some()
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

    pub(crate) fn remove_attribute(&self, qualified_name: &str) {
        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.clear_attribute(
            self.node.node_id,
            QualName::new(None, ns!(html), LocalName::from(qualified_name)),
        );
    }
}
