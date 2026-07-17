use log::trace;

use super::event::EventTargetAccess;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::call_user_objects_operation;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::BUBBLING_PHASE;
use super::CAPTURING_PHASE;
use super::event::{Event, EventListener, EventTarget, NONE};

fn dispatch_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

/// <https://dom.spec.whatwg.org/#event-path-item>
#[derive(Clone)]
pub(crate) struct EventPathItem {
    /// <https://dom.spec.whatwg.org/#event-path-invocation-target>
    pub(crate) invocation_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#event-path-shadow-adjusted-target>
    pub(crate) shadow_adjusted_target: Option<EventTarget>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ListenerPhase {
    Capturing,
    Bubbling,
}

pub(crate) fn simple_path(
    target_access: &dyn super::event::EventTargetAccess,
) -> Vec<EventPathItem> {
    vec![EventPathItem {
        invocation_target: target_access.get_event_target(),
        shadow_adjusted_target: Some(target_access.get_event_target()),
    }]
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
pub(crate) fn fire_event(
    ec: &mut dyn ExecutionContext<Types>,
    target: &dyn super::event::EventTargetAccess,
    event_type: &str,
    time_millis: f64,
    legacy_target_override: bool,
) -> Completion<bool, Types> {

    // Step 1: If eventConstructor is not given, then let eventConstructor be Event.
    // (Event is always used for this code path.)
    // Step 2: Let event be the result of creating an event given eventConstructor,
    // in the relevant realm of target.
    let event_domain = Event::new(
        event_type.to_owned(),

        // Step 3: Initialize event's type attribute to e.
        // Step 4: Initialize any other IDL attributes of event...
        false,  // bubbles
        false,  // cancelable
        false,  // composed
        true,   // isTrusted
        time_millis,
    );
    let event_object =
        create_interface_instance::<Types, Event>(event_domain, ec)?;
    // Clone the Event domain object from the JsObject — GcCell fields share
    // data, and the reflector was set automatically by create_interface_instance.
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>())
        .cloned()
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;

    // Step 5: Return the result of dispatching event at target, with
    // legacy target override flag set if set.
    let path = build_path_for_target(target, legacy_target_override);
    dispatch_event(ec, &path, &event)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_with_path(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
    event: &Event,
) -> Completion<bool, Types> {
    dispatch_event(ec, path, event)
}

/// <https://dom.spec.whatwg.org/#concept-event-path-append>
fn append_to_event_path(
    path: &mut Vec<EventPathItem>,
    invocation_target: EventTarget,
    shadow_adjusted_target: Option<EventTarget>,
) {

    // Step 1: Let invocationTargetInShadowTree be false.
    // Step 3: Let rootOfClosedTree be false.
    // (Shadow tree fields are not yet modeled; always false.)
    // Step 5: Append a new event path item to event's path whose
    // invocation target is invocationTarget,
    // shadow-adjusted target is shadowAdjustedTarget, ...
    path.push(EventPathItem {
        invocation_target,
        shadow_adjusted_target,
    });
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
fn build_path_for_target(
    target_access: &dyn super::event::EventTargetAccess,
    _legacy_target_override: bool,
) -> Vec<EventPathItem> {
    let mut path: Vec<EventPathItem> = Vec::new();

    // Step 6.3: Append to an event path with event, target, targetOverride,
    // relatedTarget, touchTargets, and false.
    // Note: targetOverride, relatedTarget, and touchTargets are not yet modeled.
    let et = target_access.get_event_target();
    append_to_event_path(&mut path, et.clone(), Some(et));

    // Step 6.6: Let slottable be target, if target is a slottable…
    // Step 6.7: Let slotInClosedTree be false.
    // (Not yet modeled.)
    // Step 6.8: Let parent be the result of invoking target's get the parent with event.
    let mut parent = target_access.get_the_parent();

    // Step 6.9: While parent is non-null:
    while let Some(parent_target) = parent {

        // Step 6.9.6-6.9.8: Append to an event path with event, parent, …
        append_to_event_path(&mut path, parent_target.clone(), None);

        // Step 6.9.9: If parent is non-null, then set parent to the result of
        // invoking parent's get the parent with event.
        parent = parent_target.get_the_parent();
    }

    path
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
pub(crate) fn dispatch_event(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
    event: &Event,
) -> Completion<bool, Types> {

    // Step 1: Set event's dispatch flag.
    *event.dispatch_flag.borrow_mut() = true;

    // Step 3: Let activationTarget be null.
    // Step 6.5: If isActivationEvent is true and target has activation behavior,
    //           then set activationTarget to target.
    let activation_target_idx = if event.type_ == "click" {
        // Only check the first path entry (the target per step 6.3).
        path.first()
            .filter(|entry| entry.invocation_target.has_activation_behavior())
            .map(|_| 0)
    } else {
        None
    };

    // Step 6.13: For each item of event's path, in reverse order (capturing phase).
    for (index, entry) in path.iter().enumerate().rev() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else {
            CAPTURING_PHASE
        };

        // Step 6.13.1-2: Set event's eventPhase.
        *event.event_phase.borrow_mut() = phase;

        // Step 6.13.3: Invoke with item, event, "capturing".
        invoke(ec, path, index, event, ListenerPhase::Capturing)?;
    }

    // Step 6.14: For each item of event's path (forward — bubbling phase).
    for (index, entry) in path.iter().enumerate() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else if *event.bubbles.borrow() {
            BUBBLING_PHASE
        } else {

            // Step 6.14.2.1: If event's bubbles attribute is false, then continue.
            continue;
        };

        // Step 6.14.1-2: Set event's eventPhase.
        *event.event_phase.borrow_mut() = phase;

        // Step 6.14.3: Invoke with item, event, "bubbling".
        invoke(ec, path, index, event, ListenerPhase::Bubbling)?;
    }

    let canceled = *event.canceled_flag.borrow();

    // Step 7: Set event's eventPhase attribute to NONE.
    *event.event_phase.borrow_mut() = NONE;

    // Step 8: Set event's currentTarget attribute to null.
    *event.current_target.borrow_mut() = None;

    // Step 9: Set event's path to the empty list. (Not stored on Event yet.)
    // Step 10: Unset event's dispatch flag, stop propagation flag, and
    //          stop immediate propagation flag.
    *event.dispatch_flag.borrow_mut() = false;
    *event.stop_propagation_flag.borrow_mut() = false;
    *event.stop_immediate_propagation_flag.borrow_mut() = false;

    // Step 12: If activationTarget is non-null:
    if let Some(idx) = activation_target_idx {

        // Step 12.1: If event's canceled flag is unset, then run
        //            activationTarget's activation behavior with event.
        if !canceled {
            path[idx].invocation_target.run_activation_behavior(event)?;
        }
    }

    // Step 13: Return false if event's canceled flag is set; otherwise true.
    Ok(!canceled)
}

/// <https://dom.spec.whatwg.org/#concept-event-listener-invoke>
fn invoke(
    ec: &mut dyn ExecutionContext<Types>,
    path: &[EventPathItem],
    index: usize,
    event: &Event,
    phase: ListenerPhase,
) -> Completion<(), Types> {
    let entry = &path[index];

    // Step 1: Let targetItem be pathItem.
    // Step 2: While targetItem's shadow-adjusted target is null:
    //   set targetItem to the event path item preceding targetItem in event's path.
    let target_item = path[..=index]
        .iter()
        .rev()
        .find(|item| item.shadow_adjusted_target.is_some());
    let target = target_item.and_then(|item| item.shadow_adjusted_target.clone());

    // Step 3: Set event's target to targetItem's shadow-adjusted target.
    *event.target.borrow_mut() = target;

    // Step 4: Set event's relatedTarget to pathItem's relatedTarget.
    // TODO: relatedTarget is not yet modeled.
    // Step 5: Set event's touch target list to pathItem's touch target list.
    // TODO: touch target list is not yet modeled.

    // Step 6: If event's stop propagation flag is set, then return.
    if *event.stop_propagation_flag.borrow() {
        return Ok(());
    }

    // Step 7: Initialize event's currentTarget attribute to pathItem's invocation target.
    *event.current_target.borrow_mut() = Some(entry.invocation_target.clone());

    // Step 8: Let listeners be a clone of event's currentTarget attribute value's event listener list.
    let listeners = entry.invocation_target.event_listener_list.borrow().clone();

    // Step 9: Let invocationTargetInShadowTree be pathItem's invocation-target-in-shadow-tree.
    // TODO: Shadow tree is not yet modeled.
    // Step 10: Let found be the result of running inner invoke with event,
    // listeners, phase, invocationTargetInShadowTree, and
    // legacyOutputDidListenersThrowFlag if given.
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
            "[input-debug][dispatch] phase={} listeners={} matching_click_listeners={}",
            phase_name,
            listeners.len(),
            matching_listeners,
        );
    }

