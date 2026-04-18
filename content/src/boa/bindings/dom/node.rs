use std::rc::Rc;

use blitz_dom::NodeData;
use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::boa::platform_objects::{collect_child_subtree_node_ids, invalidate_cached_node_ids};
use crate::dom::{Document, Element, Node};
use crate::html::{HTMLAnchorElement, HTMLElement};

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
    class
        .accessor(
            js_string!("textContent"),
            Some(NativeFunction::from_fn_ptr(get_text_content).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_text_content).to_js_function(&realm)),
            Attribute::all(),
        )
        .method(
            js_string!("appendChild"),
            1,
            NativeFunction::from_fn_ptr(append_child),
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

fn append_child(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let child = appendable_node(args.get_or_undefined(0))?;
    with_node_ref(this, |node| node.append_child(&child))?
        .map_err(|error| JsNativeError::typ().with_message(error))?;
    Ok(args.get_or_undefined(0).clone())
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
    Err(JsNativeError::typ()
        .with_message("appendChild requires a Node")
        .into())
}
