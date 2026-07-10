use std::rc::Rc;

use blitz_dom::NodeData;

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::{DOMException, Document, Element, Node};
use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement};
use crate::js::platform_objects::{
    collect_child_subtree_node_ids, document_object, invalidate_cached_node_ids,
    object_for_existing_node,
};
use crate::webidl::bindings::{
    AttributeDef, ConstantDef, InterfaceDefinition, OperationDef, WebIdlInterface,
    create_interface_instance,
};

impl WebIdlInterface<crate::js::Types> for Node {
    const NAME: &'static str = "Node";
    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // https://dom.spec.whatwg.org/#interface-node
        // Node constants per §4.4
        def.add_constant(ConstantDef::number("ELEMENT_NODE", 1.0));
        def.add_constant(ConstantDef::number("ATTRIBUTE_NODE", 2.0));
        def.add_constant(ConstantDef::number("TEXT_NODE", 3.0));
        def.add_constant(ConstantDef::number("CDATA_SECTION_NODE", 4.0));
        def.add_constant(ConstantDef::number("ENTITY_REFERENCE_NODE", 5.0));
        def.add_constant(ConstantDef::number("ENTITY_NODE", 6.0));
        def.add_constant(ConstantDef::number("PROCESSING_INSTRUCTION_NODE", 7.0));
        def.add_constant(ConstantDef::number("COMMENT_NODE", 8.0));
        def.add_constant(ConstantDef::number("DOCUMENT_NODE", 9.0));
        def.add_constant(ConstantDef::number("DOCUMENT_TYPE_NODE", 10.0));
        def.add_constant(ConstantDef::number("DOCUMENT_FRAGMENT_NODE", 11.0));
        def.add_constant(ConstantDef::number("NOTATION_NODE", 12.0));
        def.add_constant(ConstantDef::number("DOCUMENT_POSITION_DISCONNECTED", 1.0));
        def.add_constant(ConstantDef::number("DOCUMENT_POSITION_PRECEDING", 2.0));
        def.add_constant(ConstantDef::number("DOCUMENT_POSITION_FOLLOWING", 4.0));
        def.add_constant(ConstantDef::number("DOCUMENT_POSITION_CONTAINS", 8.0));
        def.add_constant(ConstantDef::number("DOCUMENT_POSITION_CONTAINED_BY", 16.0));
        def.add_constant(ConstantDef::number(
            "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",
            32.0,
        ));
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            id: "nodeType",
            getter: get_node_type,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "ownerDocument",
            getter: get_owner_document,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "parentNode",
            getter: get_parent_node,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "childNodes",
            getter: get_child_nodes,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "firstChild",
            getter: get_first_child,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "lastChild",
            getter: get_last_child,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "previousSibling",
            getter: get_previous_sibling,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "nextSibling",
            getter: get_next_sibling,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "nodeName",
            getter: get_node_name,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "nodeValue",
            getter: get_node_value,
            setter: Some(set_node_value),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "textContent",
            getter: get_text_content,
            setter: Some(set_text_content),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });

        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "hasChildNodes",
            length: 0,
            method: has_child_nodes,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "appendChild",
            length: 1,
            method: append_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "insertBefore",
            length: 2,
            method: insert_before,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "removeChild",
            length: 1,
            method: remove_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "remove",
            length: 0,
            method: remove,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Member getters/setters/methods ──

fn try_with_node_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Node) -> R,
) -> Completion<R, crate::js::Types> {
    let object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("node receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&object) {
        if let Some(node) = data.downcast_ref::<Node>() {
            return Ok(f(node));
        }
        if let Some(document) = data.downcast_ref::<Document>() {
            return Ok(f(&document.node));
        }
        if let Some(element) = data.downcast_ref::<Element>() {
            return Ok(f(&element.node));
        }
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(f(&html_element.element.node));
        }
        if let Some(html_anchor_element) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&html_anchor_element.html_element.element.node));
        }
        if let Some(html_iframe_element) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(&html_iframe_element.html_element.element.node));
        }
    }
    Err(ec.new_type_error("receiver is not a Node"))
}

fn get_text_content(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match try_with_node_ref(this, ec, |node| node.text_content())? {
        Some(content) => Ok(ec.value_from_string(ec.js_string_from_str(content.as_str()))),
        None => Ok(ec.value_null()),
    }
}

fn get_first_child(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_id) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.first_child())
    })?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_last_child(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_id) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.last_child())
    })?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_parent_node(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_id) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.parent_node())
    })?;
    match node_id {
        Some(0) => {
            let obj = document_object(ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_previous_sibling(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_id) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.previous_sibling())
    })?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_next_sibling(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_id) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.next_sibling())
    })?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_child_nodes(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let (document, node_ids) = try_with_node_ref(this, ec, |node| {
        (Rc::clone(&node.document), node.child_node_ids())
    })?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = object_for_existing_node(Rc::clone(&document), node_id, ec)?;
        ec.array_push(&array, crate::js::Types::value_from_object(obj))?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn get_node_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let node_type = try_with_node_ref(this, ec, |node| node.node_type())?;
    Ok(ec.value_from_number(node_type as f64))
}