    // Step 10: Let found be the result of inner invoke.
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

    // Step 1: Let found be false.
    let mut found = false;

    // Step 2: For each listener of listeners, whose removed is false:
    for listener in listeners.iter().filter(|listener| !listener.removed) {

        // Step 2.1: If event's type attribute value is not listener's type, continue.
        if event.type_ != listener.type_ {
            continue;
        }

        // Step 2.2: Set found to true.
        found = true;

        // Step 2.3: If phase is "capturing" and listener's capture is false, continue.
        if phase == ListenerPhase::Capturing && !listener.capture {
            continue;
        }

        // Step 2.4: If phase is "bubbling" and listener's capture is true, continue.
        if phase == ListenerPhase::Bubbling && listener.capture {
            continue;
        }

        // Step 2.5: If listener's once is true, then remove an event listener.
        if listener.once {
            current_target.remove_event_listener_by_id(listener.id);
        }

        // Step 2.9: If listener's passive is true, set event's in passive listener flag.
        if listener.passive == Some(true) {
            *event.in_passive_listener_flag.borrow_mut() = true;
        }

        // Step 2.11: Call a user object's operation with listener's callback,
        //            "handleEvent", « event », and event's currentTarget attribute value.
        if let Some(callback) = listener.callback.as_ref() {
            // Get the Event JsObject from its reflector.
            if let Some(event_js) = event.reflector.as_ref().cloned() {
                let event_value = <Types as JsTypes>::value_from_object(event_js);
                // Get the currentTarget JsObject from its reflector, or use undefined.
                // <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
                // Step 2: "If thisArg was not given, let thisArg be undefined."
                let this_value = current_target
                    .reflector
                    .as_ref()
                    .map(|obj| <Types as JsTypes>::value_from_object(obj.clone()));
                if let Err(error) = call_user_objects_operation(
                    ec,
                    callback,
                    "handleEvent",
                    &[event_value],
                    this_value.as_ref(),
                ) {
                    ec.report_exception(error);
                }
            }
        }

        // Step 2.12: Unset event's in passive listener flag.
        *event.in_passive_listener_flag.borrow_mut() = false;

        // Step 2.14: If event's stop immediate propagation flag is set, break.
        if *event.stop_immediate_propagation_flag.borrow() {
            break;
        }
    }

    // Step 3: Return found.
    Ok(found)
}
