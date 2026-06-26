use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Node as BlitzNode};
use boa_engine::{object::JsObject, Context, JsError, JsNativeError, JsResult};
use html5ever::{local_name, ns};

use crate::dom::{Element, Node};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLVideoElement, Window,
};
use crate::webidl::bindings::create_interface_instance;

pub(crate) fn with_global_scope<R>(
    context: &Context,
    f: impl FnOnce(&GlobalScope) -> JsResult<R>,
) -> JsResult<R> {
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsError::from(JsNativeError::typ().with_message("global object is not a Window"))
    })?;
    f(&window.global_scope)
}

pub(crate) fn document_object(context: &Context) -> JsResult<JsObject> {
    with_global_scope(context, |global_scope| {
        global_scope.document_object().ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message("missing document object"))
        })
    })
}

pub(crate) fn store_document_object(context: &Context, object: JsObject) -> JsResult<()> {
    with_global_scope(context, |global_scope| {
        global_scope.store_document_object(object);
        Ok(())
    })
}

pub(crate) fn location_object(context: &Context) -> JsResult<Option<JsObject>> {
    with_global_scope(context, |global_scope| Ok(global_scope.location_object()))
}

pub(crate) fn store_location_object(context: &Context, object: JsObject) -> JsResult<()> {
    with_global_scope(context, |global_scope| {
        global_scope.store_location_object(object);
        Ok(())
    })
}

fn collect_node_subtree_ids(document: &BaseDocument, node_id: usize, node_ids: &mut Vec<usize>) {
    let Some(node) = document.get_node(node_id) else {
        return;
    };

    node_ids.push(node_id);
    for child_id in node.children.iter().copied() {
        collect_node_subtree_ids(document, child_id, node_ids);
    }
}

pub(crate) fn collect_child_subtree_node_ids(
    document: &Rc<RefCell<BaseDocument>>,
    parent_node_id: usize,
) -> Vec<usize> {
    let document = document.borrow();
    let Some(parent) = document.get_node(parent_node_id) else {
        return Vec::new();
    };

    let mut node_ids = Vec::new();
    for child_id in parent.children.iter().copied() {
        collect_node_subtree_ids(&document, child_id, &mut node_ids);
    }
    node_ids
}

pub(crate) fn invalidate_cached_node_ids(context: &Context, node_ids: &[usize]) -> JsResult<()> {
    with_global_scope(context, |global_scope| {
        global_scope.invalidate_cached_node_ids(node_ids);
        Ok(())
    })
}

pub(crate) fn take_animation_frame_callbacks(
    context: &Context,
) -> JsResult<Vec<crate::webidl::Callback>> {
    with_global_scope(context, |global_scope| {
        Ok(global_scope.take_animation_frame_callbacks())
    })
}

fn cached_node_object(context: &Context, node_id: usize) -> JsResult<Option<JsObject>> {
    with_global_scope(context, |global_scope| {
        Ok(global_scope.cached_node_object(node_id))
    })
}

fn cache_node_object(context: &Context, node_id: usize, object: JsObject) -> JsResult<()> {
    with_global_scope(context, |global_scope| {
        global_scope.cache_node_object(node_id, object);
        Ok(())
    })
}

pub(crate) fn resolve_element_object(node_id: usize, context: &mut Context) -> JsResult<JsObject> {
    if let Some(object) = cached_node_object(context, node_id)? {
        return Ok(object);
    }

    let document = with_global_scope(context, |global_scope| Ok(global_scope.document()))?;
    let object = {
        let kind = document
            .borrow()
            .get_node(node_id)
            .and_then(|node| node.element_data())
            .map(|element| {
                if element.name.ns == ns!(html) {
                    if element.name.local == local_name!("video") {
                        4_u8
                    } else if element.name.local == local_name!("a") {
                        2_u8
                    } else if element.name.local == local_name!("iframe") {
                        3_u8
                    } else if element.name.local == local_name!("input") {
                        5_u8
                    } else {
                        1_u8
                    }
                } else {
                    0_u8
                }
            })
            .unwrap_or(0);

        match kind {
            5 => create_interface_instance::<HTMLInputElement>(
                HTMLInputElement::new(document, node_id),
                context,
            )?,
            4 => create_interface_instance::<HTMLVideoElement>(
                HTMLVideoElement::new(document, node_id),
                context,
            )?,
            3 => create_interface_instance::<HTMLIFrameElement>(
                HTMLIFrameElement::new(document, node_id),
                context,
            )?,
            2 => create_interface_instance::<HTMLAnchorElement>(
                HTMLAnchorElement::new(document, node_id),
                context,
            )?,
            1 => create_interface_instance::<HTMLElement>(
                HTMLElement::new(document, node_id),
                context,
            )?,
            _ => create_interface_instance::<Element>(Element::new(document, node_id), context)?,
        }
    };
    cache_node_object(context, node_id, object.clone())?;
    Ok(object)
}

pub(crate) fn resolve_or_create_text_node_object(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    if let Some(object) = cached_node_object(context, node_id)? {
        return Ok(object);
    }

    let object = create_interface_instance::<Node>(Node::new(document, node_id), context)?;
    cache_node_object(context, node_id, object.clone())?;
    Ok(object)
}

pub(crate) fn object_for_existing_node(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    if let Some(object) = cached_node_object(context, node_id)? {
        return Ok(object);
    }

    let is_element = document
        .borrow()
        .get_node(node_id)
        .is_some_and(BlitzNode::is_element);
    if is_element {
        resolve_element_object(node_id, context)
    } else {
        resolve_or_create_text_node_object(document, node_id, context)
    }
}
