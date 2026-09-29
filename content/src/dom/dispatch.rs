use super::event::EventTargetAccess;
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::call_user_objects_operation;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::BUBBLING_PHASE;
use super::CAPTURING_PHASE;
use super::event::{Event, EventListener, EventTarget, HasEvent, NONE};
use crate::webidl::bindings::WebIdlInterface;

/// <https://dom.spec.whatwg.org/#event-path-item>
#[derive(Clone)]
pub(crate) struct EventPathItem {
    /// <https://dom.spec.whatwg.org/#event-path-invocation-target>
    pub(crate) invocation_target: EventTarget,

    /// <https://dom.spec.whatwg.org/#event-path-shadow-adjusted-target>
    pub(crate) shadow_adjusted_target: Option<EventTarget>,

    /// <https://dom.spec.whatwg.org/#eventtarget-activation-behavior>
    pub(crate) has_activation_behavior: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ListenerPhase {
    Capturing,
    Bubbling,
}

pub(crate) fn simple_path(
    target_access: &dyn super::event::EventTargetAccess,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    vec![EventPathItem {
        invocation_target: target_access.get_event_target(ec),
        shadow_adjusted_target: Some(target_access.get_event_target(ec)),
        has_activation_behavior: false,
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
        false, // bubbles
        false, // cancelable
        false, // composed
        true,  // isTrusted
        time_millis,
        ec,
    );
    let event_object = create_interface_instance::<Types, Event>(event_domain, ec)?;
    // Clone the Event domain object from the JsObject — GcCell fields share
    // data, and the reflector was set automatically by create_interface_instance.
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<Event>())
        .cloned()
        .ok_or_else(|| ec.new_type_error("event_object is not an Event"))?;

    // Step 5: Return the result of dispatching event at target, with
    // legacy target override flag set if set.
    let path = build_path_for_target(target, legacy_target_override, ec);
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
        has_activation_behavior: false,
    });
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
fn build_path_for_target(
    target_access: &dyn super::event::EventTargetAccess,
    _legacy_target_override: bool,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    let mut path: Vec<EventPathItem> = Vec::new();

    // Step 6.3: Append to an event path with event, target, targetOverride,
    // relatedTarget, touchTargets, and false.
    // Note: targetOverride, relatedTarget, and touchTargets are not yet modeled.
    let et = target_access.get_event_target(ec);
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
    *event.dispatch_flag.borrow_mut(ec) = true;

    // Step 3: Let activationTarget be null.
    // Step 6.5: If isActivationEvent is true and target has activation behavior,
    //           then set activationTarget to target.
    // Step 6.9.6.1: If isActivationEvent is true, event's bubbles attribute is
    //               true, activationTarget is null, and parent has activation
    //               behavior, then set activationTarget to parent.
    let activation_target_idx = if event.type_ == "click" {
        path.iter().position(|entry| entry.has_activation_behavior)
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
        *event.event_phase.borrow_mut(ec) = phase;

        // Step 6.13.3: Invoke with item, event, "capturing".
        invoke(ec, path, index, event, ListenerPhase::Capturing)?;
    }

    // Step 6.14: For each item of event's path (forward — bubbling phase).
    for (index, entry) in path.iter().enumerate() {
        let phase = if entry.shadow_adjusted_target.is_some() {
            super::AT_TARGET
        } else if *event.bubbles.borrow(ec) {
            BUBBLING_PHASE
        } else {
            // Step 6.14.2.1: If event's bubbles attribute is false, then continue.
            continue;
        };

        // Step 6.14.1-2: Set event's eventPhase.
        *event.event_phase.borrow_mut(ec) = phase;

        // Step 6.14.3: Invoke with item, event, "bubbling".
        invoke(ec, path, index, event, ListenerPhase::Bubbling)?;
    }

    let canceled = *event.canceled_flag.borrow(ec);

    // Step 7: Set event's eventPhase attribute to NONE.
    *event.event_phase.borrow_mut(ec) = NONE;

    // Step 8: Set event's currentTarget attribute to null.
    *event.current_target.borrow_mut(ec) = None;

    // Step 9: Set event's path to the empty list. (Not stored on Event yet.)
    // Step 10: Unset event's dispatch flag, stop propagation flag, and
    //          stop immediate propagation flag.
    *event.dispatch_flag.borrow_mut(ec) = false;
    *event.stop_propagation_flag.borrow_mut(ec) = false;
    *event.stop_immediate_propagation_flag.borrow_mut(ec) = false;

    // Step 12: If activationTarget is non-null:
    if activation_target_idx.is_some() {
        // Step 12.1: If event's canceled flag is unset, then run
        //            activationTarget's activation behavior with event.
        if !canceled {
            // The activation behavior lives in the JS layer (it resolves the
            // element from the path item's reflector and needs the realm's
            // global scope for the navigation context).
            crate::js::platform_objects::run_activation_behavior_for_path(ec, path)?;
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
    *event.target.borrow_mut(ec) = target;

    // Step 4: Set event's relatedTarget to pathItem's relatedTarget.
    // TODO: relatedTarget is not yet modeled.
    // Step 5: Set event's touch target list to pathItem's touch target list.
    // TODO: touch target list is not yet modeled.

    // Step 6: If event's stop propagation flag is set, then return.
    if *event.stop_propagation_flag.borrow(ec) {
        return Ok(());
    }

    // Step 7: Initialize event's currentTarget attribute to pathItem's invocation target.
    *event.current_target.borrow_mut(ec) = Some(entry.invocation_target.clone());

    // Step 8: Let listeners be a clone of event's currentTarget attribute value's event listener list.
    let listeners = entry
        .invocation_target
        .event_listener_list
        .borrow(ec)
        .clone();

    // Step 9: Let invocationTargetInShadowTree be pathItem's invocation-target-in-shadow-tree.
    // TODO: Shadow tree is not yet modeled.
    // Step 10: Let found be the result of running inner invoke with event,
    // listeners, phase, invocationTargetInShadowTree, and
    // legacyOutputDidListenersThrowFlag if given.
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
            current_target.remove_event_listener_by_id(listener.id, ec);
        }

        // Step 2.9: If listener's passive is true, set event's in passive listener flag.
        if listener.passive == Some(true) {
            *event.in_passive_listener_flag.borrow_mut(ec) = true;
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
        *event.in_passive_listener_flag.borrow_mut(ec) = false;

        // Step 2.14: If event's stop immediate propagation flag is set, break.
        if *event.stop_immediate_propagation_flag.borrow(ec) {
            break;
        }
    }

    // Step 3: Return found.
    Ok(found)
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
/// Fire an event given as a constructed Event subclass (steps 3 and 4 of the
/// fire algorithm ran when the caller built `event_data`).
pub(crate) fn fire_event_using<E>(
    target: &EventTarget,
    event_data: E,
    time_millis: f64,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types>
where
    E: HasEvent
        + WebIdlInterface<Types>
        + Clone
        + js_engine::gc::Trace
        + js_engine::gc::Finalize
        + 'static,
{
    // Step 2: Let event be the result of creating an event given
    //         eventConstructor, in the relevant realm of target.
    // Note: Creating the event also initializes its isTrusted attribute to
    // true and its timeStamp attribute to the time of the occurrence.
    let event_object = create_interface_instance::<Types, E>(event_data, ec)?;
    let event: Event = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<E>().map(|event| event.event().clone()))
        .ok_or_else(|| ec.new_type_error("event object is not the expected Event subclass"))?;
    *event.is_trusted.borrow_mut(ec) = true;
    *event.time_stamp.borrow_mut(ec) = time_millis;
    // Step 5: Return the result of dispatching event at target, with legacy
    //         target override flag set if set.
    let path = simple_path(target, ec);
    dispatch_with_path(ec, &path, &event)
}
