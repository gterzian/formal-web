use std::rc::Rc;

use blitz_dom::NodeData;
use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::builtins::JsArray,
    property::Attribute,
};

use crate::boa::platform_objects::{
    collect_child_subtree_node_ids, document_object, invalidate_cached_node_ids,
    object_for_existing_node,
};
use crate::dom::{DOMException, Document, Element, Node};
use crate::html::{HTMLAnchorElement, HTMLIFrameElement, HTMLElement};

use super::event_target::register_event_target_methods;

impl Class for Node {
    const NAME: &'static str = "Node";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)?;
        register_node_methods(class)
    }
}

pub(crate) fn register_node_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    let constant_attributes = Attribute::default();
    class
        .property(js_string!("ELEMENT_NODE"), 1, constant_attributes)
        .property(js_string!("ATTRIBUTE_NODE"), 2, constant_attributes)
        .property(js_string!("TEXT_NODE"), 3, constant_attributes)
        .property(js_string!("CDATA_SECTION_NODE"), 4, constant_attributes)
        .property(js_string!("ENTITY_REFERENCE_NODE"), 5, constant_attributes)
        .property(js_string!("ENTITY_NODE"), 6, constant_attributes)
        .property(
            js_string!("PROCESSING_INSTRUCTION_NODE"),
            7,
            constant_attributes,
        )
        .property(js_string!("COMMENT_NODE"), 8, constant_attributes)
        .property(js_string!("DOCUMENT_NODE"), 9, constant_attributes)
        .property(js_string!("DOCUMENT_TYPE_NODE"), 10, constant_attributes)
        .property(js_string!("DOCUMENT_FRAGMENT_NODE"), 11, constant_attributes)
        .property(js_string!("NOTATION_NODE"), 12, constant_attributes)
        .property(
            js_string!("DOCUMENT_POSITION_DISCONNECTED"),
            0x01,
            constant_attributes,
        )
        .property(
            js_string!("DOCUMENT_POSITION_PRECEDING"),
            0x02,
            constant_attributes,
        )
        .property(
            js_string!("DOCUMENT_POSITION_FOLLOWING"),
            0x04,
            constant_attributes,
        )
        .property(
            js_string!("DOCUMENT_POSITION_CONTAINS"),
            0x08,
            constant_attributes,
        )
        .property(
            js_string!("DOCUMENT_POSITION_CONTAINED_BY"),
            0x10,
            constant_attributes,
        )
        .property(
            js_string!("DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC"),
            0x20,
            constant_attributes,
        )
        .static_property(js_string!("ELEMENT_NODE"), 1, constant_attributes)
        .static_property(js_string!("ATTRIBUTE_NODE"), 2, constant_attributes)
        .static_property(js_string!("TEXT_NODE"), 3, constant_attributes)
        .static_property(js_string!("CDATA_SECTION_NODE"), 4, constant_attributes)
        .static_property(js_string!("ENTITY_REFERENCE_NODE"), 5, constant_attributes)
        .static_property(js_string!("ENTITY_NODE"), 6, constant_attributes)
        .static_property(
            js_string!("PROCESSING_INSTRUCTION_NODE"),
            7,
            constant_attributes,
        )
        .static_property(js_string!("COMMENT_NODE"), 8, constant_attributes)
        .static_property(js_string!("DOCUMENT_NODE"), 9, constant_attributes)
        .static_property(js_string!("DOCUMENT_TYPE_NODE"), 10, constant_attributes)
        .static_property(js_string!("DOCUMENT_FRAGMENT_NODE"), 11, constant_attributes)
        .static_property(js_string!("NOTATION_NODE"), 12, constant_attributes)
        .static_property(
            js_string!("DOCUMENT_POSITION_DISCONNECTED"),
            0x01,
            constant_attributes,
        )
        .static_property(
            js_string!("DOCUMENT_POSITION_PRECEDING"),
            0x02,
            constant_attributes,
        )
        .static_property(
            js_string!("DOCUMENT_POSITION_FOLLOWING"),
            0x04,
            constant_attributes,
        )
        .static_property(
            js_string!("DOCUMENT_POSITION_CONTAINS"),
            0x08,
            constant_attributes,
        )
        .static_property(
            js_string!("DOCUMENT_POSITION_CONTAINED_BY"),
            0x10,
            constant_attributes,
        )
        .static_property(
            js_string!("DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC"),
            0x20,
            constant_attributes,
        )
        .accessor(
            js_string!("parentNode"),
            Some(NativeFunction::from_fn_ptr(get_parent_node).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("previousSibling"),
            Some(NativeFunction::from_fn_ptr(get_previous_sibling).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("nextSibling"),
            Some(NativeFunction::from_fn_ptr(get_next_sibling).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("firstChild"),
            Some(NativeFunction::from_fn_ptr(get_first_child).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("lastChild"),
            Some(NativeFunction::from_fn_ptr(get_last_child).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("textContent"),
            Some(NativeFunction::from_fn_ptr(get_text_content).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_text_content).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("nodeType"),
            Some(NativeFunction::from_fn_ptr(get_node_type).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("nodeName"),
            Some(NativeFunction::from_fn_ptr(get_node_name).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("ownerDocument"),
            Some(NativeFunction::from_fn_ptr(get_owner_document).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("nodeValue"),
            Some(NativeFunction::from_fn_ptr(get_node_value).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_node_value).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("childNodes"),
            Some(NativeFunction::from_fn_ptr(get_child_nodes).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(
            js_string!("appendChild"),
            1,
            NativeFunction::from_fn_ptr(append_child),
        )
        .method(
            js_string!("insertBefore"),
            2,
            NativeFunction::from_fn_ptr(insert_before),
        )
        .method(
            js_string!("removeChild"),
            1,
            NativeFunction::from_fn_ptr(remove_child),
        )
        .method(
            js_string!("hasChildNodes"),
            0,
            NativeFunction::from_fn_ptr(has_child_nodes),
        )
        .method(
            js_string!("remove"),
            0,
            NativeFunction::from_fn_ptr(remove),
        );
    Ok(())
}

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

fn get_text_content(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, |node| match node.text_content() {
        Some(content) => JsValue::from(JsString::from(content.as_str())),
        None => JsValue::null(),
    })
}

fn get_first_child(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_id) = with_node_ref(this, |node| (Rc::clone(&node.document), node.first_child()))?;
    match node_id {
        Some(node_id) => Ok(object_for_existing_node(document, node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_last_child(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_id) = with_node_ref(this, |node| (Rc::clone(&node.document), node.last_child()))?;
    match node_id {
        Some(node_id) => Ok(object_for_existing_node(document, node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_parent_node(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_id) = with_node_ref(this, |node| (Rc::clone(&node.document), node.parent_node()))?;
    match node_id {
        Some(0) => Ok(document_object(context)?.into()),
        Some(node_id) => Ok(object_for_existing_node(document, node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_previous_sibling(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_id) = with_node_ref(this, |node| (Rc::clone(&node.document), node.previous_sibling()))?;
    match node_id {
        Some(node_id) => Ok(object_for_existing_node(document, node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_next_sibling(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_id) = with_node_ref(this, |node| (Rc::clone(&node.document), node.next_sibling()))?;
    match node_id {
        Some(node_id) => Ok(object_for_existing_node(document, node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_child_nodes(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let (document, node_ids) = with_node_ref(this, |node| {
        (Rc::clone(&node.document), node.child_node_ids())
    })?;
    let values = node_ids
        .into_iter()
        .map(|node_id| object_for_existing_node(Rc::clone(&document), node_id, context).map(JsValue::from))
        .collect::<JsResult<Vec<_>>>()?;
    Ok(JsArray::from_iter(values, context).into())
}

fn get_node_type(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, |node| JsValue::from(node.node_type()))
}

fn get_node_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, |node| JsValue::from(JsString::from(node.node_name().as_str())))
}

fn get_owner_document(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let owner_document = with_node_ref(this, Node::owner_document_node_id)?;
    match owner_document {
        Some(0) => Ok(document_object(context)?.into()),
        Some(node_id) => {
            let document = with_node_ref(this, |node| Rc::clone(&node.document))?;
            Ok(object_for_existing_node(document, node_id, context)?.into())
        }
        None => Ok(JsValue::null()),
    }
}

fn get_node_value(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, |node| match node.node_value() {
        Some(value) => JsValue::from(JsString::from(value.as_str())),
        None => JsValue::null(),
    })
}

fn set_node_value(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args.get_or_undefined(0);
    let value = if value.is_null() {
        None
    } else {
        Some(value.to_string(context)?.to_std_string_escaped())
    };
    with_node_ref(this, |node| node.set_node_value(value.as_deref()))?;
    Ok(JsValue::undefined())
}

fn set_text_content(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args.get_or_undefined(0);
    let text = if value.is_null() {
        None
    } else {
        Some(value.to_string(context)?.to_std_string_escaped())
    };
    let dropped_node_ids = with_node_ref(this, |node| {
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
    invalidate_cached_node_ids(context, &dropped_node_ids)?;
    with_node_ref(this, |node| {
        node.set_text_content(text.as_deref());
    })?;
    Ok(JsValue::undefined())
}

fn append_child(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let child = appendable_node(args.get_or_undefined(0))?;
    with_node_ref(this, |node| node.append_child(&child))?
        .map_err(|error| dom_exception_error(error, context))?;
    Ok(args.get_or_undefined(0).clone())
}

fn insert_before(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let child = appendable_node(args.get_or_undefined(0))?;
    let reference_child = match args.get_or_undefined(1) {
        value if value.is_null() || value.is_undefined() => None,
        value => Some(appendable_node(value)?),
    };
    with_node_ref(this, |node| node.insert_before(&child, reference_child.as_ref()))?
        .map_err(|error| dom_exception_error(error, context))?;
    Ok(args.get_or_undefined(0).clone())
}

fn remove_child(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let child = appendable_node(args.get_or_undefined(0))?;
    with_node_ref(this, |node| node.remove_child(&child))?
        .map_err(|error| JsNativeError::typ().with_message(error))?;
    Ok(args.get_or_undefined(0).clone())
}

fn remove(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, Node::remove)?;
    Ok(JsValue::undefined())
}

fn has_child_nodes(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_node_ref(this, |node| JsValue::from(node.has_child_nodes()))
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

fn dom_exception_error(exception: DOMException, context: &mut Context) -> JsError {
    JsError::from_opaque(JsValue::from(
        DOMException::from_data(exception, context)
            .expect("DOMException construction should not fail"),
    ))
}
