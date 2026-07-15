use log::trace;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;

use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, Window};
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::call_user_objects_operation;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::event::{Event, EventListener, EventTarget, NONE};
use super::BUBBLING_PHASE;
use super::CAPTURING_PHASE;

type JsObject = <Types as JsTypes>::JsObject;

fn dispatch_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

fn debug_target_label(object: &JsObject, ec: &mut dyn ExecutionContext<Types>) -> String {
    use crate::dom::Document;
    use crate::dom::Element;
    use crate::dom::Node;

    if let Some(data) = ec.with_object_any(object) {
        if data.downcast_ref::<Window>().is_some() {
            return String::from("Window");
        }
        if let Some(document) = data.downcast_ref::<Document>() {
            return format!("Document(node={})", document.node.node_id);
        }
        if let Some(html_anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            return format!(
                "HTMLAnchorElement(node={})",
                html_anchor.html_element.element.node.node_id,
            );
        }
        if let Some(html_iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return format!(
                "HTMLIFrameElement(node={})",
                html_iframe.html_element.element.node.node_id,
            );
        }
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return format!("HTMLElement(node={})", html_element.element.node.node_id);
        }
        if let Some(element) = data.downcast_ref::<Element>() {
            return format!("Element(node={})", element.node.node_id);
        }
        if let Some(node) = data.downcast_ref::<Node>() {
            return format!("Node(node={})", node.node_id);
        }
    }
    String::from("UnknownTarget")
}

#[derive(Clone)]
struct EventPathEntry {
    invocation_target: JsObject,
    shadow_adjusted_target: Option<JsObject>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ListenerPhase {
    Capturing,
    Bubbling,
}

/// Convenience to fire a simple event at the global object.
///
/// Used by HTML-level algorithms such as
/// <https://html.spec.whatwg.org/multipage/#steps-to-fire-beforeunload>.
pub(crate) fn dispatch_window_event(
    ec: &mut dyn ExecutionContext<Types>,
    event_type: &str,
    cancelable: bool,
    time_millis: f64,
) -> Completion<bool, Types> {
    let target = ec.global_object();
    let event_domain = Event::new(
        event_type.to_owned(),
        false,
        cancelable,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event_domain, ec)?;
    dispatch(ec, &target, &event_object, false)
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    event_type: &str,
    time_millis: f64,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    // Step 1: "If eventConstructor is not given, then let eventConstructor be Event."

    // Step 2: "Let event be the result of creating an event given eventConstructor, in the relevant realm of target."
    let event_domain = Event::new(
        event_type.to_owned(),
        false,
        false,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event_domain, ec)?;

    // Step 3: "Initialize event's type attribute to e."
    // Done above in Event::new.

    // Step 5: "Return the result of dispatching event at target, with legacy target override flag set if set."
    dispatch(ec, target, &event_object, legacy_target_override)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    event_object: &JsObject,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let path = path_for_target(ec, target, legacy_target_override)?;
    dispatch_event(ec, &path, event_object)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_chain(
    ec: &mut dyn ExecutionContext<Types>,
    chain: &[usize],
    event_object: &JsObject,
) -> Completion<bool, Types> {
    let path = if chain.is_empty() {
        let document = document_object(ec)?;
        vec![
            EventPathEntry {
                invocation_target: document.clone(),
                shadow_adjusted_target: Some(document),
            },
            EventPathEntry {
                invocation_target: global_object(ec),
                shadow_adjusted_target: None,
            },
        ]
    } else {
        let mut path = Vec::with_capacity(chain.len() + 2);
        for (index, node_id) in chain.iter().enumerate() {
            let object = resolve_element_object(*node_id, ec)?;
            path.push(EventPathEntry {
                invocation_target: object.clone(),
                shadow_adjusted_target: (index == 0).then_some(object),
            });
        }
        path.push(EventPathEntry {
            invocation_target: document_object(ec)?,
            shadow_adjusted_target: None,
        });
        path.push(EventPathEntry {
            invocation_target: global_object(ec),
            shadow_adjusted_target: None,
        });
        path
    };

    dispatch_event(ec, &path, event_object)
}

/// Build the event path for a single target following the spec's
/// "append to an event path" algorithm.
///
/// <https://dom.spec.whatwg.org/#concept-event-path-append>
///
/// Walks `get_the_parent` from the target to build the capture/bubble path.
fn path_for_target(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    legacy_target_override: bool,
) -> Completion<Vec<EventPathEntry>, Types> {
    let mut path: Vec<EventPathEntry> = Vec::new();

    // For Window targets with legacy_target_override, the shadow-adjusted target
    // is the associated Document (<https://dom.spec.whatwg.org/#concept-event-dispatch>, step 2).
    let is_window = ec
        .with_object_any(target)
        .and_then(|d| d.downcast_ref::<Window>())
        .is_some();
    let shadow_adjusted = if is_window && legacy_target_override {
        Some(document_object(ec)?)
    } else {
        Some(target.clone())
    };

    append_to_event_path(&mut path, target.clone(), shadow_adjusted);

    // Walk up via get_the_parent, appending each parent.
    let mut current = target.clone();
    loop {
        let parent = get_the_parent(ec, &current, target);
        match parent {
            Some(parent_obj) => {
                append_to_event_path(&mut path, parent_obj.clone(), None);
                current = parent_obj;
            }
            None => break,
        }
    }

    Ok(path)
}

/// <https://dom.spec.whatwg.org/#dom-eventtarget-gettheparent>
///
/// Returns the parent EventTarget for the event path.
fn get_the_parent(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    _event: &JsObject,
) -> Option<JsObject> {
    use crate::dom::{Document, Element, Node};

    // For Window targets: parent is null (no parent).
    if ec
        .with_object_any(target)
        .and_then(|d| d.downcast_ref::<Window>())
        .is_some()
    {
        return None;
    }

    // For Document targets: parent is the Window.
    if ec
        .with_object_any(target)
        .and_then(|d| d.downcast_ref::<Document>())
        .is_some()
    {
        return Some(global_object(ec));
    }

    // For Node targets: extract node_id, walk up via node_chain.
    let node_info: Option<(Rc<RefCell<BaseDocument>>, usize)> =
        ec.with_object_any(target).and_then(|data| {
            if let Some(element) = data.downcast_ref::<Element>() {
                Some((Rc::clone(&element.node.document), element.node.node_id))
            } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
                Some((
                    Rc::clone(&html_element.element.node.document),
                    html_element.element.node.node_id,
                ))
            } else if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
                Some((
                    Rc::clone(&input.html_element.element.node.document),
                    input.html_element.element.node.node_id,
                ))
            } else if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
                Some((
                    Rc::clone(&anchor.html_element.element.node.document),
                    anchor.html_element.element.node.node_id,
                ))
            } else if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
                Some((
                    Rc::clone(&iframe.html_element.element.node.document),
                    iframe.html_element.element.node.node_id,
                ))
            } else if let Some(node) = data.downcast_ref::<Node>() {
                Some((Rc::clone(&node.document), node.node_id))
            } else {
                None
            }
        });

    let (document, node_id) = node_info?;
    let chain = document.borrow().node_chain(node_id);
    let parent_id = chain.into_iter().nth(1)?;
    resolve_existing_node_object(document, parent_id, ec).ok()
}

