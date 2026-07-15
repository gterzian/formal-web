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
    let event_target = resolve_global_event_target(ec, &target_object);
    let event_domain = Event::new(
        event_type.to_owned(),
        false,
        cancelable,
        false,
        true,
        time_millis,
    );
    let event_object = create_interface_instance::<Types, Event>(event_domain, ec)?;
    dispatch(ec, &event_target, &event_object, false)
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    ec: &mut dyn ExecutionContext<Types>,
    target: &EventTarget,
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

    // Step 5: "Return the result of dispatching event at target, with legacy target override flag set if set."
    dispatch(ec, target, &event_object, legacy_target_override)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch(
    ec: &mut dyn ExecutionContext<Types>,
    target: &EventTarget,
    event_object: &JsObject,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let target_object = target.reflector.as_ref().ok_or_else(|| {
        ec.new_type_error("EventTarget has no reflector — platform object not created")
    })?;
    let path = path_for_target(ec, target, target_object, legacy_target_override)?;
    dispatch_event(ec, &path, event_object)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_chain(
    ec: &mut dyn ExecutionContext<Types>,
    chain: &[usize],
    event_object: &JsObject,
) -> Completion<bool, Types> {
    let path = if chain.is_empty() {
        let doc_object = document_object(ec)?;
        let doc_target = resolve_event_target(ec, &doc_object);
        let global_obj = global_object(ec);
        let global_target = resolve_global_event_target(ec, &global_obj);
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
            let event_target = resolve_event_target(ec, &object);
            path.push(EventPathEntry {
                invocation_target: event_target,
                invocation_target_object: object.clone(),
                shadow_adjusted_target: (index == 0).then_some(object),
            });
        }
        let doc_object = document_object(ec)?;
        let doc_target = resolve_event_target(ec, &doc_object);
        path.push(EventPathEntry {
            invocation_target: doc_target,
            invocation_target_object: doc_object,
            shadow_adjusted_target: None,
        });
        let global_obj = global_object(ec);
        let global_target = resolve_global_event_target(ec, &global_obj);
        path.push(EventPathEntry {
            invocation_target: global_target,
            invocation_target_object: global_obj,
            shadow_adjusted_target: None,
        });
        path
    };

    dispatch_event(ec, &path, event_object)
}

// ---------------------------------------------------------------------------
// Event path building
// ---------------------------------------------------------------------------

/// Build the event path for a single target following the spec's
/// "append to an event path" algorithm.
///
/// <https://dom.spec.whatwg.org/#concept-event-path-append>
fn path_for_target(
    ec: &mut dyn ExecutionContext<Types>,
    target: &EventTarget,
    target_object: &JsObject,
    legacy_target_override: bool,
) -> Completion<Vec<EventPathEntry>, Types> {
    let mut path: Vec<EventPathEntry> = Vec::new();

    // For Window targets with legacy_target_override, the shadow-adjusted target
    // is the associated Document (dispatch step 2).
    let is_window = ec
        .with_object_any(target_object)
        .and_then(|d| d.downcast_ref::<Window>())
        .is_some();
    let shadow_adjusted = if is_window && legacy_target_override {
        let doc_obj = document_object(ec)?;
        Some(doc_obj)
    } else {
        Some(target_object.clone())
    };

    path.push(EventPathEntry {
        invocation_target: target.clone(),
        invocation_target_object: target_object.clone(),
        shadow_adjusted_target: shadow_adjusted,
    });

    // Walk up via get_the_parent, appending each parent.
    let mut current_object = target_object.clone();
    loop {
        let (parent_object, parent_event_target) =
            match get_the_parent(ec, &current_object, target_object) {
                Some(result) => result,
                None => break,
            };
        path.push(EventPathEntry {
            invocation_target: parent_event_target,
            invocation_target_object: parent_object.clone(),
            shadow_adjusted_target: None,
        });
        current_object = parent_object;
    }

    Ok(path)
}

