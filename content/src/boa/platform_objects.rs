use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Node as BlitzNode};
use boa_engine::{Context, JsNativeError, JsResult, class::Class, object::JsObject};

use crate::dom::{Element, Node};

use super::runtime_data::{AnimationFrameCallback, CachedNodeObject, RuntimeData};

pub(crate) fn runtime_data(context: &Context) -> JsResult<&RuntimeData> {
    context.get_data::<RuntimeData>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("missing runtime data")
            .into()
    })
}

pub(crate) fn document_object(context: &Context) -> JsResult<JsObject> {
    runtime_data(context)?
        .document_object
        .borrow()
        .clone()
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("missing document object")
                .into()
        })
}

pub(crate) fn store_document_object(context: &Context, object: JsObject) -> JsResult<()> {
    runtime_data(context)?
        .document_object
        .borrow_mut()
        .replace(object);
    Ok(())
}

pub(crate) fn next_animation_frame_callback_handle(context: &Context) -> JsResult<u32> {
    let runtime_data = runtime_data(context)?;
    let callbacks = runtime_data.animation_frame_callbacks.borrow();
    let mut handle = runtime_data.animation_frame_callback_identifier.get();

    loop {
        handle = handle.wrapping_add(1);
        if handle == 0 {
            continue;
        }
        if callbacks.iter().all(|entry| entry.handle != handle) {
            break;
        }
    }

    drop(callbacks);
    runtime_data.animation_frame_callback_identifier.set(handle);
    Ok(handle)
}

pub(crate) fn store_animation_frame_callback(
    context: &Context,
    handle: u32,
    callback: JsObject,
) -> JsResult<()> {
    runtime_data(context)?
        .animation_frame_callbacks
        .borrow_mut()
        .push(AnimationFrameCallback { handle, callback });
    Ok(())
}

pub(crate) fn remove_animation_frame_callback(context: &Context, handle: u32) -> JsResult<()> {
    runtime_data(context)?
        .animation_frame_callbacks
        .borrow_mut()
        .retain(|entry| entry.handle != handle);
    Ok(())
}

pub(crate) fn animation_frame_callback_handles(context: &Context) -> JsResult<Vec<u32>> {
    Ok(runtime_data(context)?
        .animation_frame_callbacks
        .borrow()
        .iter()
        .map(|entry| entry.handle)
        .collect())
}

pub(crate) fn take_animation_frame_callback(
    context: &Context,
    handle: u32,
) -> JsResult<Option<JsObject>> {
    let runtime_data = runtime_data(context)?;
    let mut callbacks = runtime_data.animation_frame_callbacks.borrow_mut();
    let Some(index) = callbacks.iter().position(|entry| entry.handle == handle) else {
        return Ok(None);
    };
    Ok(Some(callbacks.remove(index).callback.clone()))
}

fn cached_node_object(context: &Context, node_id: usize) -> JsResult<Option<JsObject>> {
    Ok(runtime_data(context)?
        .node_objects
        .borrow()
        .iter()
        .find(|entry| entry.node_id == node_id)
        .map(|entry| entry.object.clone()))
}

fn cache_node_object(context: &Context, node_id: usize, object: JsObject) -> JsResult<()> {
    runtime_data(context)?
        .node_objects
        .borrow_mut()
        .push(CachedNodeObject { node_id, object });
    Ok(())
}

pub(crate) fn resolve_element_object(node_id: usize, context: &mut Context) -> JsResult<JsObject> {
    if let Some(object) = cached_node_object(context, node_id)? {
        return Ok(object);
    }

    let document = Rc::clone(&runtime_data(context)?.document);
    let object = Element::from_data(Element::new(document, node_id), context)?;
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

    let object = Node::from_data(Node::new(document, node_id), context)?;
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