/// <https://dom.spec.whatwg.org/#concept-event-path-append>
fn append_to_event_path(
    path: &mut Vec<EventPathEntry>,
    invocation_target: JsObject,
    shadow_adjusted_target: Option<JsObject>,
) {
    path.push(EventPathEntry {
        invocation_target,
        shadow_adjusted_target,
    });
}

// ---------------------------------------------------------------------------
// Activation behavior — standalone functions downcasting the target JsObject
// ---------------------------------------------------------------------------

fn target_has_activation_behavior(
    _ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
) -> bool {
    // Currently only HTMLAnchorElement has activation behavior.
    _ec.with_object_any(target)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
        .is_some()
}

fn target_run_activation_behavior(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    event: &JsObject,
) -> Completion<(), Types> {
    let anchor = ec
        .with_object_any(target)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
        .cloned();
    if let Some(anchor) = anchor {
        // Activation behavior requires content-process context (navigable IDs,
        // IpcSender) which is handled by `run_anchor_activation_behavior` in
        // `ui_event_dispatch.rs`.  The plain-EC path returns an error to
        // indicate this must be handled by the caller.
        return Err(ec.new_type_error(
            "anchor activation behavior requires content-process context",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core event dispatch loop
// ---------------------------------------------------------------------------

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
fn dispatch_event(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathEntry],
    event_object: &JsObject,
) -> Completion<bool, Types> {
    // Step 1: "Set event's dispatch flag."
    mutate_event(ec, event_object, |event| {
        event.dispatch_flag = true;
        event.stop_propagation_flag = false;
        event.stop_immediate_propagation_flag = false;
    });

    // Step 3: "Let activationTarget be null."
    let activation_target = compute_activation_target(ec, path, event_object)?;

    // Step 12: "If activationTarget is non-null and activationTarget has
    // legacy-pre-activation behavior, then run activationTarget's
    // legacy-pre-activation behavior."
    if let Some(ref activation_target) = activation_target {
        run_legacy_pre_activation_behavior(ec, activation_target, event_object)?;
    }

    // Step 13: "For each struct of event's path, in reverse order:"
    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        mutate_event(ec, event_object, |event| {
            event.target = shadow_adjusted_target(path, index);
            event.current_target = Some(entry.invocation_target.clone());
            event.event_phase = phase;
        });

        // Step 13.3: "Invoke with struct, event, `capturing`."
        invoke(ec, path, index, event_object, ListenerPhase::Capturing)?;
    }

    // Step 14: "For each struct of event's path:"
    for (index, entry) in path.iter().enumerate() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else if access_event(ec, event_object, |event| event.bubbles) {
            BUBBLING_PHASE
        } else {
            continue;
        };

        mutate_event(ec, event_object, |event| {
            event.target = shadow_adjusted_target(path, index);
            event.current_target = Some(entry.invocation_target.clone());
            event.event_phase = phase;
        });

        invoke(ec, path, index, event_object, ListenerPhase::Bubbling)?;
    }

    // Step 15-18: Cleanup
    let canceled = access_event(ec, event_object, |event| event.canceled_flag);
    mutate_event(ec, event_object, |event| {
        event.target = None;
        event.current_target = None;
        event.event_phase = NONE;
        event.dispatch_flag = false;
        event.stop_propagation_flag = false;
        event.stop_immediate_propagation_flag = false;
    });

    // Step 20: "If activationTarget is non-null:"
    if let Some(ref activation_target) = activation_target {
        if !canceled {
            run_activation_behavior(ec, activation_target, event_object)?;
        } else {
            run_legacy_canceled_activation_behavior(ec, activation_target, event_object)?;
        }
    }

    // Step 21: "Return false if event's canceled flag is set; otherwise true."
    Ok(!canceled)
}

fn compute_activation_target(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathEntry],
    event_object: &JsObject,
) -> Completion<Option<JsObject>, Types> {
    // Step 6.5: "If isActivationEvent is true and target has activation behavior,
    // then set activationTarget to target."
    if !(access_event(ec, event_object, |event| event.type_.clone()) == "click") {
        return Ok(None);
    }

    let Some(target_entry) = path.first() else {
        return Ok(None);
    };

    let target = target_entry
        .shadow_adjusted_target
        .clone()
        .unwrap_or_else(|| target_entry.invocation_target.clone());
    if target_has_activation_behavior(ec, &target) {
        return Ok(Some(target));
    }

    // Steps 9.6.1/9.8.2: walk ancestors for activation behavior.
    if !(access_event(ec, event_object, |event| event.bubbles)) {
        return Ok(None);
    }

    for entry in path.iter().skip(1) {
        let candidate = entry.invocation_target.clone();
        if target_has_activation_behavior(ec, &candidate) {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

fn run_legacy_pre_activation_behavior(
    _ec: &mut dyn ExecutionContext<Types>,
    _target: &JsObject,
    _event: &JsObject,
) -> Completion<(), Types> {
    // Note: No type in the current codebase implements legacy-pre-activation behavior.
    Ok(())
}

fn run_activation_behavior(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    event: &JsObject,
) -> Completion<(), Types> {
    target_run_activation_behavior(ec, target, event)
}

fn run_legacy_canceled_activation_behavior(
    _ec: &mut dyn ExecutionContext<Types>,
    _target: &JsObject,
    _event: &JsObject,
) -> Completion<(), Types> {
    // Note: No type in the current codebase implements legacy-canceled-activation behavior.
    Ok(())
}

// ---------------------------------------------------------------------------
// invoke / inner_invoke
// ---------------------------------------------------------------------------

/// <https://dom.spec.whatwg.org/#concept-event-listener-invoke>
fn invoke(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathEntry],
    index: usize,
    event_object: &JsObject,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];
    let target = shadow_adjusted_target(path, index);

    // Step 6: "If event's stop propagation flag is set, then return."
    if access_event(ec, event_object, |event| event.stop_propagation_flag) {
        return Ok(());
    }

    // Step 7: "Initialize event's currentTarget attribute to pathItem's invocation target."
    let phase_value = access_event(ec, event_object, |event| event.event_phase);
    mutate_event(ec, event_object, |event| {
        event.target = target;
        event.current_target = Some(entry.invocation_target.clone());
        event.event_phase = phase_value;
    });

    // Step 8: "Let listeners be a clone of event's currentTarget attribute value's event listener list."
    let listeners = read_event_target_list(ec, &entry.invocation_target);

    if dispatch_debug_enabled() && access_event(ec, event_object, |event| event.type_.clone()) == "click"
    {
        let matching_listeners = listeners
            .iter()
            .filter(|listener| !listener.removed && listener.type_ == "click")
            .count();
        let phase_name = match phase {
            ListenerPhase::Capturing => "capturing",
            ListenerPhase::Bubbling => "bubbling",
        };
        trace!(
            "[input-debug][dispatch] phase={} current_target={} listeners={} matching_click_listeners={}",
            phase_name,
            debug_target_label(&entry.invocation_target, ec),
            listeners.len(),
            matching_listeners,
        );
    }

    // Step 10: "Let found be the result of running inner invoke with event, listeners, phase, ..."
    let _found = inner_invoke(ec, &entry.invocation_target, event_object, &listeners, phase)?;

    // Step 11: Legacy event-type remapping (not yet wired).

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke>
fn inner_invoke(
    ec: &mut dyn ExecutionContext<Types>,
    current_target_object: &JsObject,
    event_object: &JsObject,
    listeners: &[EventListener],
    phase: ListenerPhase,
) -> Completion<bool, Types> {
    // Step 1: "Let found be false."
    let mut found = false;

    // Step 2: "For each listener of listeners, whose removed is false:"
    for listener in listeners.iter().filter(|listener| !listener.removed) {
        // Step 2.1: "If event's type attribute value is not listener's type, then continue."
        if access_event(ec, event_object, |event| event.type_.clone()) != listener.type_ {
            continue;
        }

        // Step 2.2: "Set found to true."
        found = true;

        // Step 2.3: "If phase is `capturing` and listener's capture is false, then continue."
        if phase == ListenerPhase::Capturing && !listener.capture {
            continue;
        }

        // Step 2.4: "If phase is `bubbling` and listener's capture is true, then continue."
        if phase == ListenerPhase::Bubbling && listener.capture {
            continue;
        }

        // Step 2.5: "If listener's once is true, then remove an event listener."
        if listener.once {
            remove_listener_by_id(ec, current_target_object, listener.id);
        }

        // Step 2.9: "If listener's passive is true, then set event's in passive listener flag."
        if listener.passive == Some(true) {
            mutate_event(ec, event_object, |event| {
                event.in_passive_listener_flag = true;
            });
        }

        // Step 2.11: "Call a user object's operation with listener's callback, `handleEvent`, « event »."
        if let Some(callback) = listener.callback.as_ref() {
            if let Err(error) = call_user_objects_operation(
                ec,
                callback,
                "handleEvent",
                &[<Types as JsTypes>::value_from_object(event_object.clone())],
                Some(&<Types as JsTypes>::value_from_object(
                    current_target_object.clone(),
                )),
            ) {
                ec.report_exception(error);
            }
        }

        // Step 2.12: "Unset event's in passive listener flag."
        mutate_event(ec, event_object, |event| {
            event.in_passive_listener_flag = false;
        });

        // Step 2.14: "If event's stop immediate propagation flag is set, then break."
        if access_event(ec, event_object, |event| event.stop_immediate_propagation_flag) {
            break;
        }
    }

    // Step 3: "Return found."
    Ok(found)
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn shadow_adjusted_target(path: &[EventPathEntry], index: usize) -> Option<JsObject> {
    path[..=index]
        .iter()
        .rev()
        .find_map(|entry| entry.shadow_adjusted_target.clone())
}

fn read_event_target_list(
    ec: &mut dyn ExecutionContext<Types>,
    target_object: &JsObject,
) -> Vec<EventListener> {
    ec.with_object_any(target_object)
        .and_then(|data| data.downcast_ref::<EventTarget>())
        .map(|event_target| event_target.event_listener_list.clone())
        .unwrap_or_default()
}

fn remove_listener_by_id(
    ec: &mut dyn ExecutionContext<Types>,
    target_object: &JsObject,
    listener_id: u64,
) {
    if let Some(data) = ec.with_object_any_mut(target_object) {
        if let Some(event_target) = data.downcast_mut::<EventTarget>() {
            event_target.remove_event_listener_by_id(listener_id);
        }
    }
}

fn mutate_event(
    ec: &mut dyn ExecutionContext<Types>,
    event_object: &JsObject,
    f: impl FnOnce(&mut Event),
) {
    if let Some(data) = ec.with_object_any_mut(event_object) {
        if let Some(event) = data.downcast_mut::<Event>() {
            f(event);
        }
    }
}

fn access_event<R>(
    ec: &mut dyn ExecutionContext<Types>,
    event_object: &JsObject,
    f: impl FnOnce(&Event) -> R,
) -> R {
    ec.with_object_any(event_object)
        .and_then(|data| data.downcast_ref::<Event>().map(f))
        .expect("event_object must wrap an Event")
}

// ---------------------------------------------------------------------------
// Platform object resolution helpers
// ---------------------------------------------------------------------------

fn document_object(ec: &mut dyn ExecutionContext<Types>) -> Completion<JsObject, Types> {
    crate::js::platform_objects::document_object(ec)
}

fn global_object(ec: &mut dyn ExecutionContext<Types>) -> JsObject {
    ec.global_object()
}

fn resolve_element_object(
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    crate::js::platform_objects::resolve_element_object(node_id, ec)
}

fn resolve_existing_node_object(
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    crate::js::platform_objects::object_for_existing_node(document, node_id, ec)
}