fn get_node_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let name = try_with_node_ref(this, ec, |node| node.node_name())?;
    Ok(ec.value_from_string(ec.js_string_from_str(name.as_str())))
}

fn get_owner_document(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let owner_document = try_with_node_ref(this, ec, Node::owner_document_node_id)?;
    match owner_document {
        Some(0) => {
            let obj = document_object(ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        Some(node_id) => {
            let document = try_with_node_ref(this, ec, |node| Rc::clone(&node.document))?;
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_node_value(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match try_with_node_ref(this, ec, |node| node.node_value())? {
        Some(value) => Ok(ec.value_from_string(ec.js_string_from_str(value.as_str()))),
        None => Ok(ec.value_null()),
    }
}

fn set_node_value(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let first = args.first();
    let value = if first.map_or(true, |v| crate::js::Types::value_is_null(v)) {
        None
    } else {
        Some(ec.to_rust_string(first.unwrap().clone())?)
    };
    try_with_node_ref(this, ec, |node| node.set_node_value(value.as_deref()))?;
    Ok(ec.value_undefined())
}

fn set_text_content(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let first = args.first();
    let text = if first.map_or(true, |v| crate::js::Types::value_is_null(v)) {
        None
    } else {
        Some(ec.to_rust_string(first.unwrap().clone())?)
    };
    let dropped_node_ids = try_with_node_ref(this, ec, |node| {
        let should_invalidate = {
            let document = node.document.borrow();
            document
                .get_node(node.node_id)
                .is_some_and(|current| matches!(current.data, NodeData::Element(_)))
        };
        if should_invalidate {
            collect_child_subtree_node_ids(&node.document, node.node_id)
        } else {
            Vec::new()
        }
    })?;
    invalidate_cached_node_ids(ec, &dropped_node_ids)?;
    try_with_node_ref(this, ec, |node| node.set_text_content(text.as_deref()))?;
    Ok(ec.value_undefined())
}

fn append_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let child_val = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined());
    let child = appendable_node(&child_val, ec)?;
    match try_with_node_ref(this, ec, |node| node.append_child(&child))? {
        Ok(_) => Ok(child_val),
        Err(dom_exception) => Err(dom_exception_error(dom_exception, ec)),
    }
}

fn insert_before(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let child_val = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined());
    let child = appendable_node(&child_val, ec)?;
    let reference_val = args.get(1).cloned().unwrap_or_else(|| ec.value_undefined());
    let reference_child = if crate::js::Types::value_is_null(&reference_val)
        || crate::js::Types::value_is_undefined(&reference_val)
    {
        None
    } else {
        Some(appendable_node(&reference_val, ec)?)
    };
    match try_with_node_ref(this, ec, |node| {
        node.insert_before(&child, reference_child.as_ref())
    })? {
        Ok(_) => Ok(child_val),
        Err(dom_exception) => Err(dom_exception_error(dom_exception, ec)),
    }
}

fn remove_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let child_val = args
        .first()
        .cloned()
        .unwrap_or_else(|| ec.value_undefined());
    let child = appendable_node(&child_val, ec)?;
    match try_with_node_ref(this, ec, |node| node.remove_child(&child))? {
        Ok(()) => Ok(child_val),
        Err(error_msg) => Err(ec.new_type_error(&error_msg)),
    }
}

fn appendable_node(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Node, crate::js::Types> {
    let object = crate::js::Types::value_as_object(value)
        .ok_or_else(|| ec.new_type_error("appendChild requires a Node"))?;
    if let Some(data) = ec.with_object_any(&object) {
        if let Some(node) = data.downcast_ref::<Node>() {
            return Ok(Node::new(Rc::clone(&node.document), node.node_id));
        }
        if let Some(element) = data.downcast_ref::<Element>() {
            return Ok(Node::new(
                Rc::clone(&element.node.document),
                element.node.node_id,
            ));
        }
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(Node::new(
                Rc::clone(&html_element.element.node.document),
                html_element.element.node.node_id,
            ));
        }
        if let Some(html_iframe_element) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(Node::new(
                Rc::clone(&html_iframe_element.html_element.element.node.document),
                html_iframe_element.html_element.element.node.node_id,
            ));
        }
    }
    Err(ec.new_type_error("appendChild requires a Node"))
}

fn remove(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    try_with_node_ref(this, ec, Node::remove)?;
    Ok(ec.value_undefined())
}

fn has_child_nodes(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let has = try_with_node_ref(this, ec, |node| node.has_child_nodes())?;
    Ok(ec.value_from_bool(has))
}

fn dom_exception_error(
    exception: DOMException,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsValue {
    create_interface_instance::<crate::js::Types, DOMException>(exception, ec)
        .map(|obj| crate::js::Types::value_from_object(obj))
        .unwrap_or_else(|err| err)
}
