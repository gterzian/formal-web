mod document;
mod element;
mod event;
mod event_target;
mod node;
mod ui_event;
mod window;

use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::Class,
    object::{JsObject, builtins::JsFunction},
};

use crate::dom::{AT_TARGET, BUBBLING_PHASE, CAPTURING_PHASE, Document, Element, Node, Window};

use super::runtime_data::{CachedNodeObject, RuntimeData};

pub(crate) use document::install_document_property;
pub(crate) use event::with_event_mut;
pub(crate) use event_target::{with_event_target_mut, with_event_target_ref};
pub(crate) fn runtime_data(context: &Context) -> JsResult<&RuntimeData> {
    context
        .get_data::<RuntimeData>()
        .ok_or_else(|| JsNativeError::typ().with_message("missing runtime data").into())
}

pub(crate) fn document_object(context: &Context) -> JsResult<JsObject> {
    runtime_data(context)?
        .document_object
        .borrow()
        .clone()
        .ok_or_else(|| JsNativeError::typ().with_message("missing document object").into())
}

pub(crate) fn store_document_object(context: &Context, object: JsObject) -> JsResult<()> {
    runtime_data(context)?.document_object.borrow_mut().replace(object);
    Ok(())
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

fn object_for_existing_node(
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
        .is_some_and(blitz_dom::Node::is_element);
    if is_element {
        resolve_element_object(node_id, context)
    } else {
        resolve_or_create_text_node_object(document, node_id, context)
    }
}

pub(crate) fn dispatch(target: &JsObject, event: &JsObject, context: &mut Context) -> JsResult<bool> {
    let path = if target.downcast_ref::<Window>().is_some() {
        vec![context.global_object()]
    } else if target.downcast_ref::<Document>().is_some() {
        vec![document_object(context)?, context.global_object()]
    } else if let Some(element) = target.downcast_ref::<Element>() {
        path_for_node(Rc::clone(&element.node.document), element.node.node_id, target.clone(), context)?
    } else if let Some(node) = target.downcast_ref::<Node>() {
        path_for_node(Rc::clone(&node.document), node.node_id, target.clone(), context)?
    } else {
        vec![target.clone()]
    };
    dispatch_along_path(&path, event, context)
}

pub(crate) fn dispatch_with_chain(
    chain: &[usize],
    event: &JsObject,
    context: &mut Context,
) -> JsResult<bool> {
    if chain.is_empty() {
        return dispatch_along_path(&[document_object(context)?, context.global_object()], event, context);
    }
    let mut path = Vec::with_capacity(chain.len() + 2);
    for node_id in chain {
        path.push(resolve_element_object(*node_id, context)?);
    }
    path.push(document_object(context)?);
    path.push(context.global_object());
    dispatch_along_path(&path, event, context)
}

fn path_for_node(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    target: JsObject,
    context: &mut Context,
) -> JsResult<Vec<JsObject>> {
    let mut path = vec![target];
    let chain = document.borrow().node_chain(node_id);
    for ancestor_id in chain.into_iter().skip(1) {
        path.push(object_for_existing_node(Rc::clone(&document), ancestor_id, context)?);
    }
    path.push(document_object(context)?);
    path.push(context.global_object());
    Ok(path)
}

fn dispatch_along_path(
    path: &[JsObject],
    event: &JsObject,
    context: &mut Context,
) -> JsResult<bool> {
    let Some(target) = path.first() else {
        return Ok(false);
    };

    {
        let event_value = JsValue::from(event.clone());
        with_event_mut(&event_value, |inner| {
            inner.target = Some(target.clone());
            inner.current_target = None;
            inner.event_phase = 0;
            inner.dispatch_flag = true;
            inner.stop_propagation_flag = false;
            inner.stop_immediate_propagation_flag = false;
        })?;
    }

    for current_target in path.iter().rev().skip(1) {
        set_event_target_state(event, Some(current_target.clone()), CAPTURING_PHASE)?;
        invoke(current_target, event, true, context)?;
        if stop_propagation(event)? {
            break;
        }
    }

    set_event_target_state(event, Some(target.clone()), AT_TARGET)?;
    invoke(target, event, true, context)?;
    if !stop_immediate(event)? {
        invoke(target, event, false, context)?;
    }

    let should_bubble = bubbles(event)?;
    if should_bubble && !stop_propagation(event)? {
        for current_target in path.iter().skip(1) {
            set_event_target_state(event, Some(current_target.clone()), BUBBLING_PHASE)?;
            invoke(current_target, event, false, context)?;
            if stop_propagation(event)? {
                break;
            }
        }
    }

    let canceled = canceled(event)?;
    set_event_target_state(event, None, 0)?;
    {
        let event_value = JsValue::from(event.clone());
        with_event_mut(&event_value, |inner| {
            inner.dispatch_flag = false;
            inner.stop_immediate_propagation_flag = false;
        })?;
    }
    Ok(canceled)
}

fn invoke(
    target: &JsObject,
    event: &JsObject,
    capture: bool,
    context: &mut Context,
) -> JsResult<()> {
    let listeners = with_event_target_ref(target, |event_target| event_target.event_listener_list.clone())?;
    for listener in listeners {
        if listener.removed || listener.capture != capture {
            continue;
        }
        let Some(callback) = listener.callback.clone() else {
            continue;
        };
        let callback: JsFunction = callback;
        {
            let event_value = JsValue::from(event.clone());
            with_event_mut(&event_value, |inner| {
                inner.in_passive_listener_flag = listener.passive == Some(true);
            })?;
        }
        if let Err(error) = callback.call(&JsValue::from(target.clone()), &[JsValue::from(event.clone())], context) {
            eprintln!("uncaught event listener error: {error}");
        }
        {
            let event_value = JsValue::from(event.clone());
            with_event_mut(&event_value, |inner| {
                inner.in_passive_listener_flag = false;
            })?;
        }
        if listener.once {
            with_event_target_mut(&JsValue::from(target.clone()), |event_target| {
                event_target.event_listener_list.retain(|existing| {
                    existing.type_ != listener.type_
                        || existing.capture != listener.capture
                        || !existing.callback.as_ref().is_some_and(|callback| {
                            listener.callback.as_ref().is_some_and(|listener_callback| {
                                boa_engine::object::JsObject::equals(callback, listener_callback)
                            })
                        })
                });
            })?;
        }
        if stop_immediate(event)? {
            break;
        }
    }
    Ok(())
}

fn set_event_target_state(
    event: &JsObject,
    current_target: Option<JsObject>,
    phase: u16,
) -> JsResult<()> {
    let event_value = JsValue::from(event.clone());
    with_event_mut(&event_value, |inner| {
        inner.current_target = current_target;
        inner.event_phase = phase;
    })
}

fn stop_propagation(event: &JsObject) -> JsResult<bool> {
    let event_value = JsValue::from(event.clone());
    with_event_mut(&event_value, |inner| inner.stop_propagation_flag)
}

fn stop_immediate(event: &JsObject) -> JsResult<bool> {
    let event_value = JsValue::from(event.clone());
    with_event_mut(&event_value, |inner| inner.stop_immediate_propagation_flag)
}

fn bubbles(event: &JsObject) -> JsResult<bool> {
    let event_value = JsValue::from(event.clone());
    with_event_mut(&event_value, |inner| inner.bubbles)
}

fn canceled(event: &JsObject) -> JsResult<bool> {
    let event_value = JsValue::from(event.clone());
    with_event_mut(&event_value, |inner| inner.canceled_flag)
}