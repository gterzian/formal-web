use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, NodeData};
use boa_engine::JsData;
use boa_gc::{Finalize, Trace};

use super::event::EventTarget;

/// <https://dom.spec.whatwg.org/#interface-node>
#[derive(Trace, Finalize, JsData)]
pub struct Node {
    /// <https://dom.spec.whatwg.org/#concept-node-document>
    #[unsafe_ignore_trace]
    pub document: Rc<RefCell<BaseDocument>>,

    /// <https://dom.spec.whatwg.org/#interface-node>
    #[unsafe_ignore_trace]
    pub node_id: usize,

    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    pub event_target: EventTarget,
}

impl Node {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            document,
            node_id,
            event_target: EventTarget::default(),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-appendchild>
    pub(crate) fn append_child(&self, child: &Node) -> Result<(), String> {
        if self.node_id == 0 {
            return Err(String::from(
                "appendChild cannot append to a detached Document wrapper",
            ));
        }

        if child.node_id == 0 {
            return Err(String::from("appendChild cannot append a Document node"));
        }

        if !Rc::ptr_eq(&self.document, &child.document) {
            return Err(String::from(
                "appendChild requires nodes from the same document",
            ));
        }

        let mut document = self.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.append_children(self.node_id, &[child.node_id]);
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-node-textcontent>
    pub(crate) fn text_content(&self) -> Option<String> {
        // Step 1: "Return the result of running get text content with this."
        self.get_text_content()
    }

    /// <https://dom.spec.whatwg.org/#get-text-content>
    pub(crate) fn get_text_content(&self) -> Option<String> {
        let document = self.document.borrow();
        let Some(node) = document.get_node(self.node_id) else {
            // Step 4: "Null."
            // Note: Removed nodes are dropped from BaseDocument, so stale wrappers fall back to null instead of panicking.
            return None;
        };

        if matches!(node.data, NodeData::Element(_)) {
            // Step 1: "The descendant text content of node."
            // Note: BaseDocument does not currently materialize DocumentFragment wrappers in the JS runtime, so this reachable branch covers Element nodes.
            return Some(node.text_content());
        }

        // Step 2: "node’s value."
        // Note: The current runtime does not materialize Attr nodes, so there is no corresponding branch here.

        if let Some(text) = node.text_data() {
            // Step 3: "node’s data."
            // Note: The current runtime currently materializes Text nodes as CharacterData. Comment and processing-instruction data are not retained separately.
            return Some(text.content.clone());
        }

        // Step 4: "Null."
        None
    }

    /// <https://dom.spec.whatwg.org/#dom-node-textcontent>
    pub(crate) fn set_text_content(&self, value: Option<&str>) {
        // Step 1: "If the given value is null, act as if it was the empty string instead, and then run set text content with this and the given value."
        let normalized_value = value.unwrap_or("");

        self.run_set_text_content(normalized_value);
    }

    /// <https://dom.spec.whatwg.org/#set-text-content>
    pub(crate) fn run_set_text_content(&self, value: &str) {
        let node_data = {
            let document = self.document.borrow();
            let Some(node) = document.get_node(self.node_id) else {
                // Note: Removed nodes are dropped from BaseDocument, so stale wrappers have no backing node to mutate.
                return;
            };

            node.data.clone()
        };

        if matches!(node_data, NodeData::Element(_)) {
            // Step 1: "String replace all with value within node."
            // Note: BaseDocument does not currently materialize DocumentFragment wrappers in the JS runtime, so this reachable branch covers Element nodes.
            string_replace_all(&self.document, self.node_id, value);
            return;
        }

        // Step 2: "Set an existing attribute value with node and value."
        // Note: The current runtime does not materialize Attr nodes, so there is no corresponding branch here.

        if matches!(node_data, NodeData::Text(_)) {
            // Step 3: "Replace data of node with 0, node’s length, and value."
            // Note: DocumentMutator applies the full replacement in one operation rather than exposing a character-range API.
            let mut document = self.document.borrow_mut();
            let mut mutator = document.mutate();
            mutator.set_node_text(self.node_id, value);
            return;
        }

        // Step 4: "Do nothing."
    }
}

/// <https://dom.spec.whatwg.org/#string-replace-all>
fn string_replace_all(document: &Rc<RefCell<BaseDocument>>, parent_node_id: usize, string: &str) {
    let mut document = document.borrow_mut();
    if document.get_node(parent_node_id).is_none() {
        // Note: Removed nodes are dropped from BaseDocument, so there is no parent to mutate.
        return;
    }

    // Step 1: "Let node be null."
    let mut replacement_node_id = None;

    // Step 2: "If string is not the empty string, then set node to a new Text node whose data is string and node document is parent’s node document."
    let mut mutator = document.mutate();
    if !string.is_empty() {
        replacement_node_id = Some(mutator.create_text_node(string));
    }

    // Step 3: "Replace all with node within parent."
    // Note: DocumentMutator exposes this as clearing the current children and then appending the replacement node when present.
    mutator.remove_and_drop_all_children(parent_node_id);
    if let Some(replacement_node_id) = replacement_node_id {
        mutator.append_children(parent_node_id, &[replacement_node_id]);
    }
}
