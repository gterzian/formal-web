use std::{cell::RefCell, rc::Rc};

use crate::js::Types;
use blitz_dom::BaseDocument;
use html5ever::{LocalName, Prefix, QualName, ns};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext};
use style::dom_apis::{
    MayUseInvalidation, QueryAll, QueryFirst, QuerySelectorAllResult,
    element_closest as style_element_closest, element_matches as style_element_matches,
    query_selector as style_query_selector,
};

use super::event::{EventTarget, EventTargetAccess};
use super::namespaces::{
    ValidateAndExtractContext, is_a_valid_attribute_local_name, validate_and_extract,
};
use super::{Attr, Attribute, DOMException, NamedNodeMap, Node};

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

fn attribute_from_storage(name: &QualName, value: &str) -> Attribute {
    Attribute {
        namespace: (!name.ns.is_empty()).then(|| name.ns.to_string()),
        namespace_prefix: name.prefix.as_ref().map(|prefix| prefix.to_string()),
        local_name: name.local.to_string(),
        value: value.to_owned(),
    }
}

fn storage_name(attribute: &Attribute) -> QualName {
    QualName {
        prefix: attribute.namespace_prefix.as_deref().map(Prefix::from),
        ns: attribute.namespace.as_deref().unwrap_or("").into(),
        local: LocalName::from(attribute.local_name.as_str()),
    }
}

fn collect_subtree_node_ids(document: &BaseDocument, node_id: usize, node_ids: &mut Vec<usize>) {
    let Some(node) = document.get_node(node_id) else {
        return;
    };
    node_ids.push(node_id);
    for child_id in node.children.iter().copied() {
        collect_subtree_node_ids(document, child_id, node_ids);
    }
}

/// <https://dom.spec.whatwg.org/#interface-element>
#[gc_struct]
pub struct Element {
    /// <https://dom.spec.whatwg.org/#interface-node>
    pub node: Node,

    /// <https://dom.spec.whatwg.org/#concept-element-attribute>
    // Note: blitz's attribute storage is the attribute list; this holds the
    // Attr platform object of each stored attribute once `attribute_list`
    // has materialized it.
    attribute_nodes: GcCell<Vec<Attr>>,