/// <https://dom.spec.whatwg.org/#dom-eventtarget-gettheparent>
///
/// Returns (parent_JsObject, parent_EventTarget) or None.
fn get_the_parent(
    ec: &mut dyn ExecutionContext<Types>,
    target_object: &JsObject,
    _event_object: &JsObject,
) -> Option<(JsObject, EventTarget)> {
    use crate::dom::Document;

    // Window targets have no parent.
    if ec
        .with_object_any(target_object)
        .and_then(|d| d.downcast_ref::<Window>())
        .is_some()
    {
        return None;
    }

    // Document targets return Window as parent.
    if ec
        .with_object_any(target_object)
        .and_then(|d| d.downcast_ref::<Document>())
        .is_some()
    {
        let global_obj = global_object(ec);
        let global_target = resolve_global_event_target(ec, &global_obj);
        return Some((global_obj, global_target));
    }

    // Node targets: extract node_id, walk up via node_chain.
    let (document, node_id) = extract_node_info(ec, target_object)?;
    let chain = document.borrow().node_chain(node_id);
    let parent_id = chain.into_iter().nth(1)?;
    let parent_object = resolve_existing_node_object(document, parent_id, ec).ok()?;
    let parent_target = resolve_event_target(ec, &parent_object);
    Some((parent_object, parent_target))
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
pub(crate) fn resolve_event_target(
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
            } else if let Some(signal) = data.downcast_ref::<super::abort::AbortSignal>() {
                // AbortSignal stores its EventTarget inside a GcCell;
                // clone it via the accessor method.
                Some(signal.with_event_target_mut(|et| et.clone()))
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Resolve the EventTarget from the global (Window) JsObject.
pub(crate) fn resolve_global_event_target(
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

    // Step 12: Run legacy-pre-activation behavior.
    if let Some(ref activation_target) = activation_target {
        run_legacy_pre_activation_behavior(ec, activation_target, event_object)?;
    }

    // Step 13: Capture phase (reverse order).
    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        mutate_event(ec, event_object, |event| {
            event.target = shadow_adjusted_target(path, index);
            event.current_target = Some(entry.invocation_target_object.clone());
            event.event_phase = phase;
        });

        invoke(ec, path, index, event_object, ListenerPhase::Capturing)?;
    }

    // Step 14: Bubble phase.
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
            event.current_target = Some(entry.invocation_target_object.clone());
            event.event_phase = phase;
        });

        invoke(ec, path, index, event_object, ListenerPhase::Bubbling)?;
    }

    // Step 15-18: Cleanup.
    let canceled = access_event(ec, event_object, |event| event.canceled_flag);
    mutate_event(ec, event_object, |event| {
        event.target = None;
        event.current_target = None;
        event.event_phase = NONE;
        event.dispatch_flag = false;
        event.stop_propagation_flag = false;
        event.stop_immediate_propagation_flag = false;
    });

    // Step 20: Activation behavior.
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
    if !(access_event(ec, event_object, |event| event.type_.clone()) == "click") {
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

    if !(access_event(ec, event_object, |event| event.bubbles)) {
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
    event_object: &JsObject,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];
    let target = shadow_adjusted_target(path, index);

    if access_event(ec, event_object, |event| event.stop_propagation_flag) {
        return Ok(());
    }

    let phase_value = access_event(ec, event_object, |event| event.event_phase);
    mutate_event(ec, event_object, |event| {
        event.target = target;
        event.current_target = Some(entry.invocation_target_object.clone());
        event.event_phase = phase_value;
    });

    // Read listener list from the domain EventTarget (no JsObject needed).
    let listeners = entry.invocation_target.event_listener_list.clone();

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
            debug_target_label(&entry.invocation_target_object, ec),
            listeners.len(),
            matching_listeners,
        );
    }

    let _found = inner_invoke(ec, &entry.invocation_target_object, &entry.invocation_target, event_object, &listeners, phase)?;

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke>
fn inner_invoke(
    ec: &mut dyn ExecutionContext<Types>,
    current_target_object: &JsObject,
    current_target: &EventTarget,
    event_object: &JsObject,
    listeners: &[EventListener],
    phase: ListenerPhase,
) -> Completion<bool, Types> {
    let mut found = false;

    for listener in listeners.iter().filter(|listener| !listener.removed) {
        if access_event(ec, event_object, |event| event.type_.clone()) != listener.type_ {
            continue;
        }

        found = true;

        if phase == ListenerPhase::Capturing && !listener.capture {
            continue;
        }
        if phase == ListenerPhase::Bubbling && listener.capture {
            continue;
        }

        // Step 2.5: Once listener → remove from domain EventTarget.
        if listener.once {
            // Remove from the cloned EventTarget domain struct.
            let mut ct = current_target.clone();
            ct.remove_event_listener_by_id(listener.id);
            // Also sync back to the GC — the simplest approach is to write
            // through the JsObject.
            if let Some(data) = ec.with_object_any_mut(current_target_object) {
                if let Some(window) = data.downcast_mut::<Window>() {
                    window.event_target = ct;
                } else if let Some(document) = data.downcast_mut::<crate::dom::Document>() {
                    document.node.event_target = ct;
                } else if let Some(element) = data.downcast_mut::<crate::dom::Element>() {
                    element.node.event_target = ct;
                } else if let Some(html_element) = data.downcast_mut::<HTMLElement>() {
                    html_element.element.node.event_target = ct;
                } else if let Some(anchor) = data.downcast_mut::<HTMLAnchorElement>() {
                    anchor.html_element.element.node.event_target = ct;
                } else if let Some(iframe) = data.downcast_mut::<HTMLIFrameElement>() {
                    iframe.html_element.element.node.event_target = ct;
                } else if let Some(input) = data.downcast_mut::<HTMLInputElement>() {
                    input.html_element.element.node.event_target = ct;
                } else if let Some(node) = data.downcast_mut::<crate::dom::Node>() {
                    node.event_target = ct;
                } else if let Some(et) = data.downcast_mut::<EventTarget>() {
                    *et = ct;
                }
            }
        }

        if listener.passive == Some(true) {
            mutate_event(ec, event_object, |event| {
                event.in_passive_listener_flag = true;
            });
        }

        // Step 2.11: "Call a user object's operation with listener's callback, `handleEvent`, « event »."
        if let Some(callback) = listener.callback.as_ref() {
            // The Web IDL layer receives JsValues — these are the GC handles
            // for the platform objects, obtained from the caller.
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

        mutate_event(ec, event_object, |event| {
            event.in_passive_listener_flag = false;
        });

        if access_event(ec, event_object, |event| event.stop_immediate_propagation_flag) {
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

fn mutate_event(
    ec: &mut dyn ExecutionContext<Types>,
    event_object: &JsObject,
    f: impl FnOnce(&mut Event),
) {
    if let Some(data) = ec.with_object_any_mut(event_object) {
        if let Some(event) = data.downcast_mut::<Event>() {
            f(event);
            return;
        }
        // UIEvent wraps Event; apply the mutation to the inner event.
        if let Some(ui_event) = data.downcast_mut::<crate::dom::UIEvent>() {
            f(&mut ui_event.event);
        }
    }
}

fn access_event<R>(
    ec: &mut dyn ExecutionContext<Types>,
    event_object: &JsObject,
    f: impl FnOnce(&Event) -> R,
) -> R {
    // Try Event first, then UIEvent (which wraps Event).
    if let Some(data) = ec.with_object_any(event_object) {
        if let Some(event) = data.downcast_ref::<Event>() {
            return f(event);
        }
        if let Some(ui_event) = data.downcast_ref::<crate::dom::UIEvent>() {
            return f(&ui_event.event);
        }
    }
    panic!("event_object must wrap an Event or UIEvent");
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
