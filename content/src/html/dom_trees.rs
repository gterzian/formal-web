//! <https://html.spec.whatwg.org/#dom-trees>

use blitz_dom::{ElementData, MutationHooks};
use html5ever::{local_name, ns};

use crate::html::document_providers::DocumentProviders;
use crate::html::images_processing_model;

/// Whether the element is in the HTML namespace: every HTML element step is
/// defined for a local name within it.
fn is_html_element(element: &ElementData) -> bool {
    element.name.ns == ns!(html)
}

impl MutationHooks for DocumentProviders {
    /// <https://dom.spec.whatwg.org/#concept-element-attributes-change-ext>
    fn attribute_change_steps(
        &self,
        node_id: usize,
        element: &ElementData,
        local_name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        namespace: &str,
    ) {
        if is_html_element(element) && element.name.local.as_ref() == "img" {
            images_processing_model::attribute_change_steps(
                self, node_id, element, local_name, old_value, new_value, namespace,
            );
        }
        if is_html_element(element) && element.name.local.as_ref() == "iframe" {
            // <https://html.spec.whatwg.org/#the-iframe-element>
            // "Whenever an iframe element with a non-null content navigable has its
            // srcdoc attribute set, changed, or removed, the user agent must process
            // the iframe attributes."
            // "Similarly, whenever an iframe element with a non-null content
            // navigable but with no srcdoc attribute specified has its src attribute
            // set, changed, or removed, the user agent must process the iframe
            // attributes."
            // Note: "process the iframe attributes" needs the content process's
            // document and navigable state, which a DOM hook cannot reach while
            // blitz holds the document mutably borrowed, so the hook records the
            // step and the content process runs it when it has the document back
            // (`ContentProcess::drain_pending_iframe_steps`), before the current
            // operation ends.  The recorded step re-reads the element's current
            // attributes, so a burst of changes processes once; the
            // content-navigable guard and the missing-`srcdoc` guard for `src`
            // are re-applied there.
            let relevant = namespace.is_empty()
                && (local_name == "srcdoc"
                    || (local_name == "src" && element.attr(local_name!("srcdoc")).is_none()));
            if relevant {
                self.record_iframe_attribute_change_steps(node_id);
            }
        }
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-insert-ext>
    fn insertion_steps(&self, node_id: usize, element: Option<&ElementData>) {
        // Step 1: "If insertedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element insertion steps for insertedNode's local name, then
        // run the corresponding HTML element insertion steps given insertedNode."
        if let Some(inserted_node) = element
            && is_html_element(inserted_node)
            && inserted_node.name.local.as_ref() == "img"
        {
            images_processing_model::insertion_steps(self, node_id, inserted_node);
        }
        // Step 2: "If insertedNode is a form-associated element or the ancestor of a
        // form-associated element:"
        // Step 2.1: "If the form-associated element's parser inserted flag is set, then return."
        // Step 2.2: "Reset the form owner of the form-associated element."
        // Note: Form-associated elements are not implemented.
        // Step 3: "If insertedNode is an Element that is not on the stack of open elements of an
        // HTML parser, then process internal resource links given insertedNode's node document."
        // Note: The stack of open elements is parser state that is not available here, and
        // internal resource links are not implemented.
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-post-connection-ext>
    fn post_connection_steps(&self, node_id: usize, element: Option<&ElementData>) {
        // Step 1: "If insertedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element post-connection steps for insertedNode's local name, then
        // run the corresponding HTML element post-connection steps given insertedNode."
        if let Some(inserted_node) = element
            && is_html_element(inserted_node)
            && inserted_node.name.local.as_ref() == "iframe"
        {
            // Note: The iframe post-connection steps create a child navigable, which
            // needs the content process's document and navigable state; a DOM hook
            // cannot reach it while blitz holds the document mutably borrowed, so
            // the hook records the step and the content process runs it when it has
            // the document back (`ContentProcess::drain_pending_iframe_steps`),
            // before the current operation ends.
            self.record_iframe_post_connection_steps(node_id);
        }
    }

    /// <https://html.spec.whatwg.org/#dom-trees:concept-node-remove-ext>
    fn removing_steps(
        &self,
        node_id: usize,
        element: Option<&ElementData>,
        is_subtree_root: bool,
        old_ancestor: Option<&ElementData>,
    ) {
        // Step 1: "Let document be removedNode's node document."
        // Step 2: "If document's focused area is removedNode, then set document's focused area to
        // document's viewport, and set document's relevant global object's navigation API's focus
        // changed during ongoing navigation to false."
        // Note: Focus tracking is not implemented, and the document is not reachable from this
        // hook while blitz holds it mutably borrowed.
        // Step 3: "If removedNode is an element whose namespace is the HTML namespace, and this
        // standard defines HTML element removing steps for removedNode's local name, then run the
        // corresponding HTML element removing steps given removedNode, isSubtreeRoot, and
        // oldAncestor."
        if let Some(removed_node) = element
            && is_html_element(removed_node)
            && removed_node.name.local.as_ref() == "img"
        {
            images_processing_model::removing_steps(
                self,
                node_id,
                removed_node,
                is_subtree_root,
                old_ancestor,
            );
        }
        if let Some(removed_node) = element
            && is_html_element(removed_node)
            && removed_node.name.local.as_ref() == "iframe"
        {
            self.record_iframe_removing_steps(node_id);
        }
        // Step 4: "If removedNode is a form-associated element with a non-null form owner and
        // removedNode and its form owner are no longer in the same tree, then reset the form
        // owner of removedNode."
        // Step 5: "If removedNode's popover attribute is not in the No Popover state, then run
        // the hide popover algorithm given removedNode, false, false, false, and null."
        // Note: Form-associated elements and popovers are not implemented.
    }
}
