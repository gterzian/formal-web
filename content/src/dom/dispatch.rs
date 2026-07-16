use log::trace;

use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, Window};
use crate::js::Types;
use super::event::EventTargetAccess;
use crate::js::platform_objects::HasJsObject;
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

/// <https://dom.spec.whatwg.org/#concept-event-path-append>
#[derive(Clone)]
pub(crate) struct EventPathItem {
    /// <https://dom.spec.whatwg.org/#concept-event-path-append>
    invocation_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#concept-event-path-append>
    shadow_adjusted_target: Option<EventTarget>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ListenerPhase {
    Capturing,
    Bubbling,
}

/// Build a single-entry event path (for targets with no parent walking).
pub(crate) fn simple_path(target_access: &dyn super::event::EventTargetAccess) -> Vec<EventPathItem> {
    vec![EventPathItem {
        invocation_target: target_access.get_event_target(),
        shadow_adjusted_target: Some(target_access.get_event_target()),
    }]
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    ec: &mut dyn ExecutionContext<Types>,
    target: &dyn super::event::EventTargetAccess,
    event_type: &str,
    time_millis: f64,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let event = Event::new(
        event_type.to_owned(),
        false,
        false,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event, ec)?;
    // Set reflector on the Event inside the JsObject.
    if let Some(data) = ec.with_object_any_mut(&event_object) {
        if let Some(e) = data.downcast_mut::<Event>() {
            e.reflector = Some(event_object.clone());
        }
    }
    // Clone — GcCell shares data, reflector carried by clone.
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>())
        .cloned()
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;

