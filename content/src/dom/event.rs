use blitz_traits::events::{DomEvent, EventState};

use crate::js::Types;
use crate::webidl::Callback;
use std::cell::Cell;
use js_engine::JsTypes;
use js_engine::gc_struct;
use js_engine::gc::{GcCell, gc_cell_new};

use super::{AbortAlgorithm, AbortSignal};

type JsObject = <Types as JsTypes>::JsObject;

pub const NONE: u16 = 0;
pub const CAPTURING_PHASE: u16 = 1;
pub const AT_TARGET: u16 = 2;
pub const BUBBLING_PHASE: u16 = 3;

/// <https://dom.spec.whatwg.org/#concept-event-listener>
#[gc_struct]
pub(crate) struct EventListener {
    #[ignore_trace]
    pub id: u64,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-type>
    #[ignore_trace]
    pub type_: String,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-callback>
    pub callback: Option<Callback>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-capture>
    #[ignore_trace]
    pub capture: bool,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-passive>
    #[ignore_trace]
    pub passive: Option<bool>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-once>
    #[ignore_trace]
    pub once: bool,

    /// <https://dom.spec.whatwg.org/#event-listener-signal>
    // Note: Spec-defined slot, not yet wired to AbortSignal-backed removal.
    #[allow(dead_code)]
    pub signal: Option<AbortSignal>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-removed>
    #[ignore_trace]
    pub removed: bool,
}

/// <https://dom.spec.whatwg.org/#interface-eventtarget>
#[gc_struct]
#[derive(Default)]
pub struct EventTarget {
    /// The JsObject GC handle for the platform object that owns this
    /// EventTarget (Window, Node, AbortSignal, etc.).
    /// Set during path building so the Web IDL layer can retrieve the
    /// JsObject for callback invocation without storing it separately.
    pub(crate) reflector: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#eventtarget-event-listener-list>
    pub(crate) event_listener_list: GcCell<Vec<EventListener>>,

    #[ignore_trace]
    next_listener_id: Cell<u64>,
}

/// Trait for types that embed or are associated with an EventTarget.
pub(crate) trait EventTargetAccess {
    fn get_event_target(&self) -> EventTarget;

    /// The JsObject GC handle for this EventTarget's platform object.
    /// Default implementation uses the EventTarget's reflector field.
    fn get_target_object(&self) -> Option<JsObject> {
        self.get_event_target().reflector.clone()
    }

    /// Returns (parent_JsObject, parent_EventTarget) or None.
    /// <https://dom.spec.whatwg.org/#dom-eventtarget-gettheparent>
    fn get_the_parent(&self) -> Option<(JsObject, EventTarget)> {
        None
    }
}

impl EventTargetAccess for EventTarget {
    fn get_event_target(&self) -> EventTarget {
        self.clone()
    }

    fn get_target_object(&self) -> Option<JsObject> {
        self.reflector.clone()
    }
}

impl EventTarget {
    /// Set the JsObject reflector for this EventTarget.
    /// Called when the platform object that embeds this EventTarget
    /// is created (e.g. after `create_interface_instance`).
    pub(crate) fn set_reflector(&mut self, reflector: JsObject) {
        self.reflector = Some(reflector);
    }

    /// <https://dom.spec.whatwg.org/#dom-eventtarget-addeventlistener>
    pub(crate) fn add_event_listener(
        &self,
        event_target: EventTarget,
        type_: String,
        callback: Option<Callback>,
        capture: bool,
        once: bool,
        passive: Option<bool>,
        signal: Option<AbortSignal>,
    ) {
        if let Some(signal) = signal.as_ref() {
            if signal.aborted_value() {
                return;
            }
        }

        let Some(callback) = callback else {
            return;
        };

        let passive = passive.or(Some(false));
        let listener_id = self.next_listener_id.get().wrapping_add(1);
        let mut listeners = self.event_listener_list.borrow_mut();
        let duplicate = listeners.iter().any(|listener| {
            listener.type_ == type_
                && listener.capture == capture
                && listener
                    .callback
                    .as_ref()
                    .is_some_and(|existing| existing.equals(&callback))
        });

        if !duplicate {
            self.next_listener_id.set(listener_id);
            listeners.push(EventListener {
                id: listener_id,
                type_,
                callback: Some(callback),
                capture,
                passive,
                once,
                signal: signal.clone(),
                removed: false,
            });
            // Drop the borrow before signal.add_abort_algorithm (which may borrow ec).
            std::mem::drop(listeners);

            if let Some(signal) = signal {
                signal.add_abort_algorithm(AbortAlgorithm::RemoveEventListener {
                    event_target: event_target.clone(),
                    listener_id,
                });
            }
        }
    }

    /// <https://dom.spec.whatwg.org/#remove-an-event-listener>
    pub(crate) fn remove_event_listener_entry(
        &self,
        type_: &str,
        callback: &Callback,
        capture: bool,
    ) {
        let mut listeners = self.event_listener_list.borrow_mut();
        for listener in listeners.iter_mut() {
            if listener.type_ == type_
                && listener.capture == capture
                && listener
                    .callback
                    .as_ref()
                    .is_some_and(|existing| existing.equals(callback))
            {
                listener.removed = true;
            }
        }

        listeners.retain(|listener| !listener.removed);
    }

    pub(crate) fn remove_event_listener_by_id(&self, listener_id: u64) {
        let mut listeners = self.event_listener_list.borrow_mut();
        for listener in listeners.iter_mut() {
            if listener.id == listener_id {
                listener.removed = true;
            }
        }

        listeners.retain(|listener| !listener.removed);
    }

