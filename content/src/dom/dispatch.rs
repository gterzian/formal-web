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
    /// The JsObject GC handle for the invocation target — needed by Web IDL
    /// callback invocation (`call_user_objects_operation`).
    invocation_target_object: JsObject,
    /// The domain EventTarget — used by all spec algorithm operations
    /// (reading listener list, activation checks, etc.).
    invocation_target: EventTarget,
    /// Shadow-adjusted target JsObject for setting event.target during dispatch.
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
    let target_object = ec.global_object();
    let event_target = event_target_from_window(ec, &target_object);
    let mut event = Event::new(
        event_type.to_owned(),
        false,
        cancelable,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event.clone(), ec)?;
    let path = simple_path(&event_target, &target_object);
    dispatch_event(ec, &path, &mut event, &event_object)
}

fn simple_path(target: &EventTarget, target_object: &JsObject) -> Vec<EventPathEntry> {
    vec![EventPathEntry {
        invocation_target: target.clone(),
        invocation_target_object: target_object.clone(),
        shadow_adjusted_target: Some(target_object.clone()),
    }]
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    ec: &mut dyn ExecutionContext<Types>,
    target: &dyn super::event::EventTargetAccess,
    target_object: &JsObject,
    event_type: &str,
    time_millis: f64,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let mut event = Event::new(
        event_type.to_owned(),
        false,
        false,
        false,
        true,
        time_millis,
    );

    let target_event = target.get_event_target();
    let path = path_for_target(target, &target_event, target_object, legacy_target_override)?;
    let event_object = create_interface_instance::<Types, Event>(event.clone(), ec)?;
    dispatch_event(ec, &path, &mut event, &event_object)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch(
    ec: &mut dyn ExecutionContext<Types>,
    target: &dyn super::event::EventTargetAccess,
    target_object: &JsObject,
    event_object: &JsObject,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let mut event: Event = ec
        .with_object_any(event_object)
        .and_then(|data| {
            data.downcast_ref::<Event>()
                .or_else(|| data.downcast_ref::<crate::dom::UIEvent>().map(|u| &u.event))
                .cloned()
        })
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;

    let target_event = target.get_event_target();
    let path = path_for_target(target, &target_event, target_object, legacy_target_override)?;
    dispatch_event(ec, &path, &mut event, event_object)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_chain(
    ec: &mut dyn ExecutionContext<Types>,
    chain: &[usize],
    event_object: &JsObject,
) -> Completion<bool, Types> {
    let path = if chain.is_empty() {
        let doc_object = document_object(ec)?;
        let doc_target = event_target_from_object(ec, &doc_object);
        let global_obj = global_object(ec);
        let global_target = event_target_from_window(ec, &global_obj);
        vec![
            EventPathEntry {
                invocation_target: doc_target,
                invocation_target_object: doc_object,
                shadow_adjusted_target: None,
            },
            EventPathEntry {
                invocation_target: global_target,
                invocation_target_object: global_obj,
                shadow_adjusted_target: None,
            },
        ]
    } else {
        let mut path = Vec::with_capacity(chain.len() + 2);
        for (index, node_id) in chain.iter().enumerate() {
            let object = resolve_element_object(*node_id, ec)?;
            let event_target = event_target_from_object(ec, &object);
            path.push(EventPathEntry {
                invocation_target: event_target,
                invocation_target_object: object.clone(),
                shadow_adjusted_target: (index == 0).then_some(object),
            });
        }
        let doc_object = document_object(ec)?;
        let doc_target = event_target_from_object(ec, &doc_object);
        path.push(EventPathEntry {
            invocation_target: doc_target,
            invocation_target_object: doc_object,
            shadow_adjusted_target: None,
        });
        let global_obj = global_object(ec);
        let global_target = event_target_from_window(ec, &global_obj);
        path.push(EventPathEntry {
            invocation_target: global_target,
            invocation_target_object: global_obj,
            shadow_adjusted_target: None,
        });
        path
    };

    let mut event: Event = ec
        .with_object_any(event_object)
        .and_then(|data| {
            data.downcast_ref::<Event>()
                .or_else(|| data.downcast_ref::<crate::dom::UIEvent>().map(|u| &u.event))
                .cloned()
        })
        .ok_or_else(|| ec.new_type_error("dispatch_with_chain: event_object is not an Event"))?;
    dispatch_event(ec, &path, &mut event, event_object)

}

// ---------------------------------------------------------------------------
// Event path building
// ---------------------------------------------------------------------------

/// Build the event path for a single target following the spec's
/// "append to an event path" algorithm.
///
/// <https://dom.spec.whatwg.org/#concept-event-path-append>
fn path_for_target(
    target_access: &dyn super::event::EventTargetAccess,
    target: &EventTarget,
    target_object: &JsObject,
    legacy_target_override: bool,
) -> Completion<Vec<EventPathEntry>, Types> {
    let mut path: Vec<EventPathEntry> = Vec::new();
    let shadow_adjusted = Some(target_object.clone());

    path.push(EventPathEntry {
        invocation_target: target.clone(),
        invocation_target_object: target_object.clone(),
        shadow_adjusted_target: shadow_adjusted,
    });

    // Walk up via the trait's get_the_parent.
    loop {
        match target_access.get_the_parent() {
            Some((parent_object, parent_event_target)) => {
                path.push(EventPathEntry {
                    invocation_target: parent_event_target,
                    invocation_target_object: parent_object,
                    shadow_adjusted_target: None,
                });
            }
            None => break,
        }
    }

    Ok(path)
}

/// Extract (document, node_id) from a Node-platform-object JsObject.
fn extract_node_info(
    ec: &mut dyn ExecutionContext<Types>,
    object: &JsObject,
) -> Option<(Rc<RefCell<BaseDocument>>, usize)> {
    use crate::dom::{Element, Node};

    ec.with_object_any(object).and_then(|data| {
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
    })
}

/// Extract the cloneable EventTarget from any JsObject that embeds one.
///
/// Walks all known platform-object types (Window, Document, Element,
/// HTMLElement, HTMLAnchorElement, etc.) to find the embedded EventTarget
/// field and returns a clone.
fn event_target_from_object(
    ec: &mut dyn ExecutionContext<Types>,
    object: &JsObject,
) -> EventTarget {
    ec.with_object_any(object)
        .and_then(|data| {
            if let Some(window) = data.downcast_ref::<Window>() {
                Some(window.event_target.clone())
            } else if let Some(document) = data.downcast_ref::<crate::dom::Document>() {
                Some(document.node.event_target.clone())
            } else if let Some(element) = data.downcast_ref::<crate::dom::Element>() {
                Some(element.node.event_target.clone())
            } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
                Some(html_element.element.node.event_target.clone())
            } else if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
                Some(anchor.html_element.element.node.event_target.clone())
            } else if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
                Some(iframe.html_element.element.node.event_target.clone())
            } else if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
                Some(input.html_element.element.node.event_target.clone())
            } else if let Some(node) = data.downcast_ref::<crate::dom::Node>() {
                Some(node.event_target.clone())
            } else if let Some(target) = data.downcast_ref::<EventTarget>() {
                Some(target.clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn event_target_from_window(
    ec: &mut dyn ExecutionContext<Types>,
    object: &JsObject,
) -> EventTarget {
    ec.with_object_any(object)
        .and_then(|data| data.downcast_ref::<Window>().map(|w| w.event_target.clone()))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Activation behavior — standalone functions
// ---------------------------------------------------------------------------

fn target_has_activation_behavior(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
) -> bool {
    ec.with_object_any(target)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
        .is_some()
}

fn target_run_activation_behavior(
    ec: &mut dyn ExecutionContext<Types>,
    target: &JsObject,
    _event: &JsObject,
) -> Completion<(), Types> {
    let anchor = ec
        .with_object_any(target)
        .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
        .cloned();
    if anchor.is_some() {
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
    event: &mut Event,
    event_object: &JsObject,
) -> Completion<bool, Types> {
    *event.dispatch_flag.borrow_mut() = true;
    *event.stop_propagation_flag.borrow_mut() = false;
    *event.stop_immediate_propagation_flag.borrow_mut() = false;

    let activation_target = compute_activation_target(ec, path, event)?;

    if let Some(ref activation_target) = activation_target {
        run_legacy_pre_activation_behavior(ec, activation_target, event_object)?;
    }

    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        *event.target.borrow_mut() = shadow_adjusted_target(path, index);
        *event.current_target.borrow_mut() = Some(entry.invocation_target_object.clone());
        *event.event_phase.borrow_mut() = phase;

        invoke(ec, path, index, event, event_object, ListenerPhase::Capturing)?;
    }

    for (index, entry) in path.iter().enumerate() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else if *event.bubbles.borrow() {
            BUBBLING_PHASE
        } else {
            continue;
        };

        *event.target.borrow_mut() = shadow_adjusted_target(path, index);
        *event.current_target.borrow_mut() = Some(entry.invocation_target_object.clone());
        *event.event_phase.borrow_mut() = phase;

        invoke(ec, path, index, event, event_object, ListenerPhase::Bubbling)?;
    }

    let canceled = *event.canceled_flag.borrow();
    *event.target.borrow_mut() = None;
    *event.current_target.borrow_mut() = None;
    *event.event_phase.borrow_mut() = NONE;
    *event.dispatch_flag.borrow_mut() = false;
    *event.stop_propagation_flag.borrow_mut() = false;
    *event.stop_immediate_propagation_flag.borrow_mut() = false;

    if let Some(ref activation_target) = activation_target {
        if !canceled {
            run_activation_behavior(ec, activation_target, event_object)?;
        } else {
            run_legacy_canceled_activation_behavior(ec, activation_target, event_object)?;
        }
    }

    Ok(!canceled)
}

fn compute_activation_target(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathEntry],
    event: &Event,
) -> Completion<Option<JsObject>, Types> {
    if event.type_ != "click" {
        return Ok(None);
    }

    let Some(target_entry) = path.first() else {
        return Ok(None);
    };

    let target = target_entry
        .shadow_adjusted_target
        .clone()
        .unwrap_or_else(|| target_entry.invocation_target_object.clone());
    if target_has_activation_behavior(ec, &target) {
        return Ok(Some(target));
    }

    if !*event.bubbles.borrow() {
        return Ok(None);
    }

    for entry in path.iter().skip(1) {
        let candidate = entry.invocation_target_object.clone();
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
    event: &mut Event,
    event_object: &JsObject,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];
    let target = shadow_adjusted_target(path, index);

    if *event.stop_propagation_flag.borrow() {
        return Ok(());
    }

    let phase_value = *event.event_phase.borrow();
    *event.target.borrow_mut() = target;
    *event.current_target.borrow_mut() = Some(entry.invocation_target_object.clone());
    *event.event_phase.borrow_mut() = phase_value;

    let listeners = entry.invocation_target.event_listener_list.borrow().clone();

    if dispatch_debug_enabled() && event.type_ == "click" {
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
            debug_target_label(&entry.invocation_target_object, ec),
            listeners.len(),
            matching_listeners,
        );
    }

    let _found = inner_invoke(ec, &entry.invocation_target_object, &entry.invocation_target, event, event_object, &listeners, phase)?;

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke>
fn inner_invoke(
    ec: &mut dyn ExecutionContext<Types>,
    current_target_object: &JsObject,
    current_target: &EventTarget,
    event: &mut Event,
    event_object: &JsObject,
    listeners: &[EventListener],
    phase: ListenerPhase,
) -> Completion<bool, Types> {
    let mut found = false;

    for listener in listeners.iter().filter(|listener| !listener.removed) {
        if event.type_ != listener.type_ {
            continue;
        }

        found = true;

        if phase == ListenerPhase::Capturing && !listener.capture {
            continue;
        }
        if phase == ListenerPhase::Bubbling && listener.capture {
            continue;
        }

        if listener.once {
            // GcCell shares data across clones — the mutation is visible
            // on the original EventTarget immediately.
            current_target.remove_event_listener_by_id(listener.id);
        }

        if listener.passive == Some(true) {
            *event.in_passive_listener_flag.borrow_mut() = true;
        }

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

        *event.in_passive_listener_flag.borrow_mut() = false;

        if *event.stop_immediate_propagation_flag.borrow() {
            break;
        }
    }

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