    /// <https://dom.spec.whatwg.org/#dom-element-attributes>
    attributes: GcCell<Option<NamedNodeMap>>,
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
            attribute_nodes: gc_cell_new(Vec::new(), ec),
            attributes: gc_cell_new(None, ec),
        }
    }

    pub(crate) fn is_same_element(&self, other: &Element) -> bool {
        self.node.node_id == other.node.node_id
            && Rc::ptr_eq(&self.node.document, &other.node.document)
    }

    /// <https://dom.spec.whatwg.org/#html-document>
    // Note: every document here is an HTML document, so the test is on the
    // element's namespace alone.
    pub(crate) fn is_in_the_html_namespace_in_an_html_document(&self) -> bool {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .is_some_and(|element| element.name.ns == ns!(html))
    }

    fn normalized_attribute_qualified_name(&self, qualified_name: &str) -> String {
        if self.is_in_the_html_namespace_in_an_html_document() {
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

    /// <https://dom.spec.whatwg.org/#dom-element-id>
    pub(crate) fn set_id(&self, value: &str) {
        // The id attribute must reflect the "id" content attribute.
        self.set_an_attribute_value("id", value, None, None);
    }

    /// <https://dom.spec.whatwg.org/#dom-element-classname>
    pub(crate) fn class_name(&self) -> String {
        // The className attribute must reflect the "class" content attribute.
        self.get_an_attribute_value(None, "class")
    }

    /// <https://dom.spec.whatwg.org/#dom-element-classname>
    pub(crate) fn set_class_name(&self, value: &str) {
        // The className attribute must reflect the "class" content attribute.
        self.set_an_attribute_value("class", value, None, None);
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

    pub(crate) fn child_subtree_node_ids(&self) -> Vec<usize> {
        let document = self.node.document.borrow();
        let Some(node) = document.get_node(self.node.node_id) else {
            return Vec::new();
        };

        let mut node_ids = Vec::new();
        for child_id in node.children.iter().copied() {
            collect_subtree_node_ids(&document, child_id, &mut node_ids);
        }
        node_ids
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

    /// <https://dom.spec.whatwg.org/#concept-element-attribute>
    pub(crate) fn attribute_records(&self) -> Vec<Attribute> {
        let document = self.node.document.borrow();
        document
            .get_node(self.node.node_id)
            .and_then(|node| node.element_data())
            .map(|element| {
                element
                    .attrs
                    .iter()
                    .map(|stored| attribute_from_storage(&stored.name, &stored.value))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn write_attribute_storage(&self, attribute: &Attribute, value: &str) {
        {
            let mut document = self.node.document.borrow_mut();
            let mut mutator = document.mutate();
            mutator.set_attribute(self.node.node_id, storage_name(attribute), value);
        }
        Self::suppress_snapshot_on_unstyled_element(&self.node.document, self.node.node_id);
    }

    fn clear_attribute_storage(&self, attribute: &Attribute) {
        {
            let mut document = self.node.document.borrow_mut();
            let mut mutator = document.mutate();
            mutator.clear_attribute(self.node.node_id, storage_name(attribute));
        }
        Self::suppress_snapshot_on_unstyled_element(&self.node.document, self.node.node_id);
    }

    fn attribute_storage_position(&self, attribute: &Attribute) -> Option<usize> {
        self.attribute_records().iter().position(|stored| {
            stored
                .has_namespace_and_local_name(attribute.namespace.as_deref(), &attribute.local_name)
        })
    }

    fn move_attribute_storage(&self, attribute: &Attribute, index: usize) {
        let name = storage_name(attribute);
        let mut document = self.node.document.borrow_mut();
        let Some(attrs) = document
            .get_node_mut(self.node.node_id)
            .and_then(|node| node.element_data_mut())
            .map(|element| &mut element.attrs)
        else {
            return;
        };
        let stored_list: &mut Vec<_> = attrs;
        let Some(position) = stored_list.iter().position(|stored| stored.name == name) else {
            return;
        };
        if position == index || index >= stored_list.len() {
            return;
        }
        let stored = stored_list.remove(position);
        stored_list.insert(index, stored);
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attribute>
    // Note: the Attr platform objects of the attributes in blitz's storage,
    // in storage order: an attribute keeps its Attr across calls, a stored
    // attribute without one gets one, and an Attr whose attribute left the
    // storage gets a null element.
    pub(crate) fn attribute_list(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<Attr>, Types> {
        let records = self.attribute_records();
        let cached: Vec<Attr> = self.attribute_nodes.borrow(ec).clone();
        let mut list = Vec::with_capacity(records.len());
        for record in &records {
            match cached.iter().find(|attr| attr.is_attribute(record)) {
                Some(attr) => list.push(attr.clone()),
                None => list.push(Attr::create_an_attribute(
                    record.clone(),
                    Some(self.clone()),
                    ec,
                )?),
            }
        }
        for attr in cached {
            if !list.iter().any(|kept| kept.ptr_eq(&attr)) {
                attr.set_element(None, ec);
            }
        }
        self.attribute_nodes.set(list.clone(), ec);
        Ok(list)
    }

    /// The Attr platform object of an attribute in this element's attribute
    /// list.
    pub(crate) fn attribute_node(
        &self,
        attribute: &Attribute,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        Ok(self
            .attribute_list(ec)?
            .into_iter()
            .find(|attr| attr.is_attribute(attribute)))
    }

    /// Sets the element of an attribute's Attr platform object to null,
    /// when one exists, while the attribute is still stored, so the Attr
    /// keeps the value it had when it was removed or replaced.
    fn detach_cached_attribute_node(
        &self,
        attribute: &Attribute,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let cached: Vec<Attr> = self.attribute_nodes.borrow(ec).clone();
        let Some(attr) = cached.iter().find(|attr| attr.is_attribute(attribute)) else {
            return;
        };
        attr.set_element(None, ec);
        let remaining: Vec<Attr> = cached
            .iter()
            .filter(|kept| !kept.ptr_eq(attr))
            .cloned()
            .collect();
        self.attribute_nodes.set(remaining, ec);
    }

    /// Makes an existing Attr platform object the one of the attribute just
    /// stored for it: the Attr joins the attribute list and its element is
    /// set to this element.
    fn adopt_attribute_node(
        &self,
        attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let mut cached: Vec<Attr> = self.attribute_nodes.borrow(ec).clone();
        cached.push(attr.clone());
        self.attribute_nodes.set(cached, ec);
        attr.set_element(Some(self.clone()), ec);
        self.attribute_list(ec)?;
        Ok(())
    }

    /// Removes an attribute found on this element and returns its Attr
    /// platform object, with its element set to null.
    pub(crate) fn remove_attribute_returning_node(
        &self,
        attribute: Option<Attribute>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        let Some(attribute) = attribute else {
            return Ok(None);
        };
        let attr = self.attribute_node(&attribute, ec)?;
        self.detach_cached_attribute_node(&attribute, ec);
        self.remove_an_attribute(&attribute);
        Ok(attr)
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-get-by-name>
    pub(crate) fn get_an_attribute_by_name(&self, qualified_name: &str) -> Option<Attribute> {
        // Step 1: "If element is in the HTML namespace and its node document is an HTML document, then set qualifiedName to qualifiedName in ASCII lowercase."
        let qualified_name = self.normalized_attribute_qualified_name(qualified_name);

        // Step 2: "Return the first attribute in element’s attribute list whose qualified name is qualifiedName; otherwise null."
        self.attribute_records()
            .into_iter()
            .find(|attribute| attribute.qualified_name() == qualified_name)
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-get-by-namespace>
    pub(crate) fn get_an_attribute_by_namespace_and_local_name(
        &self,
        namespace: Option<&str>,
        local_name: &str,
    ) -> Option<Attribute> {
        // Step 1: "If namespace is the empty string, then set it to null."
        let namespace = namespace.filter(|namespace| !namespace.is_empty());

        // Step 2: "Return the attribute in element’s attribute list whose namespace is namespace and local name is localName, if any; otherwise null."
        self.attribute_records()
            .into_iter()
            .find(|attribute| attribute.has_namespace_and_local_name(namespace, local_name))
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-get-value>
    pub(crate) fn get_an_attribute_value(
        &self,
        namespace: Option<&str>,
        local_name: &str,
    ) -> String {
        // Step 1: "Let attr be the result of getting an attribute given namespace, localName, and element."
        let attr = self.get_an_attribute_by_namespace_and_local_name(namespace, local_name);

        // Step 2: "If attr is null, then return the empty string."
        // Step 3: "Return attr’s value."
        attr.map(|attr| attr.value).unwrap_or_default()
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-set>
    pub(crate) fn set_an_attribute(
        &self,
        attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Option<Attr>, DOMException>, Types> {
        // Step 1: "Let verifiedValue be the result of calling get trusted type compliant attribute value with attr’s local name, attr’s namespace, element, and attr’s value."
        // TODO: Trusted Types is not implemented; verifiedValue is attr's value.

        // Step 2: "If attr’s element is neither null nor element, throw an "InUseAttributeError" DOMException."
        if let Some(owner) = attr.element(ec)
            && !owner.is_same_element(self)
        {
            return Ok(Err(DOMException::in_use_attribute_error()));
        }

        // Step 3: "Let oldAttr be the result of getting an attribute given attr’s namespace, attr’s local name, and element."
        let attribute = attr.attribute(ec);
        let old_attribute = self.get_an_attribute_by_namespace_and_local_name(
            attribute.namespace.as_deref(),
            &attribute.local_name,
        );
        let old_attr = match &old_attribute {
            Some(old_attribute) => self.attribute_node(old_attribute, ec)?,
            None => None,
        };

        // Step 4: "If oldAttr is attr, return attr."
        if let Some(old_attr) = &old_attr
            && old_attr.ptr_eq(attr)
        {
            return Ok(Ok(Some(attr.clone())));
        }

        // Step 5: "Set attr’s value to verifiedValue."
        // Step 6: "If oldAttr is non-null, then replace oldAttr with attr."
        if let Some(old_attr) = &old_attr {
            self.replace_an_attribute(old_attr, attr, ec)?;
        } else {
            // Step 7: "Otherwise, append attr to element."
            self.append_an_attribute(&attribute);
            self.adopt_attribute_node(attr, ec)?;
        }

        // Step 8: "Return oldAttr."
        Ok(Ok(old_attr))
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-set-value>
    pub(crate) fn set_an_attribute_value(
        &self,
        local_name: &str,
        value: &str,
        prefix: Option<&str>,
        namespace: Option<&str>,
    ) {
        let namespace = namespace.filter(|namespace| !namespace.is_empty());
        // Step 1: "Let attribute be the result of getting an attribute given namespace, localName, and element."
        let attribute = self.get_an_attribute_by_namespace_and_local_name(namespace, local_name);

        // Step 2: "If attribute is null, then append the result of creating an attribute given element’s node document, localName, namespace, prefix, and value to element, and then return."
        // Note: the attribute is stored as a record; its Attr node is created
        // by `attribute_list` when script reaches for it.
        let Some(attribute) = attribute else {
            self.append_an_attribute(&Attribute {
                namespace: namespace.map(str::to_owned),
                namespace_prefix: prefix.map(str::to_owned),
                local_name: local_name.to_owned(),
                value: value.to_owned(),
            });
            return;
        };

        // Step 3: "Change attribute to value."
        self.change_an_attribute(&attribute, value);
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-append>
    pub(crate) fn append_an_attribute(&self, attribute: &Attribute) {
        // Step 1: "Append attribute to element’s attribute list."
        self.write_attribute_storage(attribute, &attribute.value);

        // Step 2: "Set attribute’s element to element."
        // Step 3: "Set attribute’s node document to element’s node document."
        // Note: the attribute's Attr platform object gets its element when
        // `attribute_list` materializes it, or from `adopt_attribute_node`
        // when the Attr exists already; the node document is the realm's.

        // Step 4: "Handle attribute changes for attribute with element, null, and attribute’s value."
        // TODO: Not yet implemented.
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-replace>
    pub(crate) fn replace_an_attribute(
        &self,
        old_attr: &Attr,
        new_attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let old_attribute = old_attr.attribute(ec);
        let new_attribute = new_attr.attribute(ec);
        let position = self.attribute_storage_position(&old_attribute);

        // Step 1: "Let element be oldAttribute’s element."
        // Step 5: "Set oldAttribute’s element to null."
        // Note: runs before step 2 so oldAttribute keeps the value it is
        // replaced with.
        self.detach_cached_attribute_node(&old_attribute, ec);

        // Step 2: "Replace oldAttribute by newAttribute in element’s attribute list."
        self.clear_attribute_storage(&old_attribute);
        self.write_attribute_storage(&new_attribute, &new_attribute.value);
        if let Some(position) = position {
            self.move_attribute_storage(&new_attribute, position);
        }

        // Step 3: "Set newAttribute’s element to element."
        // Step 4: "Set newAttribute’s node document to element’s node document."
        self.adopt_attribute_node(new_attr, ec)?;

        // Step 6: "Handle attribute changes for oldAttribute with element, oldAttribute’s value, and newAttribute’s value."
        // TODO: Not yet implemented.
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-change>
    pub(crate) fn change_an_attribute(&self, attribute: &Attribute, value: &str) {
        // Step 1: "Let oldValue be attribute’s value."
        // Step 2: "Set attribute’s value to value."
        self.write_attribute_storage(attribute, value);

        // Step 3: "Handle attribute changes for attribute with attribute’s element, oldValue, and value."
        // TODO: Not yet implemented.
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-remove>
    pub(crate) fn remove_an_attribute(&self, attribute: &Attribute) {
        // Step 1: "Let element be attribute’s element."
        // Step 2: "Remove attribute from element’s attribute list."
        self.clear_attribute_storage(attribute);

        // Step 3: "Set attribute’s element to null."
        // Note: a caller holding the attribute's Attr platform object sets
        // its element to null before step 2 (`detach_cached_attribute_node`)
        // so the Attr keeps the removed value; otherwise `attribute_list`
        // does it when it next runs.

        // Step 4: "Handle attribute changes for attribute with element, attribute’s value, and null."
        // TODO: Not yet implemented.
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-remove-by-name>
    pub(crate) fn remove_an_attribute_by_name(&self, qualified_name: &str) -> Option<Attribute> {
        // Step 1: "Let attr be the result of getting an attribute given qualifiedName and element."
        let attr = self.get_an_attribute_by_name(qualified_name);

        // Step 2: "If attr is non-null, then remove attr."
        if let Some(attr) = &attr {
            self.remove_an_attribute(attr);
        }

        // Step 3: "Return attr."
        attr
    }

    /// <https://dom.spec.whatwg.org/#concept-element-attributes-remove-by-namespace>
    pub(crate) fn remove_an_attribute_by_namespace_and_local_name(
        &self,
        namespace: Option<&str>,
        local_name: &str,
    ) -> Option<Attribute> {
        // Step 1: "Let attr be the result of getting an attribute given namespace, localName, and element."
        let attr = self.get_an_attribute_by_namespace_and_local_name(namespace, local_name);

        // Step 2: "If attr is non-null, then remove attr."
        if let Some(attr) = &attr {
            self.remove_an_attribute(attr);
        }

        // Step 3: "Return attr."
        attr
    }

    /// <https://dom.spec.whatwg.org/#dom-element-attributes>
    pub(crate) fn attributes(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<NamedNodeMap, Types> {
        // "The attributes getter steps are to return the associated NamedNodeMap."
        if let Some(map) = self.attributes.borrow(ec).clone() {
            return Ok(map);
        }
        let map = NamedNodeMap::new(self.clone(), ec)?;
        self.attributes.set(Some(map.clone()), ec);
        Ok(map)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-hasattributes>
    pub(crate) fn has_attributes(&self) -> bool {
        // "The hasAttributes() method steps are to return false if this’s attribute list is empty; otherwise true."
        !self.attribute_records().is_empty()
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattributenames>
    pub(crate) fn get_attribute_names(&self) -> Vec<String> {
        // "The getAttributeNames() method steps are to return the qualified names of the attributes in this’s attribute list, in order; otherwise a new list."
        self.attribute_records()
            .iter()
            .map(Attribute::qualified_name)
            .collect()
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattribute>
    pub(crate) fn get_attribute(&self, qualified_name: &str) -> Option<String> {
        // Step 1: "Let attr be the result of getting an attribute given qualifiedName and this."
        let attr = self.get_an_attribute_by_name(qualified_name);

        // Step 2: "If attr is null, return null."
        // Step 3: "Return attr’s value."
        attr.map(|attr| attr.value)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattributens>
    pub(crate) fn get_attribute_ns(
        &self,
        namespace: Option<&str>,
        local_name: &str,
    ) -> Option<String> {
        // Step 1: "Let attr be the result of getting an attribute given namespace, localName, and this."
        let attr = self.get_an_attribute_by_namespace_and_local_name(namespace, local_name);

        // Step 2: "If attr is null, return null."
        // Step 3: "Return attr’s value."
        attr.map(|attr| attr.value)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattribute>
    pub(crate) fn set_attribute(
        &self,
        qualified_name: &str,
        value: &str,
    ) -> Result<(), DOMException> {
        // Step 1: "If qualifiedName is not a valid attribute local name, then throw an "InvalidCharacterError" DOMException."
        if !is_a_valid_attribute_local_name(qualified_name) {
            return Err(DOMException::invalid_character_error());
        }

        // Step 2: "If this is in the HTML namespace and its node document is an HTML document, then set qualifiedName to qualifiedName in ASCII lowercase."
        let qualified_name = self.normalized_attribute_qualified_name(qualified_name);

        // Step 3: "Let verifiedValue be the result of calling get trusted type compliant attribute value with qualifiedName, null, this, and value."
        // TODO: Trusted Types is not implemented; verifiedValue is value.

        // Step 4: "Let attribute be the first attribute in this’s attribute list whose qualified name is qualifiedName, and null otherwise."
        let attribute = self
            .attribute_records()
            .into_iter()
            .find(|attribute| attribute.qualified_name() == qualified_name);

        // Step 5: "If attribute is non-null, then change attribute to verifiedValue and return."
        if let Some(attribute) = attribute {
            self.change_an_attribute(&attribute, value);
            return Ok(());
        }

        // Step 6: "Set attribute to the result of creating an attribute given this’s node document, qualifiedName, null, null, and verifiedValue."
        let attribute = Attribute {
            namespace: None,
            namespace_prefix: None,
            local_name: qualified_name,
            value: value.to_owned(),
        };

        // Step 7: "Append attribute to this."
        self.append_an_attribute(&attribute);
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattributens>
    pub(crate) fn set_attribute_ns(
        &self,
        namespace: Option<&str>,
        qualified_name: &str,
        value: &str,
    ) -> Result<(), DOMException> {
        // Step 1: "Let (namespace, prefix, localName) be the result of validating and extracting namespace and qualifiedName given "attribute"."
        let (namespace, prefix, local_name) = validate_and_extract(
            namespace,
            qualified_name,
            ValidateAndExtractContext::Attribute,
        )?;

        // Step 2: "Let verifiedValue be the result of calling get trusted type compliant attribute value with localName, namespace, this, and value."
        // TODO: Trusted Types is not implemented; verifiedValue is value.

        // Step 3: "Set an attribute value for this using localName, verifiedValue, prefix, and namespace."
        self.set_an_attribute_value(&local_name, value, prefix.as_deref(), namespace.as_deref());
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-removeattribute>
    pub(crate) fn remove_attribute(
        &self,
        qualified_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // "The removeAttribute(qualifiedName) method steps are to remove an attribute given qualifiedName and this, and then return undefined."
        if let Some(attribute) = self.get_an_attribute_by_name(qualified_name) {
            self.detach_cached_attribute_node(&attribute, ec);
        }
        self.remove_an_attribute_by_name(qualified_name);
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-removeattributens>
    pub(crate) fn remove_attribute_ns(
        &self,
        namespace: Option<&str>,
        local_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // "The removeAttributeNS(namespace, localName) method steps are to remove an attribute given namespace, localName, and this, and then return undefined."
        if let Some(attribute) =
            self.get_an_attribute_by_namespace_and_local_name(namespace, local_name)
        {
            self.detach_cached_attribute_node(&attribute, ec);
        }
        self.remove_an_attribute_by_namespace_and_local_name(namespace, local_name);
        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#dom-element-toggleattribute>
    pub(crate) fn toggle_attribute(
        &self,
        qualified_name: &str,
        force: Option<bool>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<bool, DOMException>, Types> {
        // Step 1: "If qualifiedName is not a valid attribute local name, then throw an "InvalidCharacterError" DOMException."
        if !is_a_valid_attribute_local_name(qualified_name) {
            return Ok(Err(DOMException::invalid_character_error()));
        }

        // Step 2: "If this is in the HTML namespace and its node document is an HTML document, then set qualifiedName to qualifiedName in ASCII lowercase."
        let qualified_name = self.normalized_attribute_qualified_name(qualified_name);

        // Step 3: "Let attribute be the first attribute in this’s attribute list whose qualified name is qualifiedName, and null otherwise."
        let attribute = self
            .attribute_records()
            .into_iter()
            .find(|attribute| attribute.qualified_name() == qualified_name);

        // Step 4: "If attribute is null:"
        let Some(attribute) = attribute else {
            // Step 4.1: "If force is not given or is true, then append the result of creating an attribute given this’s node document and qualifiedName to this, and then return true."
            if force != Some(false) {
                self.append_an_attribute(&Attribute {
                    namespace: None,
                    namespace_prefix: None,
                    local_name: qualified_name,
                    value: String::new(),
                });
                return Ok(Ok(true));
            }

            // Step 4.2: "Return false."
            return Ok(Ok(false));
        };

        // Step 5: "If force is not given or is false, remove an attribute given qualifiedName and this, and then return false."
        if force != Some(true) {
            self.detach_cached_attribute_node(&attribute, ec);
            self.remove_an_attribute_by_name(&qualified_name);
            return Ok(Ok(false));
        }

        // Step 6: "Return true."
        Ok(Ok(true))
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattributenode>
    pub(crate) fn get_attribute_node(
        &self,
        qualified_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        // "The getAttributeNode(qualifiedName) method steps are to return the result of getting an attribute given qualifiedName and this."
        match self.get_an_attribute_by_name(qualified_name) {
            Some(attribute) => self.attribute_node(&attribute, ec),
            None => Ok(None),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-element-getattributenodens>
    pub(crate) fn get_attribute_node_ns(
        &self,
        namespace: Option<&str>,
        local_name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Option<Attr>, Types> {
        // "The getAttributeNodeNS(namespace, localName) method steps are to return the result of getting an attribute given namespace, localName, and this."
        match self.get_an_attribute_by_namespace_and_local_name(namespace, local_name) {
            Some(attribute) => self.attribute_node(&attribute, ec),
            None => Ok(None),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-element-setattributenode>
    pub(crate) fn set_attribute_node(
        &self,
        attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Option<Attr>, DOMException>, Types> {
        // "The setAttributeNode(attr) and setAttributeNodeNS(attr) method steps are to return the result of setting an attribute given attr and this."
        self.set_an_attribute(attr, ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-removeattributenode>
    pub(crate) fn remove_attribute_node(
        &self,
        attr: &Attr,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Result<Attr, DOMException>, Types> {
        // Step 1: "If this’s attribute list does not contain attr, then throw a "NotFoundError" DOMException."
        if !self
            .attribute_list(ec)?
            .iter()
            .any(|kept| kept.ptr_eq(attr))
        {
            return Ok(Err(DOMException::not_found_error()));
        }

        // Step 2: "Remove attr."
        let attribute = attr.attribute(ec);
        self.detach_cached_attribute_node(&attribute, ec);
        self.remove_an_attribute(&attribute);

        // Step 3: "Return attr."
        Ok(Ok(attr.clone()))
    }

    /// <https://dom.spec.whatwg.org/#dom-element-hasattribute>
    pub(crate) fn has_attribute(&self, qualified_name: &str) -> bool {
        // Step 1: "If this is in the HTML namespace and its node document is an HTML document, then set qualifiedName to qualifiedName in ASCII lowercase."
        let qualified_name = self.normalized_attribute_qualified_name(qualified_name);

        // Step 2: "Return true if this has an attribute whose qualified name is qualifiedName; otherwise false."
        self.attribute_records()
            .iter()
            .any(|attribute| attribute.qualified_name() == qualified_name)
    }

    /// <https://dom.spec.whatwg.org/#dom-element-hasattributens>
    pub(crate) fn has_attribute_ns(&self, namespace: Option<&str>, local_name: &str) -> bool {
        // Step 1: "If namespace is the empty string, then set it to null."
        let namespace = namespace.filter(|namespace| !namespace.is_empty());

        // Step 2: "Return true if this has an attribute whose namespace is namespace and local name is localName; otherwise false."
        self.attribute_records()
            .iter()
            .any(|attribute| attribute.has_namespace_and_local_name(namespace, local_name))
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
