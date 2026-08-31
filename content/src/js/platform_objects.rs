use std::any::TypeId;
use std::collections::HashSet;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Node as BlitzNode};
use html5ever::{local_name, ns};

use crate::dom::{Document, Element, EventPathItem, Node};
use crate::html::{
    ActivationBehavior, GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement,
    HTMLInputElement, HTMLMediaElement, HTMLVideoElement, Window, WorkerGlobalScope,
};
use crate::js::downcast::event_target_from_js_object;
use crate::webidl::bindings::create_interface_instance;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#global-object>
pub(crate) struct GlobalObjectSlot;

/// <https://html.spec.whatwg.org/#global-object>
pub(crate) fn init_global_object_slot(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    global_object: <crate::js::Types as JsTypes>::JsObject,
) {
    ec.store_host_any(TypeId::of::<GlobalObjectSlot>(), Box::new(global_object));
}

/// <https://html.spec.whatwg.org/#global-object>
fn global_scope_or_error(ec: &dyn ExecutionContext<crate::js::Types>) -> Option<&GlobalScope> {
    let global_obj = ec.realm_global_object();
    ec.with_object_any(&global_obj).and_then(|data| {
        if let Some(window) = data.downcast_ref::<Window>() {
            Some(&window.global_scope)
        } else if let Some(worker) = data.downcast_ref::<WorkerGlobalScope>() {
            Some(&worker.global_scope)
        } else {
            None
        }
    })
}

/// <https://html.spec.whatwg.org/#global-object>
fn worker_global_scope_or_error(
    ec: &dyn ExecutionContext<crate::js::Types>,
) -> Option<&WorkerGlobalScope> {
    let global_obj = ec.realm_global_object();
    ec.with_object_any(&global_obj)
        .and_then(|data| data.downcast_ref::<WorkerGlobalScope>())
}

