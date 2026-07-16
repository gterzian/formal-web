use js_engine::gc_struct;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, NodeData};

use super::{DOMException, event::{EventTarget, EventTargetAccess}};

#[derive(Clone, Copy, Eq, PartialEq)]
enum NodeKind {
    Document,
    Element,
    Text,
    Comment,
}

/// <https://dom.spec.whatwg.org/#interface-node>
#[gc_struct]
pub struct Node {
    /// <https://dom.spec.whatwg.org/#interface-eventtarget>
    /// First field per interface inheritance convention: Node : EventTarget.
    pub event_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#concept-node-document>
    #[ignore_trace]
    pub document: Rc<RefCell<BaseDocument>>,

    /// <https://dom.spec.whatwg.org/#interface-node>
    #[ignore_trace]
    pub node_id: usize,
}

impl EventTargetAccess for Node {
    fn get_event_target(&self) -> &EventTarget {
        &self.event_target
    }
}

impl Node {
    pub fn new(document: Rc<RefCell<BaseDocument>>, node_id: usize) -> Self {
        Self {
            document,
            node_id,
            event_target: EventTarget::default(),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-childnodes>
    pub(crate) fn child_node_ids(&self) -> Vec<usize> {
        // Step 1: "Return a NodeList rooted at this matching only children."
        // The binding layer materializes the returned children as an array-backed list.
        let document = self.document.borrow();
        document
            .get_node(self.node_id)
            .map(|node| node.children.clone())
            .unwrap_or_default()
    }

    /// <https://dom.spec.whatwg.org/#dom-node-firstchild>
    pub(crate) fn first_child(&self) -> Option<usize> {
        // "Return this’s first child."
        let document = self.document.borrow();
        document
            .get_node(self.node_id)
            .and_then(|node| node.children.first().copied())
    }

    /// <https://dom.spec.whatwg.org/#dom-node-lastchild>
    pub(crate) fn last_child(&self) -> Option<usize> {
        // "Return this’s last child."
        let document = self.document.borrow();
        document
            .get_node(self.node_id)
            .and_then(|node| node.children.last().copied())
    }

    /// <https://dom.spec.whatwg.org/#dom-node-parentnode>
    pub(crate) fn parent_node(&self) -> Option<usize> {
        // "Return this’s parent."
        let document = self.document.borrow();
        document.get_node(self.node_id).and_then(|node| node.parent)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-previoussibling>
    pub(crate) fn previous_sibling(&self) -> Option<usize> {
        // "Return this’s previous sibling."
        let document = self.document.borrow();
        let node = document.get_node(self.node_id)?;
        let parent_id = node.parent?;
        let parent = document.get_node(parent_id)?;
        let index = parent
            .children
            .iter()
            .position(|child_id| *child_id == self.node_id)?;
        index
            .checked_sub(1)
            .and_then(|index| parent.children.get(index).copied())
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nextsibling>
    pub(crate) fn next_sibling(&self) -> Option<usize> {
        // "Return this’s next sibling."
        let document = self.document.borrow();
        let node = document.get_node(self.node_id)?;
        let parent_id = node.parent?;
        let parent = document.get_node(parent_id)?;
        let index = parent
            .children
            .iter()
            .position(|child_id| *child_id == self.node_id)?;
        parent.children.get(index + 1).copied()
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodetype>
    pub(crate) fn node_type(&self) -> u16 {
        let document = self.document.borrow();
        let Some(node) = document.get_node(self.node_id) else {
            return 0;
        };

        match node.data {
            NodeData::Document => 9,
            // Blitz can synthesize anonymous block boxes during layout, but the JS-visible DOM
            // still treats them as element nodes so tree APIs do not expose a Blitz-only kind.
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => 1,
            NodeData::Text(_) => 3,
            NodeData::Comment => 8,
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodename>
    pub(crate) fn node_name(&self) -> String {
        let document = self.document.borrow();
        let Some(node) = document.get_node(self.node_id) else {
            return String::new();
        };

        match &node.data {
            NodeData::Document => String::from("#document"),
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => {
                // Anonymous blocks inherit their backing element name rather than surfacing a
                // compositor/layout-specific pseudo-name through the DOM.
                if element.name.ns == html5ever::ns!(html) {
                    element.name.local.to_string().to_ascii_uppercase()
                } else {
                    element.name.local.to_string()
                }
            }
            NodeData::Text(_) => String::from("#text"),
            NodeData::Comment => String::from("#comment"),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-ownerdocument>
    pub(crate) fn owner_document_node_id(&self) -> Option<usize> {
        // Step 1: "Return null, if this is a document; otherwise this's node document."
        (self.node_id != 0).then_some(0)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-haschildnodes>
    pub(crate) fn has_child_nodes(&self) -> bool {
        // Step 1: "Return true if this has children; otherwise false."
        !self.child_node_ids().is_empty()
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodevalue>
    pub(crate) fn node_value(&self) -> Option<String> {
        let document = self.document.borrow();
        let Some(node) = document.get_node(self.node_id) else {
            return None;
        };

        match &node.data {
            NodeData::Text(text) => Some(text.content.clone()),
            NodeData::Comment => Some(String::new()),
            _ => None,
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-node-nodevalue>
    pub(crate) fn set_node_value(&self, value: Option<&str>) {
        // Step 1: "If the given value is null, act as if it was the empty string instead."
        let normalized_value = value.unwrap_or("");

        let node_data = {
            let document = self.document.borrow();
            let Some(node) = document.get_node(self.node_id) else {
                return;
            };
            node.data.clone()
        };

        match node_data {
            // Step 2: "Replace data of this with 0, this's length, and the given value."
            NodeData::Text(_) => self.set_text_content(Some(normalized_value)),
            // Step 3: "Do nothing."
            NodeData::Comment
            | NodeData::Document
            | NodeData::Element(_)
            | NodeData::AnonymousBlock(_) => {}
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-childnode-remove>
    pub(crate) fn remove(&self) {
        let parent_node_id = {
            let document = self.document.borrow();
            let Some(node) = document.get_node(self.node_id) else {
                return;
            };
            // Step 1: "If this's parent is null, then return."
            let Some(parent_node_id) = node.parent else {
                return;
            };
            parent_node_id
        };

        // Step 2: "Remove this."
        let mut document = self.document.borrow_mut();
        debug_assert_eq!(
            document.get_node(self.node_id).and_then(|node| node.parent),
            Some(parent_node_id)
        );
        let mut mutator = document.mutate();
        mutator.remove_node(self.node_id);
    }

    /// <https://dom.spec.whatwg.org/#dom-node-appendchild>
    pub(crate) fn append_child(&self, child: &Node) -> Result<usize, DOMException> {
        // Step 1: "Return the result of appending node to this."
        Self::append(child, self)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-insertbefore>
    pub(crate) fn insert_before(
        &self,
        child: &Node,
        reference_child: Option<&Node>,
    ) -> Result<usize, DOMException> {
        // Step 1: "Return the result of pre-inserting node into this before child."
        Self::pre_insert(child, self, reference_child)
    }

    /// <https://dom.spec.whatwg.org/#dom-node-removechild>
    pub(crate) fn remove_child(&self, child: &Node) -> Result<(), String> {
        if !Rc::ptr_eq(&self.document, &child.document) {
            return Err(String::from(
                "removeChild requires nodes from the same document",
            ));
        }

        {
            let document = self.document.borrow();
            let child_parent = document
                .get_node(child.node_id)
                .and_then(|node| node.parent);
            if child_parent != Some(self.node_id) {
                return Err(String::from("removeChild requires a child of the receiver"));
            }
        }

        let mut document = self.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.remove_node(child.node_id);
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
            // Note: BaseDocument does not currently materialize DocumentFragment wrappers in the JavaScript engine, so this reachable branch covers Element nodes.
            return Some(node.text_content());
        }

        // Step 2: "node’s value."
        // Note: The implementation does not materialize Attr nodes, so there is no corresponding branch here.

        if let Some(text) = node.text_data() {
            // Step 3: "node’s data."
            // Text nodes are materialized as CharacterData.
            // Note: Comment and processing-instruction data are not retained separately.
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
            // Note: BaseDocument does not currently materialize DocumentFragment wrappers in the JavaScript engine, so this reachable branch covers Element nodes.
            string_replace_all(&self.document, self.node_id, value);
            return;
        }

        // Step 2: "Set an existing attribute value with node and value."
        // Note: The implementation does not materialize Attr nodes, so there is no corresponding branch here.

        if matches!(node_data, NodeData::Text(_)) {
            // Step 3: "Replace data of node with 0, node’s length, and value."
            // DocumentMutator applies the full replacement in one operation.
            let mut document = self.document.borrow_mut();
            let mut mutator = document.mutate();
            mutator.set_node_text(self.node_id, value);
            return;
        }

        // Step 4: "Do nothing."
    }

    /// <https://dom.spec.whatwg.org/#concept-node-append>
    fn append(node: &Node, parent: &Node) -> Result<usize, DOMException> {
        // Step 1: "Pre-insert node into parent before null."
        Self::pre_insert(node, parent, None)
    }

    /// <https://dom.spec.whatwg.org/#concept-node-ensure-pre-insertion-validity>
    fn ensure_pre_insertion_validity(
        node: &Node,
        parent: &Node,
        child: Option<&Node>,
    ) -> Result<(), DOMException> {
        // Step 1: "If parent is not a Document, DocumentFragment, or Element node, then throw a \"HierarchyRequestError\" DOMException."
        if !Self::is_document_or_element_node(parent) {
            return Err(DOMException::hierarchy_request_error());
        }

        // Step 2: "If node is a host-including inclusive ancestor of parent, then throw a \"HierarchyRequestError\" DOMException."
        // Note: The current DOM implementation does not yet model shadow trees, so "host-including inclusive ancestor" reduces to the inclusive ancestor relation in the light tree.
        if Self::is_inclusive_ancestor(node, parent) {
            return Err(DOMException::hierarchy_request_error());
        }

        // Step 3: "If child is non-null and its parent is not parent, then throw a \"NotFoundError\" DOMException."
        if let Some(child) = child {
            let child_parent = child.parent_node();
            if child_parent != Some(parent.node_id)
                || !Rc::ptr_eq(&child.document, &parent.document)
            {
                return Err(DOMException::not_found_error());
            }
        }

        // Step 4: "If node is not a DocumentFragment, DocumentType, Element, or CharacterData node, then throw a \"HierarchyRequestError\" DOMException."
        if !Self::is_pre_insertable_node(node) {
            return Err(DOMException::hierarchy_request_error());
        }

        // Step 5: "If either node is a Text node and parent is a document, or node is a doctype and parent is not a document, then throw a \"HierarchyRequestError\" DOMException."
        if Self::is_text_node(node) && Self::is_document_node(parent) {
            return Err(DOMException::hierarchy_request_error());
        }

        // Step 5: "If either node is a Text node and parent is a document, or node is a doctype and parent is not a document, then throw a \"HierarchyRequestError\" DOMException."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentType nodes, so the doctype branch is unreachable here.

        // Step 6: "If parent is a document, and any of the statements below, switched on the interface node implements, are true, then throw a \"HierarchyRequestError\" DOMException."
        if !Self::is_document_node(parent) {
            return Ok(());
        }

        // Step 6.1: "If node has more than one element child or has a Text node child."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentFragment nodes, so this branch is unreachable here.

        // Step 6.2: "Otherwise, if node has one element child and either parent has an element child, child is a doctype, or child is non-null and a doctype is following child."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentFragment or DocumentType nodes, so this branch is unreachable here.

        // Step 6.3: "parent has an element child, child is a doctype, or child is non-null and a doctype is following child."
        if Self::node_kind(node) == Some(NodeKind::Element)
            && Self::document_has_element_child(parent)
        {
            return Err(DOMException::hierarchy_request_error());
        }

        // Step 6.3: "parent has an element child, child is a doctype, or child is non-null and a doctype is following child."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentType nodes, so the doctype branches are unreachable here.

        // Step 6.4: "parent has a doctype child, child is non-null and an element is preceding child, or child is null and parent has an element child."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentType nodes, so this branch is unreachable here.

        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#concept-node-pre-insert>
    fn pre_insert(node: &Node, parent: &Node, child: Option<&Node>) -> Result<usize, DOMException> {
        // Step 1: "Ensure pre-insert validity of node into parent before child."
        Self::ensure_pre_insertion_validity(node, parent, child)?;

        // Step 2: "Let referenceChild be child."
        let mut reference_child_node_id = child.map(|child| child.node_id);

        // Step 3: "If referenceChild is node, then set referenceChild to node's next sibling."
        if reference_child_node_id == Some(node.node_id) {
            reference_child_node_id = node.next_sibling();
        }

        // Step 4: "Insert node into parent before referenceChild."
        Self::insert(node, parent, reference_child_node_id)?;

        // Step 5: "Return node."
        Ok(node.node_id)
    }

    /// <https://dom.spec.whatwg.org/#concept-node-insert>
    // This helper continues the single-node, same-document tree rewrite path.
    fn insert(
        node: &Node,
        parent: &Node,
        reference_child_node_id: Option<usize>,
    ) -> Result<(), DOMException> {
        // Step 1: "Let nodes be node's children, if node is a DocumentFragment node; otherwise « node »."
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentFragment nodes, so this helper always inserts « node ».

        // Step 2: "Let count be nodes's size."
        // Note: The current JavaScript-visible DOM implementation inserts one node at a time here, so count is always 1.

        // Step 3: "If count is 0, then return."
        // Note: The current JavaScript-visible DOM implementation never reaches this helper with an empty insertion set.

        // Step 4: "If node is a DocumentFragment node:"
        // Note: The current JavaScript-visible DOM implementation does not yet expose DocumentFragment nodes, so these substeps are unreachable here.

        // Step 5: "If child is non-null:"
        // Note: The implementation does not yet model live ranges.

        // Step 6: "Let previousSibling be child's previous sibling or parent's last child if child is null."
        // Blitz's mutator derives insertion position from `reference_child_node_id`.

        // Step 7: "For each node in nodes, in tree order:"
        if !Rc::ptr_eq(&node.document, &parent.document) {
            // Step 7.1: "Adopt node into parent's node document."
            // TODO: Implement https://dom.spec.whatwg.org/#concept-node-adopt across distinct `BaseDocument` instances.
            return Err(DOMException::not_supported_error());
        }

        let mut document = parent.document.borrow_mut();
        let mut mutator = document.mutate();
        match reference_child_node_id {
            Some(reference_child_node_id) => {
                // Step 7.3: "Otherwise, insert node into parent's children before child's index."
                mutator.insert_nodes_before(reference_child_node_id, &[node.node_id]);
            }
            None => {
                // Step 7.2: "If child is null, then append node to parent's children."
                mutator.append_children(parent.node_id, &[node.node_id]);
            }
        }

        // Step 7.4: "If parent is a shadow host whose shadow root's slot assignment is \"named\" and node is a slottable, then assign a slot for node."
        // Note: The current DOM implementation does not yet model shadow trees or slot assignment.

        // Step 7.5: "If parent's root is a shadow root, and parent is a slot whose assigned nodes is the empty list, then run signal a slot change for parent."
        // Note: The current DOM implementation does not yet model shadow trees or slot assignment.

        // Step 7.6: "Run assign slottables for a tree with node's root."
        // Note: The current DOM implementation does not yet model shadow trees or slot assignment.

        // Step 7.7: "For each shadow-including inclusive descendant inclusiveDescendant of node, in shadow-including tree order:"
        // HTML insertion steps and connected callbacks continue in higher-level code paths.

        // Step 8: "If suppressObservers is unset, then queue a tree mutation record for parent with nodes, « », previousSibling, and child."
        // Note: The current DOM implementation does not yet model mutation observers.

        // Step 9: "Run the children changed steps for parent."
        // Children-changed consequences resume outside this mutator adapter.

        // Step 10: "If isConnected is true, then:"
        // Post-connection work continues outside this mutator adapter.

        Ok(())
    }

    fn node_kind(node: &Node) -> Option<NodeKind> {
        let document = node.document.borrow();
        let node = document.get_node(node.node_id)?;

        Some(match &node.data {
            NodeData::Document => NodeKind::Document,
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => NodeKind::Element,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment => NodeKind::Comment,
        })
    }

    fn is_document_node(node: &Node) -> bool {
        Self::node_kind(node) == Some(NodeKind::Document)
    }

    fn is_document_or_element_node(node: &Node) -> bool {
        matches!(
            Self::node_kind(node),
            Some(NodeKind::Document) | Some(NodeKind::Element)
        )
    }

    fn is_text_node(node: &Node) -> bool {
        Self::node_kind(node) == Some(NodeKind::Text)
    }

    fn is_pre_insertable_node(node: &Node) -> bool {
        matches!(
            Self::node_kind(node),
            Some(NodeKind::Element) | Some(NodeKind::Text) | Some(NodeKind::Comment)
        )
    }

    fn is_inclusive_ancestor(node: &Node, parent: &Node) -> bool {
        if !Rc::ptr_eq(&node.document, &parent.document) {
            return false;
        }

        let document = node.document.borrow();
        let mut current = Some(parent.node_id);
        while let Some(current_node_id) = current {
            if current_node_id == node.node_id {
                return true;
            }

            current = document
                .get_node(current_node_id)
                .and_then(|current| current.parent);
        }

        false
    }

    fn document_has_element_child(parent: &Node) -> bool {
        let document = parent.document.borrow();
        let Some(parent_node) = document.get_node(parent.node_id) else {
            return false;
        };

        parent_node.children.iter().copied().any(|child_node_id| {
            document.get_node(child_node_id).is_some_and(|child| {
                matches!(
                    &child.data,
                    NodeData::Element(_) | NodeData::AnonymousBlock(_)
                )
            })
        })
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
    // DocumentMutator: clear current children, then append replacement if present.
    mutator.remove_and_drop_all_children(parent_node_id);
    if let Some(replacement_node_id) = replacement_node_id {
        mutator.append_children(parent_node_id, &[replacement_node_id]);
    }
}
