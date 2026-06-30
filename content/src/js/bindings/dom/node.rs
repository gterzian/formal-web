use std::marker::PhantomData;
use std::rc::Rc;

use blitz_dom::NodeData;
use boa_engine::{JsArgs, JsError, JsNativeError, JsResult, JsValue, object::builtins::JsArray};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::{DOMException, Document, Element, Node};
use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement};
use crate::js::platform_objects::{
    collect_child_subtree_node_ids, document_object_ec,
    invalidate_cached_node_ids, invalidate_cached_node_ids_ec, object_for_existing_node, object_for_existing_node_ec,
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
        use boa_engine::JsValue;
        // §3.7.5: Constants
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ELEMENT_NODE",
            value: JsValue::from(1),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ATTRIBUTE_NODE",
            value: JsValue::from(2),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "TEXT_NODE",
            value: JsValue::from(3),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "CDATA_SECTION_NODE",
            value: JsValue::from(4),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ENTITY_REFERENCE_NODE",
            value: JsValue::from(5),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ENTITY_NODE",
            value: JsValue::from(6),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "PROCESSING_INSTRUCTION_NODE",
            value: JsValue::from(7),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "COMMENT_NODE",
            value: JsValue::from(8),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_NODE",
            value: JsValue::from(9),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_TYPE_NODE",
            value: JsValue::from(10),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_FRAGMENT_NODE",
            value: JsValue::from(11),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NOTATION_NODE",
            value: JsValue::from(12),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_DISCONNECTED",
            value: JsValue::from(0x01),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_PRECEDING",
            value: JsValue::from(0x02),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_FOLLOWING",
            value: JsValue::from(0x04),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_CONTAINS",
            value: JsValue::from(0x08),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_CONTAINED_BY",
            value: JsValue::from(0x10),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",
            value: JsValue::from(0x20),
        });

        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

            id: "hasChildNodes",
            length: 0,
            method: has_child_nodes,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "appendChild",
            length: 1,
            method: append_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "insertBefore",
            length: 2,
            method: insert_before,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "removeChild",
            length: 1,
            method: remove_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

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

pub(crate) fn with_node_ref<R>(this: &JsValue, f: impl FnOnce(&Node) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("node receiver is not an object"))?;
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(f(&node));
    }
    if let Some(document) = object.downcast_ref::<Document>() {
        return Ok(f(&document.node));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(f(&element.node));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(f(&html_element.element.node));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&html_anchor_element.html_element.element.node));
    }
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&html_iframe_element.html_element.element.node));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not a Node")
        .into())
}

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
    let (document, node_id) =
        try_with_node_ref(this, ec, |node| (Rc::clone(&node.document), node.first_child()))?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
    let (document, node_id) =
        try_with_node_ref(this, ec, |node| (Rc::clone(&node.document), node.last_child()))?;
    match node_id {
        Some(node_id) => {
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
    let (document, node_id) =
        try_with_node_ref(this, ec, |node| (Rc::clone(&node.document), node.parent_node()))?;
    match node_id {
        Some(0) => {
            let obj = document_object_ec(ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        Some(node_id) => {
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
    // Note: keeps ec_to_ctx — JsArray::from_iter needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let (document, node_ids) = with_node_ref(this, |node| {
        (Rc::clone(&node.document), node.child_node_ids())
    }).map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| {
            object_for_existing_node(Rc::clone(&document), node_id, ctx).map(JsValue::from)
        })
        .collect::<JsResult<Vec<_>>>()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?;
    Ok(JsArray::from_iter(values, ctx).into())
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
            let obj = document_object_ec(ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        Some(node_id) => {
            let document = try_with_node_ref(this, ec, |node| Rc::clone(&node.document))?;
            let obj = object_for_existing_node_ec(document, node_id, ec)?;
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
    let value_undefined = ec.value_undefined();
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
    let value_undefined = ec.value_undefined();
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
    invalidate_cached_node_ids_ec(ec, &dropped_node_ids)?;
    try_with_node_ref(this, ec, |node| node.set_text_content(text.as_deref()))?;
    Ok(ec.value_undefined())
}

fn append_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — appendable_node, dom_exception_error.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let child = appendable_node(args.get_or_undefined(0))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    with_node_ref(this, |node| node.append_child(&child))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .map_err(|error| dom_exception_error(error, js_engine::boa::context_as_ec(ctx)))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    Ok(args.get_or_undefined(0).clone())
}

fn insert_before(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — appendable_node, dom_exception_error.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let child = appendable_node(args.get_or_undefined(0))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let reference_child = match args.get_or_undefined(1) {
        value if value.is_null() || value.is_undefined() => None,
        value => Some(appendable_node(value)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?),
    };
    with_node_ref(this, |node| {
        node.insert_before(&child, reference_child.as_ref())
    })
    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
    .map_err(|error| dom_exception_error(error, js_engine::boa::context_as_ec(ctx)))
    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    Ok(args.get_or_undefined(0).clone())
}

fn remove_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — appendable_node needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let child = appendable_node(args.get_or_undefined(0))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    with_node_ref(this, |node| node.remove_child(&child))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .map_err(|error| JsNativeError::typ().with_message(error))
        .map_err(|e| boa_engine::JsError::from(e).into_opaque(ctx).unwrap_or(undefined))?;
    Ok(args.get_or_undefined(0).clone())
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

fn appendable_node(value: &JsValue) -> JsResult<Node> {
    let Some(object) = value.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("appendChild requires a Node")
            .into());
    };
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(Node::new(Rc::clone(&node.document), node.node_id));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(Node::new(
            Rc::clone(&element.node.document),
            element.node.node_id,
        ));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(Node::new(
            Rc::clone(&html_element.element.node.document),
            html_element.element.node.node_id,
        ));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(Node::new(
            Rc::clone(&html_anchor_element.html_element.element.node.document),
            html_anchor_element.html_element.element.node.node_id,
        ));
    }
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(Node::new(
            Rc::clone(&html_iframe_element.html_element.element.node.document),
            html_iframe_element.html_element.element.node.node_id,
        ));
    }
    Err(JsNativeError::typ()
        .with_message("appendChild requires a Node")
        .into())
}

fn dom_exception_error(
    exception: DOMException,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsError {
    JsError::from_opaque(JsValue::from(
        create_interface_instance::<crate::js::Types, DOMException>(exception, ec)
            .expect("DOMException construction should not fail"),
    ))
}
