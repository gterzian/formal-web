use std::{cell::RefCell, rc::Rc};

use crate::js::Types;
use blitz_dom::BaseDocument;
use html5ever::{LocalName, Prefix, QualName, ns};
use js_engine::ExecutionContext;
use js_engine::gc_struct;
use style::dom_apis::{
    MayUseInvalidation, QueryAll, QueryFirst, QuerySelectorAllResult,
    element_closest as style_element_closest, element_matches as style_element_matches,
    query_selector as style_query_selector,
};

use super::event::{EventTarget, EventTargetAccess};
use super::{DOMException, Node};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DomRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ElementBoxMetrics {
    pub border_top: f64,
    pub border_right: f64,
    pub border_bottom: f64,
    pub border_left: f64,
    pub padding_top: f64,
    pub padding_right: f64,
    pub padding_bottom: f64,
    pub padding_left: f64,
}

fn attribute_qualified_name(name: &QualName) -> String {
    match name.prefix.as_ref() {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn split_qualified_name(qualified_name: &str) -> (Option<Prefix>, LocalName) {
    match qualified_name.split_once(':') {
        Some((prefix, local_name)) => (Some(Prefix::from(prefix)), LocalName::from(local_name)),
        None => (None, LocalName::from(qualified_name)),
    }
}

/// <https://dom.spec.whatwg.org/#interface-element>
#[gc_struct]
pub struct Element {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,
}

impl EventTargetAccess for Element {
    fn get_event_target(&self, ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.node.get_event_target(ec)
    }
}

impl Element {
    pub fn new(
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            node: Node::new(document, node_id, ec),
        }
    }

    fn uses_ascii_lowercase_attribute_names(&self) -> bool {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .is_some_and(|element| element.name.ns == ns!(html))
    }

    fn normalized_attribute_qualified_name(&self, qualified_name: &str) -> String {
        if self.uses_ascii_lowercase_attribute_names() {
            qualified_name.to_ascii_lowercase()
        } else {
            qualified_name.to_owned()
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

    /// <https://dom.spec.whatwg.org/#dom-element-classname>
    pub(crate) fn class_name(&self) -> String {
        self.get_attribute("class").unwrap_or_default()
    }

    /// <https://dom.spec.whatwg.org/#dom-element-classname>
    pub(crate) fn set_class_name(&self, value: &str) {
        self.set_attribute("class", value);
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
        let child_ids = document
            .get_node(self.node.node_id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        let mut mutator = document.mutate();
        for child_id in child_ids {
            mutator.remove_node(child_id);
        }
        mutator.set_inner_html(self.node.node_id, html);
    }

    /// <https://dom.spec.whatwg.org/#dom-element-matches>
    pub(crate) fn matches(&self, selectors: &str) -> Result<bool, String> {
        // Step 1: Let selector be the result of parse a selector from selectors.
        // Step 2: If selector is failure, then throw a "SyntaxError" DOMException.
        let document = self.node.document.borrow();
        let selector_list = document
            .try_parse_selector_list(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))?;

        // Step 3: If the result of match a selector against an element, using
        //         selector, this, and scoping root this, returns success, then
        //         return true; otherwise, return false.
        // Note: Shadow trees are not modeled; the scoping root is the element's
        // document.
        let Some(node) = document.get_node(self.node.node_id) else {
            return Ok(false);
        };
        Ok(style_element_matches::<&blitz_dom::Node>(
            &node,
            &selector_list,
            style::context::QuirksMode::NoQuirks,
        ))
    }

    /// <https://dom.spec.whatwg.org/#dom-element-closest>
    pub(crate) fn closest(&self, selectors: &str) -> Result<Option<usize>, String> {
        // Step 1: Let selector be the result of parse a selector from selectors.
        // Step 2: If selector is failure, then throw a "SyntaxError" DOMException.
        let document = self.node.document.borrow();
        let selector_list = document
            .try_parse_selector_list(selectors)
            .map_err(|error| format!("invalid selector `{selectors}`: {error:?}"))?;

        // Step 3: Let elements be this's inclusive ancestors that are elements,
        //         in reverse tree order.
        // Step 4: For each element of elements: if match a selector against an
        //         element, using selector, element, and scoping root this,
        //         returns success, return element.
        // Step 5: Return null.
        let Some(root_node) = document.get_node(self.node.node_id) else {
            return Ok(None);
        };
        let matched = style_element_closest::<&blitz_dom::Node>(
            root_node,
            &selector_list,
            style::context::QuirksMode::NoQuirks,
        );
        Ok(matched.map(|node| node.id))
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
    pub(crate) fn insert_adjacent_text(
        &self,
        where_: &str,
        data: &str,
    ) -> Result<(), DOMException> {
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
                if parent_id == Some(0) {
                    return Err(DOMException::hierarchy_request_error());
                }
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
                if parent_id == Some(0) {
                    return Err(DOMException::hierarchy_request_error());
                }
                if parent_id.is_some() {
                    mutator.insert_nodes_after(self.node.node_id, &[text_node_id]);
                }
            }
            _ => {
                return Err(DOMException::syntax_error());
            }
        }

        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattribute>
    pub(crate) fn get_attribute(&self, qualified_name: &str) -> Option<String> {
        let normalized_name = self.normalized_attribute_qualified_name(qualified_name);
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .and_then(|element| {
                element
                    .attrs
                    .iter()
                    .find(|attribute| attribute_qualified_name(&attribute.name) == normalized_name)
                    .map(|attribute| attribute.value.clone())
            })
    }

    /// <https://dom.spec.whatwg.org/#dom-element-hasattribute>
    pub(crate) fn has_attribute(&self, qualified_name: &str) -> bool {
        // Step 1: "If this is in the HTML namespace and its node document is an HTML document, then set qualifiedName to qualifiedName in ASCII lowercase."
        let normalized_name = self.normalized_attribute_qualified_name(qualified_name);

        let document = self.node.document.borrow();

        // Step 2: "Return true if this has an attribute whose qualified name is qualifiedName; otherwise false."
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .is_some_and(|element| {
                element
                    .attrs
                    .iter()
                    .any(|attribute| attribute_qualified_name(&attribute.name) == normalized_name)
            })
    }

    /// <https://dom.spec.whatwg.org/#connected>
    pub(crate) fn is_connected(&self) -> bool {
        let document = self.node.document.borrow();
        let mut current = Some(self.node.node_id);

        while let Some(node_id) = current {
            if node_id == 0 {
                return true;
            }

            current = document.get_node(node_id).and_then(|node| node.parent);
        }

        false
    }

    /// <https://drafts.csswg.org/cssom-view/#dom-element-getboundingclientrect>
    pub(crate) fn bounding_client_rect(&self) -> Option<DomRect> {
        // Step 1 of getBoundingClientRect(): "Let list be the result of invoking
        // getClientRects() on element."
        let list = self.client_rects_for_layout_box();

        // Step 2: "If the list is empty, return a DOMRect object whose x, y, width and height
        // members are zero."
        if list.is_empty() {
            return Some(DomRect::default());
        }

        // Step 3: "If all rectangles in list have zero width or height, return the first
        // rectangle in list."
        if list
            .iter()
            .all(|rect| rect.width == 0.0 || rect.height == 0.0)
        {
            return list.first().copied();
        }

        // Step 4: "Otherwise, return a DOMRect object describing the smallest rectangle that
        // includes all of the rectangles in list of which the height or width is not zero."
        let mut non_zero_rects = list
            .into_iter()
            .filter(|rect| rect.width != 0.0 && rect.height != 0.0);
        let first_rect = non_zero_rects.next()?;
        let smallest_enclosing_rect =
            non_zero_rects.fold(first_rect, |accumulator, rect| DomRect {
                x: accumulator.x.min(rect.x),
                y: accumulator.y.min(rect.y),
                width: accumulator.right.max(rect.right) - accumulator.x.min(rect.x),
                height: accumulator.bottom.max(rect.bottom) - accumulator.y.min(rect.y),
                top: accumulator.top.min(rect.top),
                right: accumulator.right.max(rect.right),
                bottom: accumulator.bottom.max(rect.bottom),
                left: accumulator.left.min(rect.left),
            });

        Some(smallest_enclosing_rect)
    }

    fn client_rects_for_layout_box(&self) -> Vec<DomRect> {
        let document = self.node.document.borrow();
        let mut x = -document.viewport_scroll().x;
        let mut y = -document.viewport_scroll().y;
        let mut current = Some(self.node.node_id);

        while let Some(node_id) = current {
            let Some(node) = document.get_node(node_id) else {
                return Vec::new();
            };
            x += f64::from(node.final_layout.location.x) - node.scroll_offset.x;
            y += f64::from(node.final_layout.location.y) - node.scroll_offset.y;
            current = node.parent;
        }

        let Some(node) = document.get_node(self.node.node_id) else {
            return Vec::new();
        };
        let width = f64::from(node.final_layout.size.width).max(0.0);
        let height = f64::from(node.final_layout.size.height).max(0.0);
        vec![DomRect {
            x,
            y,
            width,
            height,
            top: y,
            right: x + width,
            bottom: y + height,
            left: x,
        }]
    }

    pub(crate) fn box_metrics(&self) -> Option<ElementBoxMetrics> {
        let document = self.node.document.borrow();
        let node = document.get_node(self.node.node_id)?;
        Some(ElementBoxMetrics {
            border_top: f64::from(node.final_layout.border.top),
            border_right: f64::from(node.final_layout.border.right),
            border_bottom: f64::from(node.final_layout.border.bottom),
            border_left: f64::from(node.final_layout.border.left),
            padding_top: f64::from(node.final_layout.padding.top),
            padding_right: f64::from(node.final_layout.padding.right),
            padding_bottom: f64::from(node.final_layout.padding.bottom),
            padding_left: f64::from(node.final_layout.padding.left),
        })
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattribute>
    pub(crate) fn set_attribute(&self, qualified_name: &str, value: &str) {
        let normalized_name = self.normalized_attribute_qualified_name(qualified_name);
        {
            let mut document = self.node.document.borrow_mut();
            let mut mutator = document.mutate();
            mutator.set_attribute(
                self.node.node_id,
                QualName {
                    prefix: None,
                    ns: "".into(),
                    local: LocalName::from(normalized_name.as_str()),
                },
                value,
            );
        }
        Self::suppress_snapshot_on_unstyled_element(&self.node.document, self.node.node_id);
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattributens>
    pub(crate) fn set_attribute_ns(
        &self,
        namespace: Option<&str>,
        qualified_name: &str,
        value: &str,
    ) {
        // Step 1: "Let (namespace, prefix, localName) be the result of validating and extracting namespace and qualifiedName given \"attribute\"."
        // Note: The implementation accepts the already-stringified qualified name shape used by the targeted WPTs and does not yet implement the full validation-and-extraction error surface.
        let (prefix, local_name) = split_qualified_name(qualified_name);

        {
            let mut document = self.node.document.borrow_mut();
            let mut mutator = document.mutate();

            // Step 3: "Set an attribute value for this using localName, verifiedValue, prefix, and namespace."
            mutator.set_attribute(
                self.node.node_id,
                QualName {
                    prefix,
                    ns: namespace.unwrap_or_default().into(),
                    local: local_name,
                },
                value,
            );
        }
        Self::suppress_snapshot_on_unstyled_element(&self.node.document, self.node.node_id);
    }

    /// <https://dom.spec.whatwg.org/#dom-element-removeattribute>
    pub(crate) fn remove_attribute(&self, qualified_name: &str) {
        let normalized_name = self.normalized_attribute_qualified_name(qualified_name);
        let name = {
            let document = self.node.document.borrow();
            document
                .get_node(self.node.node_id)
                .and_then(|node| node.element_data())
                .and_then(|element| {
                    element
                        .attrs
                        .iter()
                        .find(|attribute| {
                            attribute_qualified_name(&attribute.name) == normalized_name
                        })
                        .map(|attribute| attribute.name.clone())
                })
        };
        let Some(name) = name else {
            return;
        };

        let mut document = self.node.document.borrow_mut();
        let mut mutator = document.mutate();
        mutator.clear_attribute(self.node.node_id, name);
        drop(mutator);
        drop(document);
        Self::suppress_snapshot_on_unstyled_element(&self.node.document, self.node.node_id);
    }

    fn suppress_snapshot_on_unstyled_element(document: &Rc<RefCell<BaseDocument>>, node_id: usize) {
        // Note: blitz's set_attribute/clear_attribute record a stylo snapshot
        // for descendant invalidation on every attribute change, and the
        // snapshot invalidation path assumes computed styles exist: stylo's
        // ElementStyles::primary() unwraps a None primary, panicking the style
        // traversal for never-styled elements. blitz is a pinned git
        // dependency, so the content side marks the snapshot handled for
        // never-styled elements, which makes stylo skip it; the fresh element
        // is then styled from its current attributes.
        let document = document.borrow();
        let Some(node) = document.get_node(node_id) else {
            return;
        };
        if node.primary_styles().is_none() {
            node.snapshot_handled
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}
