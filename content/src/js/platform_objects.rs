use std::any::TypeId;
use std::collections::HashSet;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Node as BlitzNode};
use html5ever::{local_name, ns};

use crate::dom::{Document, Element, EventPathItem, Node};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLVideoElement, Window,
};
use crate::js::downcast::event_target_from_js_object;
use crate::webidl::bindings::create_interface_instance;
use js_engine::{Completion, ExecutionContext, JsTypes};

/// <https://html.spec.whatwg.org/#global-object>
///
/// Type-key for storing the realm's global object `JsObject` in `host_any`.
/// Initialised during realm creation; validated in
/// `host_any_stored_object_downcast_via_with_object_any`.
pub(crate) struct GlobalObjectSlot;

/// <https://html.spec.whatwg.org/#global-object>
///
/// Store the realm's global object in `host_any`.  Call once during realm
/// initialisation, after the global object has been created.
pub(crate) fn init_global_object_slot(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    global_object: <crate::js::Types as JsTypes>::JsObject,
) {
    ec.store_host_any(TypeId::of::<GlobalObjectSlot>(), Box::new(global_object));
}

/// <https://html.spec.whatwg.org/#global-object>
///
/// Downcast the realm's global object to `&GlobalScope` through
/// `realm_global_object()` + `with_object_any`.  Returns `None` if the
/// global object is not a `Window` or has no native data.
fn global_scope_or_error<'ec>(
    ec: &'ec dyn ExecutionContext<crate::js::Types>,
) -> Option<&'ec GlobalScope> {
    let global_obj = ec.realm_global_object();
    ec.with_object_any(&global_obj)
        .and_then(|data| data.downcast_ref::<Window>())
        .map(|window| &window.global_scope)
}

/// <https://html.spec.whatwg.org/#global-object>
///
/// Like `global_scope_or_error` but constructs a `Completion` error when
/// the global object can't be reached.
pub(crate) fn with_global_scope<R>(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&GlobalScope) -> Completion<R, crate::js::Types>,
) -> Completion<R, crate::js::Types> {
    match global_scope_or_error(ec) {
        Some(gs) => f(gs),
        None => Err(ec.new_type_error("global object is not a Window")),
    }
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

pub(crate) fn document_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    let missing_err = ec.new_type_error("missing document object");
    with_global_scope(ec, |global_scope| {
        global_scope.document_object().ok_or(missing_err)
    })
}

pub(crate) fn location_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<<crate::js::Types as JsTypes>::JsObject>, crate::js::Types> {
    with_global_scope(ec, |global_scope| Ok(global_scope.location_object()))
}

pub(crate) fn store_location_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    object: <crate::js::Types as JsTypes>::JsObject,
) -> Completion<(), crate::js::Types> {
    with_global_scope(ec, |global_scope| {
        global_scope.store_location_object(object);
        Ok(())
    })
}

pub(crate) fn invalidate_cached_node_ids(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    node_ids: &[usize],
) -> Completion<(), crate::js::Types> {
    with_global_scope(ec, |global_scope| {
        global_scope.invalidate_cached_node_ids(node_ids);
        Ok(())
    })
}

