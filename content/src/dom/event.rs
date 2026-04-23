use blitz_traits::events::{DomEvent, EventState};
use boa_engine::{JsData, JsResult, object::JsObject};
use boa_gc::{Finalize, Trace};

use super::{AbortAlgorithm, AbortSignal};

pub const NONE: u16 = 0;
pub const CAPTURING_PHASE: u16 = 1;
pub const AT_TARGET: u16 = 2;
pub const BUBBLING_PHASE: u16 = 3;

/// <https://dom.spec.whatwg.org/#concept-event-listener>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct EventListener {
    #[unsafe_ignore_trace]
    pub id: u64,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-type>
    #[unsafe_ignore_trace]
    pub type_: String,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-callback>
    pub callback: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-capture>
    #[unsafe_ignore_trace]
    pub capture: bool,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-passive>
    #[unsafe_ignore_trace]
    pub passive: Option<bool>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-once>
    #[unsafe_ignore_trace]
    pub once: bool,

    /// <https://dom.spec.whatwg.org/#event-listener-signal>
    pub signal: Option<AbortSignal>,

    /// <https://dom.spec.whatwg.org/#concept-event-listener-removed>
    #[unsafe_ignore_trace]
    pub removed: bool,
}

/// <https://dom.spec.whatwg.org/#interface-eventtarget>
#[derive(Default, Trace, Finalize, JsData)]
pub struct EventTarget {
    /// <https://dom.spec.whatwg.org/#eventtarget-event-listener-list>
    pub event_listener_list: Vec<EventListener>,

    #[unsafe_ignore_trace]
    next_listener_id: u64,
}

impl EventTarget {
    /// <https://dom.spec.whatwg.org/#dom-eventtarget-addeventlistener>
    pub(crate) fn add_event_listener(
        &mut self,
        event_target: &JsObject,
        type_: String,
        callback: Option<JsObject>,
        capture: bool,
        once: bool,
        passive: Option<bool>,
        signal: Option<AbortSignal>,
    ) -> JsResult<()> {
        if let Some(signal) = signal.as_ref() {
            if signal.aborted_value() {
                return Ok(());
            }
        }

        let Some(callback) = callback else {
            return Ok(());
        };

        let passive = passive.or(Some(false));
        let listener_id = self.next_listener_id.wrapping_add(1);
        let duplicate = self.event_listener_list.iter().any(|listener| {
            listener.type_ == type_
                && listener.capture == capture
                && listener
                    .callback
                    .as_ref()
                    .is_some_and(|existing| JsObject::equals(existing, &callback))
        });

        if !duplicate {
            self.next_listener_id = listener_id;
            self.event_listener_list.push(EventListener {
                id: listener_id,
                type_,
                callback: Some(callback),
                capture,
                passive,
                once,
                signal: signal.clone(),
                removed: false,
            });

            if let Some(signal) = signal {
                signal.add_abort_algorithm(AbortAlgorithm::RemoveEventListener {
                    event_target: event_target.clone(),
                    listener_id,
                });
            }
        }

        Ok(())
    }

    /// <https://dom.spec.whatwg.org/#remove-an-event-listener>
    pub(crate) fn remove_event_listener_entry(
        &mut self,
        type_: &str,
        callback: &JsObject,
        capture: bool,
    ) {
        for listener in &mut self.event_listener_list {
            if listener.type_ == type_
                && listener.capture == capture
                && listener
                    .callback
                    .as_ref()
                    .is_some_and(|existing| JsObject::equals(existing, callback))
            {
                listener.removed = true;
            }
        }

        self.event_listener_list
            .retain(|listener| !listener.removed);
    }

    pub(crate) fn remove_event_listener_by_id(&mut self, listener_id: u64) {
        for listener in &mut self.event_listener_list {
            if listener.id == listener_id {
                listener.removed = true;
            }
        }

        self.event_listener_list
            .retain(|listener| !listener.removed);
    }

    pub(crate) fn listener_is_active(&self, listener_id: u64) -> bool {
        self.event_listener_list
            .iter()
            .any(|listener| listener.id == listener_id && !listener.removed)
    }
}

