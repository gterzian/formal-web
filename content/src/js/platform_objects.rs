use std::any::TypeId;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Node as BlitzNode};
use boa_engine::{Context, JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use html5ever::{local_name, ns};
use log::error;

use crate::dom::{Element, Node};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLVideoElement, Window,
};
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use js_engine::{Completion, ExecutionContext};

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
    ec: &mut dyn ExecutionContext<Types>,
    global_object: JsObject,
) {
    ec.store_host_any(TypeId::of::<GlobalObjectSlot>(), Box::new(global_object));
}

/// <https://html.spec.whatwg.org/#global-object>
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

// ── Generic helpers — no ec_to_ctx, pure trait-method access ───────────

/// <https://html.spec.whatwg.org/#global-object>
///
/// Downcast the realm's global object to `&GlobalScope` through
/// `realm_global_object()` + `with_object_any`.  Returns `None` if the
/// global object is not a `Window` or has no native data.
fn global_scope_or_error<'ec>(ec: &'ec dyn ExecutionContext<Types>) -> Option<&'ec GlobalScope> {
    let global_obj = ec.realm_global_object();
    ec.with_object_any(&global_obj)
        .and_then(|data| data.downcast_ref::<Window>())
        .map(|window| &window.global_scope)
}

/// <https://html.spec.whatwg.org/#global-object>
///
/// Like `global_scope_or_error` but constructs a `Completion` error when
/// the global object can't be reached.
pub(crate) fn with_global_scope_ec<R>(
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&GlobalScope) -> Completion<R, Types>,
) -> Completion<R, Types> {
    match global_scope_or_error(ec) {
        Some(gs) => f(gs),
        None => Err(ec.new_type_error("global object is not a Window")),
    }
}

// ── Boa-specific entry points (take &Context) ───────────────────────────

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

// ── _ec wrappers — generic, no ec_to_ctx ───────────────────────────────
//
// Use `realm_global_object()` + `with_object_any` to reach `GlobalScope`.
// Simple wrappers use `with_global_scope_ec`.  Complex ones use block
// scoping to separate immutable `GlobalScope` reads from mutable `ec` calls.

pub(crate) fn document_object_ec(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let missing_err = ec.new_type_error("missing document object");
    with_global_scope_ec(ec, |global_scope| {
        global_scope.document_object().ok_or(missing_err)
    })
}

pub(crate) fn location_object_ec(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<JsObject>, Types> {
    with_global_scope_ec(ec, |global_scope| Ok(global_scope.location_object()))
}

pub(crate) fn store_location_object_ec(
    ec: &mut dyn ExecutionContext<Types>,
    object: JsObject,
) -> Completion<(), Types> {
    with_global_scope_ec(ec, |global_scope| {
        global_scope.store_location_object(object);
        Ok(())
    })
}

pub(crate) fn invalidate_cached_node_ids_ec(
    ec: &mut dyn ExecutionContext<Types>,
    node_ids: &[usize],
) -> Completion<(), Types> {
    with_global_scope_ec(ec, |global_scope| {
        global_scope.invalidate_cached_node_ids(node_ids);
        Ok(())
    })
}

pub(crate) fn take_animation_frame_callbacks_ec(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<crate::webidl::Callback>, Types> {
    with_global_scope_ec(ec, |global_scope| {
        Ok(global_scope.take_animation_frame_callbacks())
    })
}

// ── Complex _ec wrappers (need mutable ec during creation) ─────────────

pub(crate) fn resolve_element_object_ec(
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
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

pub(crate) fn object_for_existing_node_ec(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
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
        resolve_element_object_ec(node_id, ec)
    } else {
        resolve_or_create_text_node_object_ec(document, node_id, ec)
    }
}

pub(crate) fn resolve_or_create_text_node_object_ec(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
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

// ── Non-_ec entry points (take &mut Context; used from Boa-specific callers) ──

pub(crate) fn resolve_element_object(node_id: usize, context: &mut Context) -> JsResult<JsObject> {
    // Read cached node and document (immutable borrow, released immediately).
    let (cached, document) = with_global_scope(context, |gs| {
        Ok((gs.cached_node_object(node_id), gs.document()))
    })?;
    if let Some(object) = cached {
        return Ok(object);
    }

    // Create platform object (mutable borrow, no immutable borrow active).
    let object =
        element_object_from_document(document, node_id, js_engine::boa::context_as_ec(context))
            .map_err(JsError::from_opaque)?;

    // Cache the result (immutable borrow, released immediately).
    with_global_scope(context, |gs| {
        gs.cache_node_object(node_id, object.clone());
        Ok(())
    })?;

    Ok(object)
}

pub(crate) fn resolve_or_create_text_node_object(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    let cached = with_global_scope(context, |gs| Ok(gs.cached_node_object(node_id)))?;
    if let Some(object) = cached {
        return Ok(object);
    }

    let object = create_interface_instance::<crate::js::Types, Node>(
        Node::new(document, node_id),
        js_engine::boa::context_as_ec(context),
    )
    .map_err(JsError::from_opaque)?;

    with_global_scope(context, |gs| {
        gs.cache_node_object(node_id, object.clone());
        Ok(())
    })?;

    Ok(object)
}

pub(crate) fn object_for_existing_node(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    context: &mut Context,
) -> JsResult<JsObject> {
    let cached = with_global_scope(context, |gs| Ok(gs.cached_node_object(node_id)))?;
    if let Some(object) = cached {
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

// ── Implementation helper — element kind dispatch ──────────────────────

fn element_object_from_document(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
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
    }
}