pub(crate) fn take_animation_frame_callbacks(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<crate::webidl::Callback>, crate::js::Types> {
    with_global_scope(ec, |global_scope| {
        Ok(global_scope.take_animation_frame_callbacks())
    })
}

pub(crate) fn resolve_element_object(
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    // Read cache + document via immutable GlobalScope access.
    let (cached, document) = match global_scope_or_error(ec) {
        Some(gs) => (gs.cached_node_object(node_id), gs.document()),
        None => return Err(ec.new_type_error("global object is not a Window")),
    };
    if let Some(object) = cached {
        return Ok(object);
    }

    // Create platform object (mutable ec, no GlobalScope borrow active).
    let object = element_object_from_document(document, node_id, ec)?;

    // Cache the result (immutable GlobalScope access).
    if let Some(gs) = global_scope_or_error(ec) {
        gs.cache_node_object(node_id, object.clone());
    }

    Ok(object)
}

pub(crate) fn object_for_existing_node(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    let cached = match global_scope_or_error(ec) {
        Some(gs) => gs.cached_node_object(node_id),
        None => return Err(ec.new_type_error("global object is not a Window")),
    };
    if let Some(object) = cached {
        return Ok(object);
    }

    let is_element = document
        .borrow()
        .get_node(node_id)
        .is_some_and(BlitzNode::is_element);
    if is_element {
        resolve_element_object(node_id, ec)
    } else {
        resolve_or_create_text_node_object(document, node_id, ec)
    }
}

pub(crate) fn resolve_or_create_text_node_object(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    let cached = match global_scope_or_error(ec) {
        Some(gs) => gs.cached_node_object(node_id),
        None => return Err(ec.new_type_error("global object is not a Window")),
    };
    if let Some(object) = cached {
        return Ok(object);
    }

    let object =
        create_interface_instance::<crate::js::Types, Node>(Node::new(document, node_id), ec)?;

    if let Some(gs) = global_scope_or_error(ec) {
        gs.cache_node_object(node_id, object.clone());
    }

    Ok(object)
}

/// Use `try_with_event_target_mut` to set the reflector on the EventTarget
/// embedded in a platform object JsObject.
fn element_object_from_document(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
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

    let object = match kind {
        5 => create_interface_instance::<crate::js::Types, HTMLInputElement>(
            HTMLInputElement::new(document, node_id),
            ec,
        ),
        4 => create_interface_instance::<crate::js::Types, HTMLVideoElement>(
            HTMLVideoElement::new(document, node_id),
            ec,
        ),
        3 => create_interface_instance::<crate::js::Types, HTMLIFrameElement>(
            HTMLIFrameElement::new(document, node_id),
            ec,
        ),
        2 => create_interface_instance::<crate::js::Types, HTMLAnchorElement>(
            HTMLAnchorElement::new(document, node_id),
            ec,
        ),
        1 => create_interface_instance::<crate::js::Types, HTMLElement>(
            HTMLElement::new(document, node_id),
            ec,
        ),
        _ => create_interface_instance::<crate::js::Types, Element>(
            Element::new(document, node_id),
            ec,
        ),
    }?;
    Ok(object)
}

pub(crate) fn build_path_from_target_js_object(
    target_object: &<crate::js::Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Vec<EventPathItem> {
    let mut path: Vec<EventPathItem> = Vec::new();
    let node_info = ec.with_object_any(target_object).and_then(|data| {
        if let Some(element) = data.downcast_ref::<Element>() {
            Some((element.node.node_id, element.node.document.clone()))
        } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            Some((html_element.element.node.node_id, html_element.element.node.document.clone()))
        } else if let Some(node) = data.downcast_ref::<Node>() {
            Some((node.node_id, node.document.clone()))
        } else if let Some(document) = data.downcast_ref::<Document>() {
            Some((document.node.node_id, document.node.document.clone()))
        } else {
            None
        }
    });

    if let Some((node_id, document)) = node_info {
        if let Some(event_target) = event_target_from_js_object(ec, target_object) {
            path.push(EventPathItem { invocation_target: event_target.clone(), shadow_adjusted_target: Some(event_target) });
        }
        let mut current_node_id = node_id;
        let mut visited = HashSet::new();
        visited.insert(node_id);
        loop {
            let parent_id = { let doc = document.borrow(); doc.get_node(current_node_id).and_then(|n| n.parent) };
            match parent_id {
                Some(pid) if !visited.contains(&pid) => {
                    visited.insert(pid);
                    if let Ok(parent_object) = resolve_element_object(pid, ec) {
                        if let Some(parent_event_target) = event_target_from_js_object(ec, &parent_object) {
                            path.push(EventPathItem { invocation_target: parent_event_target, shadow_adjusted_target: None });
                        }
                        current_node_id = pid;
                    } else {
                        current_node_id = pid;
                    }
                }
                _ => break,
            }
        }
    } else {
        if let Some(event_target) = event_target_from_js_object(ec, target_object) {
            path.push(EventPathItem { invocation_target: event_target.clone(), shadow_adjusted_target: Some(event_target) });
        }
    }
    path
}