/// <https://dom.spec.whatwg.org/#event>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct Event {
    /// <https://dom.spec.whatwg.org/#dom-event-type>
    #[unsafe_ignore_trace]
    pub type_: String,

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub target: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub current_target: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#dom-event-eventphase>
    #[unsafe_ignore_trace]
    pub event_phase: u16,

    /// <https://dom.spec.whatwg.org/#dom-event-bubbles>
    #[unsafe_ignore_trace]
    pub bubbles: bool,

    /// <https://dom.spec.whatwg.org/#dom-event-cancelable>
    #[unsafe_ignore_trace]
    pub cancelable: bool,

    /// <https://dom.spec.whatwg.org/#dom-event-composed>
    #[unsafe_ignore_trace]
    pub composed: bool,

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    #[unsafe_ignore_trace]
    pub is_trusted: bool,

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    #[unsafe_ignore_trace]
    pub time_stamp: f64,

    /// <https://dom.spec.whatwg.org/#event>
    #[unsafe_ignore_trace]
    pub stop_propagation_flag: bool,

    /// <https://dom.spec.whatwg.org/#event>
    #[unsafe_ignore_trace]
    pub stop_immediate_propagation_flag: bool,

    /// <https://dom.spec.whatwg.org/#dom-event-defaultprevented>
    #[unsafe_ignore_trace]
    pub canceled_flag: bool,

    /// <https://dom.spec.whatwg.org/#event>
    #[unsafe_ignore_trace]
    pub in_passive_listener_flag: bool,

    /// <https://dom.spec.whatwg.org/#event>
    #[unsafe_ignore_trace]
    pub dispatch_flag: bool,

    /// <https://dom.spec.whatwg.org/#event>
    #[unsafe_ignore_trace]
    pub initialized_flag: bool,
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
            target: None,
            current_target: None,
            event_phase: NONE,
            bubbles,
            cancelable,
            composed,
            is_trusted,
            time_stamp,
            stop_propagation_flag: false,
            stop_immediate_propagation_flag: false,
            canceled_flag: false,
            in_passive_listener_flag: false,
            dispatch_flag: false,
            initialized_flag: true,
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-type>
    pub(crate) fn type_value(&self) -> &str {
        &self.type_
    }

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub(crate) fn target_value(&self) -> Option<JsObject> {
        self.target.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub(crate) fn current_target_value(&self) -> Option<JsObject> {
        self.current_target.clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-eventphase>
    pub(crate) fn event_phase_value(&self) -> u16 {
        self.event_phase
    }

    /// <https://dom.spec.whatwg.org/#dom-event-bubbles>
    pub(crate) fn bubbles_value(&self) -> bool {
        self.bubbles
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelable>
    pub(crate) fn cancelable_value(&self) -> bool {
        self.cancelable
    }

    /// <https://dom.spec.whatwg.org/#dom-event-defaultprevented>
    pub(crate) fn default_prevented(&self) -> bool {
        self.canceled_flag
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn cancel_bubble(&self) -> bool {
        self.stop_propagation_flag
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn set_cancel_bubble(&mut self, value: bool) {
        if value {
            self.stop_propagation_flag = true;
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    pub(crate) fn is_trusted(&self) -> bool {
        self.is_trusted
    }

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    pub(crate) fn time_stamp_value(&self) -> f64 {
        self.time_stamp
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stoppropagation>
    pub(crate) fn stop_propagation(&mut self) {
        self.stop_propagation_flag = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stopimmediatepropagation>
    pub(crate) fn stop_immediate_propagation(&mut self) {
        self.stop_propagation_flag = true;
        self.stop_immediate_propagation_flag = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-preventdefault>
    pub(crate) fn prevent_default(&mut self) {
        if self.cancelable && !self.in_passive_listener_flag {
            self.canceled_flag = true;
        }
    }
}

/// <https://w3c.github.io/uievents/#interface-uievent>
#[derive(Clone, Trace, Finalize, JsData)]
pub struct UIEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub event: Event,

    /// <https://w3c.github.io/uievents/#dom-uievent-view>
    pub view: Option<JsObject>,

    /// <https://w3c.github.io/uievents/#dom-uievent-detail>
    #[unsafe_ignore_trace]
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
        if self.event.canceled_flag {
            event_state.prevent_default();
        }
    }
}