/// Access the realm's worker global scope, when the global object is a
/// worker global scope.
/// <https://html.spec.whatwg.org/#global-object>
pub(crate) fn with_worker_global_scope<R>(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(
        &WorkerGlobalScope,
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<R, crate::js::Types>,
) -> Completion<R, crate::js::Types> {
    let Some(worker_global_scope) = worker_global_scope_or_error(ec) else {
        return Err(ec.new_type_error("global object is not a WorkerGlobalScope"));
    };
    let worker_global_scope = worker_global_scope.clone();
    f(&worker_global_scope, ec)
}

/// <https://html.spec.whatwg.org/#global-object>
pub(crate) fn with_global_scope<R>(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(
        &GlobalScope,
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<R, crate::js::Types>,
) -> Completion<R, crate::js::Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let Some(gs) = global_scope_or_error(ec) else {
        return Err(ec.new_type_error("global object is not a Window or WorkerGlobalScope"));
    };
    let gs = gs.clone();
    f(&gs, ec)
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
    with_global_scope(ec, |global_scope, ec| {
        global_scope.document_object(ec).ok_or(missing_err)
    })
}

pub(crate) fn invalidate_cached_node_ids(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    node_ids: &[usize],
) -> Completion<(), crate::js::Types> {
    with_global_scope(ec, |global_scope, ec| {
        global_scope.invalidate_cached_node_ids(node_ids, ec);
        Ok(())
    })
}

pub(crate) fn take_animation_frame_callbacks(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Vec<crate::webidl::Callback>, crate::js::Types> {
    with_global_scope(ec, |global_scope, ec| {
        Ok(global_scope.take_animation_frame_callbacks(ec))
    })
}

pub(crate) fn has_pending_animation_frame_callbacks(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<bool, crate::js::Types> {
    with_global_scope(ec, |global_scope, ec| {
        Ok(global_scope.has_pending_animation_frame_callbacks(ec))
    })
}

pub(crate) fn resolve_element_object(
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    // Read cache + document via immutable GlobalScope access.
    let (cached, document) = match global_scope_or_error(ec).cloned() {
        Some(gs) => (gs.cached_node_object(node_id, ec), gs.document()),
        None => return Err(ec.new_type_error("global object is not a Window")),
    };
    if let Some(object) = cached {
        return Ok(object);
    }

    // Create platform object (mutable ec, no GlobalScope borrow active).
    let object = element_object_from_document(document.clone(), node_id, ec)?;

    // Compile and activate the element's `on*` content attributes (e.g.
    // `onload="..."`) now that the platform object exists, so the handlers
    // fire on the element's events.  Parser-set attributes are covered
    // because element platform objects are created through this resolver
    // (and the iframe load event steps resolve the element here before
    // firing the load event); attributes set later via setAttribute are not
    // synced yet.
    sync_event_handler_content_attributes(&document, node_id, &object, ec)?;

    // Cache the result (immutable GlobalScope access).
    if let Some(gs) = global_scope_or_error(ec).cloned() {
        gs.cache_node_object(node_id, object.clone(), ec);
    }

    Ok(object)
}

pub(crate) fn object_for_existing_node(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<<crate::js::Types as JsTypes>::JsObject, crate::js::Types> {
    let cached = match global_scope_or_error(ec).cloned() {
        Some(gs) => gs.cached_node_object(node_id, ec),
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
    let cached = match global_scope_or_error(ec).cloned() {
        Some(gs) => gs.cached_node_object(node_id, ec),
        None => return Err(ec.new_type_error("global object is not a Window")),
    };
    if let Some(object) = cached {
        return Ok(object);
    }

    let object =
        create_interface_instance::<crate::js::Types, Node>(Node::new(document, node_id, ec), ec)?;

    if let Some(gs) = global_scope_or_error(ec).cloned() {
        gs.cache_node_object(node_id, object.clone(), ec);
    }

    Ok(object)
}

/// Filter an element's `on*` content attributes and sync each one to its
/// event handler: the namespace/name filter and the event handler target
/// resolution of the sync run here; the remaining steps run in
/// html::event_handler::sync_event_handler_content_attribute.
fn sync_event_handler_content_attributes(
    document: &Rc<RefCell<BaseDocument>>,
    node_id: usize,
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let Some(event_target) = event_target_from_js_object(ec, object) else {
        return Ok(());
    };
    // Clone the attributes out of the document borrow before compiling the
    // handlers (compilation allocates and must not run under the borrow).
    let attributes: Vec<(String, String)> = document
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.element_data())
        .map(|element| {
            element
                .attrs
                .iter()
                .filter(|attribute| attribute.name.ns.is_empty())
                .filter_map(|attribute| {
                    let event_type = attribute.name.local.as_ref().strip_prefix("on")?;
                    Some((event_type.to_owned(), attribute.value.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    for (event_type, source) in attributes {
        crate::html::event_handler::sync_event_handler_content_attribute(
            &event_target,
            &event_type,
            Some(&source),
            ec,
        )?;
    }
    Ok(())
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
            HTMLInputElement::new(document, node_id, ec),
            ec,
        ),
        4 => create_interface_instance::<crate::js::Types, HTMLVideoElement>(
            HTMLVideoElement::new(document, node_id, ec),
            ec,
        ),
        3 => create_interface_instance::<crate::js::Types, HTMLIFrameElement>(
            HTMLIFrameElement::new(document, node_id, ec),
            ec,
        ),
        2 => create_interface_instance::<crate::js::Types, HTMLAnchorElement>(
            HTMLAnchorElement::new(document, node_id, ec),
            ec,
        ),
        1 => create_interface_instance::<crate::js::Types, HTMLElement>(
            HTMLElement::new(document, node_id, ec),
            ec,
        ),
        _ => create_interface_instance::<crate::js::Types, Element>(
            Element::new(document, node_id, ec),
            ec,
        ),
    }?;
    Ok(object)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: The dispatch algorithm's Step 2 builds the event path by walking the
// DOM parent chain. This function implements only that parent-chain traversal
// for click events from the UI event system, without shadow-tree or slot steps.
pub(crate) fn build_path_from_target_js_object(
    target_object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    let mut path: Vec<EventPathItem> = Vec::new();
    let node_info = ec.with_object_any(target_object).and_then(|data| {
        if let Some(element) = data.downcast_ref::<Element>() {
            Some((element.node.node_id, element.node.document.clone()))
        } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            Some((
                html_element.element.node.node_id,
                html_element.element.node.document.clone(),
            ))
        } else if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            Some((
                anchor.html_element.element.node.node_id,
                anchor.html_element.element.node.document.clone(),
            ))
        } else if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            Some((
                iframe.html_element.element.node.node_id,
                iframe.html_element.element.node.document.clone(),
            ))
        } else if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            Some((
                input.html_element.element.node.node_id,
                input.html_element.element.node.document.clone(),
            ))
        } else if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
            Some((
                media.html_element.element.node.node_id,
                media.html_element.element.node.document.clone(),
            ))
        } else if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            Some((
                video.media_element.html_element.element.node.node_id,
                video
                    .media_element
                    .html_element
                    .element
                    .node
                    .document
                    .clone(),
            ))
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
            path.push(EventPathItem {
                invocation_target: event_target.clone(),
                shadow_adjusted_target: Some(event_target),
                has_activation_behavior: target_is_anchor_with_href(ec, target_object),
            });
        }
        let mut current_node_id = node_id;
        let mut visited = HashSet::new();
        visited.insert(node_id);
        loop {
            let parent_id = {
                let doc = document.borrow();
                doc.get_node(current_node_id).and_then(|n| n.parent)
            };
            match parent_id {
                Some(pid) if !visited.contains(&pid) => {
                    visited.insert(pid);
                    if let Ok(parent_object) = resolve_element_object(pid, ec) {
                        if let Some(parent_event_target) =
                            event_target_from_js_object(ec, &parent_object)
                        {
                            path.push(EventPathItem {
                                invocation_target: parent_event_target,
                                shadow_adjusted_target: None,
                                has_activation_behavior: target_is_anchor_with_href(
                                    ec,
                                    &parent_object,
                                ),
                            });
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
            path.push(EventPathItem {
                invocation_target: event_target.clone(),
                shadow_adjusted_target: Some(event_target),
                has_activation_behavior: target_is_anchor_with_href(ec, target_object),
            });
        }
    }
    path
}

/// <https://html.spec.whatwg.org/#links-created-by-a-and-area-elements:activation-behaviour-2>
fn target_is_anchor_with_href(
    ec: &mut dyn ExecutionContext<Types>,
    target_object: &JsObject,
) -> bool {
    ec.with_object_any(target_object)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
        .map(|anchor| anchor.href_attribute().is_some())
        .unwrap_or(false)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: This is the content-process portion of the dispatch algorithm's
// Step 12.1, "run activationTarget's activation behavior with event". The
// element kind is resolved from the path item's platform object and the
// behavior is dispatched to the element's ActivationBehavior implementation.
pub(crate) fn run_activation_behavior_for_path(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
) -> Completion<(), Types> {
    let Some(activation_item) = path.iter().find(|item| item.has_activation_behavior) else {
        return Ok(());
    };
    // Resolve the element from the invocation target's reflector.
    let Some(reflector) = activation_item
        .invocation_target
        .reflector
        .as_ref()
        .cloned()
    else {
        return Ok(());
    };
    // Dispatch to the element kind's activation behavior implementation: the
    // platform object is downcast to the concrete element struct (e.g.
    // HTMLAnchorElement), which implements ActivationBehavior.
    let Some(anchor) = ec
        .with_object_any(&reflector)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>().cloned())
    else {
        // TODO: HTMLAreaElement, HTMLInputElement, HTMLButtonElement, and
        // other element kinds with activation behavior are not yet modeled.
        return Ok(());
    };

    // Gather the navigation context from the realm's global scope. The scope
    // is cloned out of the object registry so its lifetime does not borrow `ec`
    // (the activation behavior also needs the engine for local realm setup).
    let global_scope = with_global_scope(ec, |global_scope, _ec| Ok(global_scope.clone()))?;
    let source_navigable_id = global_scope.source_navigable_id();
    let parent_traversable_id = global_scope.parent_traversable_id();
    let top_level_traversable_id = global_scope.top_level_traversable_id();
    let creation_url = global_scope.creation_url();
    let event_sender = global_scope.event_sender();
    let (Some(source_navigable_id), Some(creation_url), Some(event_sender)) =
        (source_navigable_id, creation_url, event_sender)
    else {
        return Ok(());
    };
    let top_level_traversable_id = top_level_traversable_id.unwrap_or(source_navigable_id);
    let window_global = ec.global_object();
    let parent_engine = ec.as_any_mut().downcast_mut::<crate::js::Engine>();

    anchor
        .activation_behavior(
            source_navigable_id,
            parent_traversable_id,
            top_level_traversable_id,
            &creation_url,
            &reflector,
            &event_sender,
            Some(&global_scope),
            Some(window_global),
            parent_engine,
        )
        .map_err(|error| ec.new_type_error(&error))?;
    Ok(())
}
