use log::trace;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;

use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, Window};
use crate::js::Types;
use crate::js::{try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref};
use crate::webidl::call_user_objects_operation;
use js_engine::{Completion, EcmascriptHost, ExecutionContext, JsTypes};

use super::event::{EventListener, NONE};
use super::{BUBBLING_PHASE, CAPTURING_PHASE, Document, Element, Event, Node};

type JsObject = <Types as JsTypes>::JsObject;

fn dispatch_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

fn debug_target_label(object: &JsObject, ec: &mut dyn ExecutionContext<Types>) -> String {
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

pub(crate) trait EventDispatchHost: EcmascriptHost<Types> {
    fn ec(&mut self) -> &mut dyn ExecutionContext<Types>;

    fn create_event_object(&mut self, event: Event) -> Completion<JsObject, Types>;

    fn document_object(&mut self) -> Completion<JsObject, Types>;

    fn global_object(&mut self) -> JsObject;

    fn resolve_element_object(&mut self, node_id: usize) -> Completion<JsObject, Types>;

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> Completion<JsObject, Types>;

    fn current_time_millis(&self) -> f64;

    fn has_activation_behavior(&mut self, _target: &JsObject) -> bool {
        false
    }

    fn run_legacy_pre_activation_behavior(
        &mut self,
        _target: &JsObject,
        _event: &JsObject,
    ) -> Completion<(), Types> {
        Ok(())
    }

    fn run_activation_behavior(
        &mut self,
        _target: &JsObject,
        _event: &JsObject,
    ) -> Completion<(), Types> {
        Ok(())
    }

    fn run_legacy_canceled_activation_behavior(
        &mut self,
        _target: &JsObject,
        _event: &JsObject,
    ) -> Completion<(), Types> {
        Ok(())
    }
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

/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    host: &mut impl EventDispatchHost,
    target: &JsObject,
    event_type: &str,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    // Step 1: "If eventConstructor is not given, then let eventConstructor be Event."
    // Note: This helper currently models only the default `Event` constructor path.

    // Step 2: "Let event be the result of creating an event given eventConstructor, in the relevant realm of target."
    let event = host.create_event_object(Event::new(
        event_type.to_owned(),
        false,
        false,
        false,
        true,
        host.current_time_millis(),
    ))?;

    // Step 3: "Initialize event's type attribute to e."
    // Step 4: "Initialize any other IDL attributes of event as described in the invocation of this algorithm."
    // Note: The implementation constructs the [platform object](https://webidl.spec.whatwg.org/#dfn-platform-object) implementing the [Event](https://dom.spec.whatwg.org/#interface-event) interface with the final attribute values before dispatch.

    // Step 5: "Return the result of dispatching event at target, with legacy target override flag set if set."
    dispatch(host, target, &event, legacy_target_override)
}

/// <https://html.spec.whatwg.org/multipage/#steps-to-fire-beforeunload>
pub(crate) fn dispatch_window_event(
    host: &mut impl EventDispatchHost,
    event_type: &str,
    cancelable: bool,
) -> Completion<bool, Types> {
    let event = host.create_event_object(Event::new(
        event_type.to_owned(),
        false,
        cancelable,
        false,
        true,
        host.current_time_millis(),
    ))?;
    let target = host.global_object();
    dispatch(host, &target, &event, false)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch(
    host: &mut impl EventDispatchHost,
    target: &JsObject,
    event: &JsObject,
    legacy_target_override: bool,
) -> Completion<bool, Types> {
    let path = path_for_target(host, target, legacy_target_override)?;
    dispatch_on_path(host, &path, event)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_chain(
    host: &mut impl EventDispatchHost,
    chain: &[usize],
    event: &JsObject,
) -> Completion<bool, Types> {
    let path = if chain.is_empty() {
        let document = host.document_object()?;
        vec![
            EventPathEntry {
                invocation_target: document.clone(),
                shadow_adjusted_target: Some(document),
            },
            EventPathEntry {
                invocation_target: host.global_object(),
                shadow_adjusted_target: None,
            },
        ]
    } else {
        let mut path = Vec::with_capacity(chain.len() + 2);
        for (index, node_id) in chain.iter().enumerate() {
            let object = host.resolve_element_object(*node_id)?;
            path.push(EventPathEntry {
                invocation_target: object.clone(),
                shadow_adjusted_target: (index == 0).then_some(object),
            });
        }
        path.push(EventPathEntry {
            invocation_target: host.document_object()?,
            shadow_adjusted_target: None,
        });
        path.push(EventPathEntry {
            invocation_target: host.global_object(),
            shadow_adjusted_target: None,
        });
        path
    };

    dispatch_on_path(host, &path, event)
}

fn path_for_target(
    host: &mut impl EventDispatchHost,
    target: &JsObject,
    legacy_target_override: bool,
) -> Completion<Vec<EventPathEntry>, Types> {
    // Extract node info (doc, node_id) in a scoped block so the EC borrow doesn't
    // conflict with the subsequent path_for_node call that needs `host`.
    let node_info: Option<(Rc<RefCell<BaseDocument>>, usize)> =
        host.ec().with_object_any(target).and_then(|data| {
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

    if let Some((doc, node_id)) = node_info {
        return path_for_node(host, doc, node_id, target.clone());
    }

    // Check Window target.
    let is_window = host
        .ec()
        .with_object_any(target)
        .and_then(|d| d.downcast_ref::<Window>())
        .is_some();
    if is_window {
        return Ok(vec![EventPathEntry {
            invocation_target: target.clone(),
            shadow_adjusted_target: Some(if legacy_target_override {
                host.document_object()?
            } else {
                target.clone()
            }),
        }]);
    }

    // Check Document target.
    let is_document = host
        .ec()
        .with_object_any(target)
        .and_then(|d| d.downcast_ref::<Document>())
        .is_some();
    if is_document {
        return Ok(vec![
            EventPathEntry {
                invocation_target: target.clone(),
                shadow_adjusted_target: Some(target.clone()),
            },
            EventPathEntry {
                invocation_target: host.global_object(),
                shadow_adjusted_target: None,
            },
        ]);
    }

    Ok(vec![EventPathEntry {
        invocation_target: target.clone(),
        shadow_adjusted_target: Some(target.clone()),
    }])
}

fn path_for_node(
    host: &mut impl EventDispatchHost,
    document: Rc<RefCell<BaseDocument>>,
    node_id: usize,
    target: JsObject,
) -> Completion<Vec<EventPathEntry>, Types> {
    let mut path = vec![EventPathEntry {
        invocation_target: target.clone(),
        shadow_adjusted_target: Some(target),
    }];

    let chain = document.borrow().node_chain(node_id);
    for ancestor_id in chain.into_iter().skip(1) {
        path.push(EventPathEntry {
            invocation_target: host
                .resolve_existing_node_object(Rc::clone(&document), ancestor_id)?,
            shadow_adjusted_target: None,
        });
    }

    path.push(EventPathEntry {
        invocation_target: host.document_object()?,
        shadow_adjusted_target: None,
    });
    path.push(EventPathEntry {
        invocation_target: host.global_object(),
        shadow_adjusted_target: None,
    });
    Ok(path)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
/// Note: This helper continues the dispatch algorithm after the event path has already been constructed.
fn dispatch_on_path(
    host: &mut impl EventDispatchHost,
    path: &[EventPathEntry],
    event: &JsObject,
) -> Completion<bool, Types> {
    // Step 1: "Set event's dispatch flag."
    let event_value = <Types as JsTypes>::value_from_object(event.clone());
    try_with_event_mut(&event_value, host.ec(), |inner| {
        inner.dispatch_flag = true;
        inner.stop_propagation_flag = false;
        inner.stop_immediate_propagation_flag = false;
    })?;

    // Step 2: "Let targetOverride be target, if legacy target override flag is not given, and target's associated Document otherwise."
    // Note: `path_for_target` resolves the shadow-adjusted target chosen for this dispatch ahead of time.

    // Step 3: "Let activationTarget be null."
    let activation_target = activation_target(host, path, event)?;

    // Step 4: "Let relatedTarget be the result of retargeting event's relatedTarget against target."
    // Step 5: "Let clearTargets be false."
    // Note: The content process does not yet model related targets or shadow-tree target clearing.

    // Step 12: "If activationTarget is non-null and activationTarget has legacy-pre-activation behavior, then run activationTarget's legacy-pre-activation behavior."
    if let Some(activation_target) = activation_target.as_ref() {
        host.run_legacy_pre_activation_behavior(activation_target, event)?;
    }

    // Step 13: "For each struct of event's path, in reverse order:"
    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        // Step 13.1: "If struct's shadow-adjusted target is non-null, then set event's eventPhase attribute to AT_TARGET."
        // Step 13.2: "Otherwise, set event's eventPhase attribute to CAPTURING_PHASE."
        set_event_target_state(
            host,
            event,
            shadow_adjusted_target(path, index),
            Some(entry.invocation_target.clone()),
            phase,
        )?;

        // Step 13.3: "Invoke with struct, event, `capturing`, and legacyOutputDidListenersThrowFlag if given."
        invoke(host, path, index, event, ListenerPhase::Capturing)?;
    }

    // Step 14: "For each struct of event's path:"
    for (index, entry) in path.iter().enumerate() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else if bubbles(host, event)? {
            BUBBLING_PHASE
        } else {
            continue;
        };

        // Step 14.1: "If struct's shadow-adjusted target is non-null, then set event's eventPhase attribute to AT_TARGET."
        // Step 14.2.1: "If event's bubbles attribute is false, then continue."
        // Step 14.2.2: "Set event's eventPhase attribute to BUBBLING_PHASE."
        set_event_target_state(
            host,
            event,
            shadow_adjusted_target(path, index),
            Some(entry.invocation_target.clone()),
            phase,
        )?;

        // Step 14.3: "Invoke with struct, event, `bubbling`, and legacyOutputDidListenersThrowFlag if given."
        invoke(host, path, index, event, ListenerPhase::Bubbling)?;
    }

    // Step 15: "Set event's eventPhase attribute to NONE."
    // Step 16: "Set event's currentTarget attribute to null."
    // Step 17: "Set event's path to the empty list."
    // Step 18: "Unset event's dispatch flag, stop propagation flag, and stop immediate propagation flag."
    let canceled = canceled(host, event)?;
    set_event_target_state(host, event, None, None, NONE)?;
    try_with_event_mut(&event_value, host.ec(), |inner| {
        inner.dispatch_flag = false;
        inner.stop_propagation_flag = false;
        inner.stop_immediate_propagation_flag = false;
    })?;

    // Step 19: "If clearTargets is true:"
    // Note: The content process does not yet model shadow-tree target clearing.

    // Step 20: "If activationTarget is non-null:"
    if let Some(activation_target) = activation_target.as_ref() {
        if !canceled {
            host.run_activation_behavior(activation_target, event)?;
        } else {
            host.run_legacy_canceled_activation_behavior(activation_target, event)?;
        }
    }

    // Step 21: "Return false if event's canceled flag is set; otherwise true."
    Ok(!canceled)
}

fn activation_target(
    host: &mut impl EventDispatchHost,
    path: &[EventPathEntry],
    event: &JsObject,
) -> Completion<Option<JsObject>, Types> {
    // Note: This helper models the `activationTarget` selection performed while DOM dispatch
    // appends the initial target and then walks up through its parents. The implementation does
    // not model shadow trees, so the spec's two `activationTarget` assignment sites collapse to a
    // target-first check plus a bubbling-only ancestor scan.

    // Step 5: "If isActivationEvent is true and target has activation behavior, then set activationTarget to target."
    // Note: The implementation does not yet materialize `MouseEvent`, so trusted `click`
    // dispatch is treated as the activation-event signal used by HTML activation behavior hooks.
    if !is_activation_event(host, event)? {
        return Ok(None);
    }

    let Some(target_entry) = path.first() else {
        return Ok(None);
    };

    let target = target_entry
        .shadow_adjusted_target
        .clone()
        .unwrap_or_else(|| target_entry.invocation_target.clone());
    if host.has_activation_behavior(&target) {
        return Ok(Some(target));
    }

    // Steps 9.6.1 and 9.8.2: "If isActivationEvent is true, event's bubbles attribute is true,
    // activationTarget is null, and parent/target has activation behavior, then set activationTarget
    // to parent/target."
    if !bubbles(host, event)? {
        return Ok(None);
    }

    for entry in path.iter().skip(1) {
        let candidate = entry.invocation_target.clone();
        if host.has_activation_behavior(&candidate) {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-invoke>
fn invoke(
    host: &mut impl EventDispatchHost,
    path: &[EventPathEntry],
    index: usize,
    event: &JsObject,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];

    // Step 1: "Set event's target to the shadow-adjusted target of the last struct in event's path, that is either struct or preceding struct, whose shadow-adjusted target is non-null."
    let target = shadow_adjusted_target(path, index);

    // Step 2: "Set event's relatedTarget to struct's relatedTarget."
    // Step 3: "Set event's touch target list to struct's touch target list."
    // Note: The content process does not yet model related targets or touch target lists.

    // Step 4: "If event's stop propagation flag is set, then return."
    if stop_propagation(host, event)? {
        return Ok(());
    }

    // Step 5: "Initialize event's currentTarget attribute to struct's invocation target."
    let current_phase = event_phase(host, event)?;
    set_event_target_state(
        host,
        event,
        target,
        Some(entry.invocation_target.clone()),
        current_phase,
    )?;

    // Step 6: "Let listeners be a clone of event's currentTarget attribute value's event listener list."
    let listeners = try_with_event_target_ref(
        &<Types as JsTypes>::value_from_object(entry.invocation_target.clone()),
        host.ec(),
        |event_target| event_target.event_listener_list.clone(),
    )?;

    if dispatch_debug_enabled() && event_type(host, event)? == "click" {
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
            debug_target_label(&entry.invocation_target, host.ec()),
            listeners.len(),
            matching_listeners,
        );
    }

    // Step 7: "Let invocationTargetInShadowTree be struct's invocation-target-in-shadow-tree."
    // Note: The implementation does not model shadow trees, so this is always false.

    // Step 8: "Let found be the result of running inner invoke with event, listeners, phase, invocationTargetInShadowTree, and legacyOutputDidListenersThrowFlag if given."
    let _found = inner_invoke(host, &entry.invocation_target, event, &listeners, phase)?;

    // Step 9: "If found is false and event's isTrusted attribute is true:"
    // Note: The implementation does not implement legacy event-type remapping.

    Ok(())
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-inner-invoke>
fn inner_invoke(
    host: &mut impl EventDispatchHost,
    current_target: &JsObject,
    event: &JsObject,
    listeners: &[EventListener],
    phase: ListenerPhase,
) -> Completion<bool, Types> {
    // Step 1: "Let found be false."
    let mut found = false;

    // Step 2: "For each listener of listeners, whose removed is false:"
    for listener in listeners.iter().filter(|listener| !listener.removed) {
        if !listener_is_active(host, current_target, listener.id)? {
            continue;
        }

        // Step 2.1: "If event's type attribute value is not listener's type, then continue."
        if event_type(host, event)? != listener.type_ {
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

        // Step 2.5: "If listener's once is true, then remove an event listener given event's currentTarget attribute value and listener."
        if listener.once {
            try_with_event_target_mut(
                &<Types as JsTypes>::value_from_object(current_target.clone()),
                host.ec(),
                |event_target| {
                    event_target.remove_event_listener_by_id(listener.id);
                },
            )?;
        }

        // Step 2.6: "Let global be listener callback's associated realm's global object."
        // Step 2.7: "Let currentEvent be undefined."
        // Step 2.8: "If global is a Window object:"
        // Note: The content process does not yet model callback realms or Window.currentEvent tracking.

        // Step 2.9: "If listener's passive is true, then set event's in passive listener flag."
        if listener.passive == Some(true) {
            try_with_event_mut(
                &<Types as JsTypes>::value_from_object(event.clone()),
                host.ec(),
                |inner| {
                    inner.in_passive_listener_flag = true;
                },
            )?;
        }

        // Step 2.10: "If global is a Window object, then record timing info for event listener given event and listener."
        // Note: The content process does not yet record per-listener performance timing.

        // Step 2.11: "Call a user object's operation with listener's callback, `handleEvent`, « event », and event's currentTarget attribute value."
        if let Some(callback) = listener.callback.as_ref() {
            if let Err(error) = call_user_objects_operation(
                host.ec(),
                callback,
                "handleEvent",
                &[<Types as JsTypes>::value_from_object(event.clone())],
                Some(&<Types as JsTypes>::value_from_object(
                    current_target.clone(),
                )),
            ) {
                host.report_exception(error);
            }
        }

        // Step 2.12: "Unset event's in passive listener flag."
        try_with_event_mut(
            &<Types as JsTypes>::value_from_object(event.clone()),
            host.ec(),
            |inner| {
                inner.in_passive_listener_flag = false;
            },
        )?;

        // Step 2.13: "If global is a Window object, then set global's current event to currentEvent."
        // Note: The content process does not yet model Window.currentEvent restoration.

        // Step 2.14: "If event's stop immediate propagation flag is set, then break."
        if stop_immediate(host, event)? {
            break;
        }
    }

    // Step 3: "Return found."
    Ok(found)
}

fn shadow_adjusted_target(path: &[EventPathEntry], index: usize) -> Option<JsObject> {
    path[..=index]
        .iter()
        .rev()
        .find_map(|entry| entry.shadow_adjusted_target.clone())
}

fn listener_is_active(
    host: &mut impl EventDispatchHost,
    target: &JsObject,
    listener_id: u64,
) -> Completion<bool, Types> {
    try_with_event_target_ref(
        &<Types as JsTypes>::value_from_object(target.clone()),
        host.ec(),
        |event_target| event_target.listener_is_active(listener_id),
    )
}

fn set_event_target_state(
    host: &mut impl EventDispatchHost,
    event: &JsObject,
    target: Option<JsObject>,
    current_target: Option<JsObject>,
    phase: u16,
) -> Completion<(), Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| {
            inner.target = target;
            inner.current_target = current_target;
            inner.event_phase = phase;
        },
    )
}

fn stop_propagation(
    host: &mut impl EventDispatchHost,
    event: &JsObject,
) -> Completion<bool, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.stop_propagation_flag,
    )
}

fn stop_immediate(host: &mut impl EventDispatchHost, event: &JsObject) -> Completion<bool, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.stop_immediate_propagation_flag,
    )
}

fn bubbles(host: &mut impl EventDispatchHost, event: &JsObject) -> Completion<bool, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.bubbles,
    )
}

fn canceled(host: &mut impl EventDispatchHost, event: &JsObject) -> Completion<bool, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.canceled_flag,
    )
}

fn event_phase(host: &mut impl EventDispatchHost, event: &JsObject) -> Completion<u16, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.event_phase,
    )
}

fn event_type(host: &mut impl EventDispatchHost, event: &JsObject) -> Completion<String, Types> {
    try_with_event_mut(
        &<Types as JsTypes>::value_from_object(event.clone()),
        host.ec(),
        |inner| inner.type_.clone(),
    )
}

fn is_activation_event(
    host: &mut impl EventDispatchHost,
    event: &JsObject,
) -> Completion<bool, Types> {
    Ok(event_type(host, event)? == "click")
}