    // Note: Defined by the spec but not yet used by the current dispatch code.
    // <https://dom.spec.whatwg.org/#concept-event-listener>
    #[allow(dead_code)]
    pub(crate) fn listener_is_active(&self, listener_id: u64) -> bool {
        self.event_listener_list
            .borrow()
            .iter()
            .any(|listener| listener.id == listener_id && !listener.removed)
    }
}

/// <https://dom.spec.whatwg.org/#event>
#[gc_struct]
pub struct Event {
    /// <https://dom.spec.whatwg.org/#dom-event-type>
    #[ignore_trace]
    pub type_: String,

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub target: GcCell<Option<JsObject>>,

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub current_target: GcCell<Option<JsObject>>,

    /// <https://dom.spec.whatwg.org/#dom-event-eventphase>
    pub event_phase: GcCell<u16>,

    /// <https://dom.spec.whatwg.org/#dom-event-bubbles>
    pub bubbles: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#dom-event-cancelable>
    pub cancelable: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#dom-event-composed>
    pub composed: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    pub is_trusted: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    pub time_stamp: GcCell<f64>,

    /// <https://dom.spec.whatwg.org/#event>
    pub stop_propagation_flag: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#event>
    pub stop_immediate_propagation_flag: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#dom-event-defaultprevented>
    pub canceled_flag: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#event>
    pub in_passive_listener_flag: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#event>
    pub dispatch_flag: GcCell<bool>,

    /// <https://dom.spec.whatwg.org/#event>
    pub initialized_flag: GcCell<bool>,
}

impl Event {
    pub fn new(
        type_: String,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
        is_trusted: bool,
        time_stamp: f64,
    ) -> Self {
        Self {
            type_,
            target: gc_cell_new(None),
            current_target: gc_cell_new(None),
            event_phase: gc_cell_new(NONE),
            bubbles: gc_cell_new(bubbles),
            cancelable: gc_cell_new(cancelable),
            composed: gc_cell_new(composed),
            is_trusted: gc_cell_new(is_trusted),
            time_stamp: gc_cell_new(time_stamp),
            stop_propagation_flag: gc_cell_new(false),
            stop_immediate_propagation_flag: gc_cell_new(false),
            canceled_flag: gc_cell_new(false),
            in_passive_listener_flag: gc_cell_new(false),
            dispatch_flag: gc_cell_new(false),
            initialized_flag: gc_cell_new(true),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-type>
    pub(crate) fn type_value(&self) -> &str {
        &self.type_
    }

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub(crate) fn target_value(&self) -> Option<JsObject> {
        self.target.borrow().clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub(crate) fn current_target_value(&self) -> Option<JsObject> {
        self.current_target.borrow().clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-eventphase>
    pub(crate) fn event_phase_value(&self) -> u16 {
        *self.event_phase.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-bubbles>
    pub(crate) fn bubbles_value(&self) -> bool {
        *self.bubbles.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelable>
    pub(crate) fn cancelable_value(&self) -> bool {
        *self.cancelable.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-defaultprevented>
    pub(crate) fn default_prevented(&self) -> bool {
        *self.canceled_flag.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn cancel_bubble(&self) -> bool {
        *self.stop_propagation_flag.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn set_cancel_bubble(&self, value: bool) {
        if value {
            *self.stop_propagation_flag.borrow_mut() = true;
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    pub(crate) fn is_trusted(&self) -> bool {
        *self.is_trusted.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    pub(crate) fn time_stamp_value(&self) -> f64 {
        *self.time_stamp.borrow()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stoppropagation>
    pub(crate) fn stop_propagation(&self) {
        *self.stop_propagation_flag.borrow_mut() = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stopimmediatepropagation>
    pub(crate) fn stop_immediate_propagation(&self) {
        *self.stop_propagation_flag.borrow_mut() = true;
        *self.stop_immediate_propagation_flag.borrow_mut() = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-preventdefault>
    pub(crate) fn prevent_default(&self) {
        if *self.cancelable.borrow() && !*self.in_passive_listener_flag.borrow() {
            *self.canceled_flag.borrow_mut() = true;
        }
    }
}

/// <https://w3c.github.io/uievents/#interface-uievent>
#[gc_struct]
pub struct UIEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub event: Event,

    /// <https://w3c.github.io/uievents/#dom-uievent-view>
    pub view: Option<JsObject>,

    /// <https://w3c.github.io/uievents/#dom-uievent-detail>
    #[ignore_trace]
    pub detail: i32,
}

impl UIEvent {
    pub fn from_dom_event(dom_event: &DomEvent, view: Option<JsObject>, time_stamp: f64) -> Self {
        Self {
            event: Event::new(
                dom_event.name().to_owned(),
                dom_event.bubbles,
                dom_event.cancelable,
                false,
                true,
                time_stamp,
            ),
            view,
            detail: 0,
        }
    }

    /// <https://w3c.github.io/uievents/#dom-uievent-view>
    pub(crate) fn view_value(&self) -> Option<JsObject> {
        self.view.clone()
    }

    /// <https://w3c.github.io/uievents/#dom-uievent-detail>
    pub(crate) fn detail_value(&self) -> i32 {
        self.detail
    }

    pub fn apply_to_event_state(&self, event_state: &mut EventState) {
        if *self.event.canceled_flag.borrow() {
            event_state.prevent_default();
        }
    }
}