    let path = path_for_target(target, legacy_target_override)?;
    dispatch_event(ec, &path, &event)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch(
    ec: &mut dyn ExecutionContext<Types>,
    target: &dyn super::event::EventTargetAccess,
    event: &Event,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let path = path_for_target(target, legacy_target_override)?;
    dispatch_event(ec, &path, event)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_chain(
    ec: &mut dyn ExecutionContext<Types>,
    chain: &[usize],
    event: &Event,
) -> Completion<bool, Types> {
    let path = if chain.is_empty() {
        let doc_object = document_object(ec)?;
        let doc_target = event_target_from_object(ec, &doc_object);
        let global_obj = global_object(ec);
        let global_target = event_target_from_window(ec, &global_obj);
        vec![
            EventPathItem {
                invocation_target: doc_target,
                shadow_adjusted_target: None,
            },
            EventPathItem {
                invocation_target: global_target,
                shadow_adjusted_target: None,
            },
        ]
    } else {
        let mut path = Vec::with_capacity(chain.len() + 2);
        for (index, node_id) in chain.iter().enumerate() {
            let object = resolve_element_object(*node_id, ec)?;
            let event_target = event_target_from_object(ec, &object);
            path.push(EventPathItem {
                invocation_target: event_target.clone(),
                shadow_adjusted_target: (index == 0).then_some(event_target),
            });
        }
        let doc_object = document_object(ec)?;
        let doc_target = event_target_from_object(ec, &doc_object);
        path.push(EventPathItem {
            invocation_target: doc_target,
            shadow_adjusted_target: None,
        });
        let global_obj = global_object(ec);
        let global_target = event_target_from_window(ec, &global_obj);
        path.push(EventPathItem {
            invocation_target: global_target,
            shadow_adjusted_target: None,
        });
        path
    };

    dispatch_event(ec, &path, event)

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
    _legacy_target_override: bool,
) -> Completion<Vec<EventPathItem>, Types> {
    let mut path: Vec<EventPathItem> = Vec::new();
    let et = target_access.get_event_target();

    path.push(EventPathItem {
        invocation_target: et.clone(),
        shadow_adjusted_target: Some(et),
    });

    loop {
        match target_access.get_the_parent() {
            Some((_parent_object, parent_event_target)) => {
                path.push(EventPathItem {
                    invocation_target: parent_event_target,
                    shadow_adjusted_target: None,
                });
            }
            None => break,
        }
    }

    Ok(path)
}

/// Extract (document, node_id) from a Node-platform-object JsObject.
/// Extract the cloneable EventTarget from any JsObject that embeds one.
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

pub(crate) fn event_target_from_window(
    ec: &mut dyn ExecutionContext<Types>,
    object: &JsObject,
) -> EventTarget {
    ec.with_object_any(object)
        .and_then(|data| data.downcast_ref::<Window>().map(|w| w.event_target.clone()))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Core event dispatch loop
// ---------------------------------------------------------------------------

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_event(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
    event: &Event,
) -> Completion<bool, Types> {
    *event.dispatch_flag.borrow_mut() = true;

    // Step 6.5: "If isActivationEvent is true and target has activation
    //            behavior, then set activationTarget to target."
    // Step 9.6.1/9.8.2: Walk parents for activation behavior during bubbling.
    let activation_target_idx = if event.type_ == "click" {
        path.iter().position(|entry|
            entry.invocation_target.has_activation_behavior()
        )
    } else {
        None
    };

    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        *event.target.borrow_mut() = shadow_adjusted_target(path, index);
        *event.current_target.borrow_mut() = entry.invocation_target.reflector.clone();
        *event.event_phase.borrow_mut() = phase;

        invoke(ec, path, index, event, ListenerPhase::Capturing)?;
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
        *event.current_target.borrow_mut() = entry.invocation_target.reflector.clone();
        *event.event_phase.borrow_mut() = phase;

        invoke(ec, path, index, event, ListenerPhase::Bubbling)?;
    }

    let canceled = *event.canceled_flag.borrow();
    *event.target.borrow_mut() = None;
    *event.current_target.borrow_mut() = None;
    *event.event_phase.borrow_mut() = NONE;
    *event.dispatch_flag.borrow_mut() = false;
    *event.stop_propagation_flag.borrow_mut() = false;
    *event.stop_immediate_propagation_flag.borrow_mut() = false;

    // Step 12: "If activationTarget is non-null:"
    if let Some(idx) = activation_target_idx {
        // Step 12.1: "If event's canceled flag is unset, then run
        //            activationTarget's activation behavior with event."
        if !canceled {
            path[idx].invocation_target.run_activation_behavior(event)?;
        }
    }

    Ok(!canceled)
}

// ---------------------------------------------------------------------------
// invoke / inner_invoke
// ---------------------------------------------------------------------------

/// <https://dom.spec.whatwg.org/#concept-event-listener-invoke>
fn invoke(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
    index: usize,
    event: &Event,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];
    let target = shadow_adjusted_target(path, index);

    if *event.stop_propagation_flag.borrow() {
        return Ok(());
    }

    let phase_value = *event.event_phase.borrow();
    *event.target.borrow_mut() = target;
    *event.current_target.borrow_mut() = entry.invocation_target.reflector.clone();
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
        if let Some(reflector) = &entry.invocation_target.reflector {
            trace!(
                "[input-debug][dispatch] phase={} current_target={} listeners={} matching_click_listeners={}",
                phase_name,
                debug_target_label(reflector, ec),
                listeners.len(),
                matching_listeners,
            );
        }
    }

    let _found = inner_invoke(ec, &entry.invocation_target, event, &listeners, phase)?;

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke>
fn inner_invoke(
    ec: &mut dyn ExecutionContext<Types>,
    current_target: &EventTarget,
    event: &Event,
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
            current_target.remove_event_listener_by_id(listener.id);
        }

        if listener.passive == Some(true) {
            *event.in_passive_listener_flag.borrow_mut() = true;
        }

        if let Some(callback) = listener.callback.as_ref() {
            let event_value = event.get_js_object().map(|o| {
                <Types as JsTypes>::value_from_object(o)
            });
            let this_value = current_target.get_js_object().map(|o| {
                <Types as JsTypes>::value_from_object(o)
            });
            if let (Some(event_value), Some(this_value)) = (event_value, this_value) {
                if let Err(error) = call_user_objects_operation(
                    ec,
                    callback,
                    "handleEvent",
                    &[event_value],
                    Some(&this_value),
                ) {
                    ec.report_exception(error);
                }
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

fn shadow_adjusted_target(path: &[EventPathItem], index: usize) -> Option<JsObject> {
    path[..=index]
        .iter()
        .rev()
        .find_map(|entry| entry.shadow_adjusted_target.as_ref()?.reflector.clone())
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

