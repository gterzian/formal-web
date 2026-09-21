use crate::js::Types;
use crate::webidl::Callback;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};
use std::cell::Cell;

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
pub struct EventTarget {
    pub(crate) reflector: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#eventtarget-event-listener-list>
    pub(crate) event_listener_list: GcCell<Vec<EventListener>>,

    /// <https://html.spec.whatwg.org/#event-handler-map>
    event_handlers: GcCell<Vec<(String, Callback)>>,

    #[ignore_trace]
    next_listener_id: Cell<u64>,
}

impl EventTarget {
    pub(crate) fn new(ec: &mut dyn ExecutionContext<Types>) -> Self {
        Self {
            reflector: None,
            event_listener_list: gc_cell_new(Vec::new(), ec),
            event_handlers: gc_cell_new(Vec::new(), ec),
            next_listener_id: Cell::new(0),
        }
    }
}

impl EventTarget {
    /// <https://html.spec.whatwg.org/#getting-the-current-value-of-the-event-handler>
    pub(crate) fn event_handler_value(
        &self,
        type_: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<Callback> {
        self.event_handlers
            .borrow(ec)
            .iter()
            .find(|(handler_type, _)| handler_type == type_)
            .map(|(_, callback)| callback.clone())
    }

    /// <https://html.spec.whatwg.org/#event-handler-idl-attributes>
    pub(crate) fn set_event_handler_value(
        &self,
        type_: &str,
        callback: Option<Callback>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut handlers = self.event_handlers.borrow_mut(ec);
        handlers.retain(|(handler_type, _)| handler_type != type_);
        if let Some(callback) = callback {
            handlers.push((type_.to_owned(), callback));
        }
    }
}

/// Every Event platform-object type embeds the base `Event` (as a field or
/// through its parent chain); this trait exposes it so the JS layer can
/// downcast any Event subclass to its embedded `Event` in one place.
pub(crate) trait HasEvent {
    fn event(&self) -> &Event;

    fn event_mut(&mut self) -> &mut Event;
}

impl HasEvent for Event {
    fn event(&self) -> &Event {
        self
    }

    fn event_mut(&mut self) -> &mut Event {
        self
    }
}

pub(crate) trait EventTargetAccess {
    fn get_event_target(&self, ec: &mut dyn ExecutionContext<Types>) -> EventTarget;

    /// <https://dom.spec.whatwg.org/#dom-eventtarget-gettheparent>
    fn get_the_parent(&self) -> Option<EventTarget> {
        None
    }
}

impl EventTargetAccess for EventTarget {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.clone()
    }
}

/// <https://dom.spec.whatwg.org/#dictdef-addeventlisteneroptions>
#[derive(Clone, Default)]
pub(crate) struct AddEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
    pub signal: Option<AbortSignal>,
}

pub(crate) enum BooleanOrAddEventListenerOptions {
    Boolean(bool),
    Dict(AddEventListenerOptions),
}

/// <https://dom.spec.whatwg.org/#concept-flatten-options>
pub(crate) fn flatten(options: &BooleanOrAddEventListenerOptions) -> bool {
    match options {
        BooleanOrAddEventListenerOptions::Boolean(b) => *b,
        BooleanOrAddEventListenerOptions::Dict(d) => d.capture,
    }
}

/// <https://dom.spec.whatwg.org/#event-flatten-more>
pub(crate) fn flatten_more(options: BooleanOrAddEventListenerOptions) -> AddEventListenerOptions {
    match options {
        BooleanOrAddEventListenerOptions::Boolean(b) => AddEventListenerOptions {
            capture: b,
            once: false,
            passive: None,
            signal: None,
        },
        BooleanOrAddEventListenerOptions::Dict(d) => d,
    }
}

impl EventTarget {
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
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 2: If listener's signal is non-null and is aborted, then return.
        if let Some(signal) = signal.as_ref() {
            if signal.aborted_value(ec) {
                return;
            }
        }

        // Step 3: If listener's callback is null, then return.
        let Some(callback) = callback else {
            return;
        };

        // Step 4: If listener's passive is null, then set it to the
        // default passive value given listener's type and eventTarget.
        // Note: The default passive value algorithm is not yet implemented;
        // defaults to false for all types.
        let passive = passive.or(Some(false));

        // Step 5: If eventTarget's event listener list does not contain
        // an event listener whose type is listener's type, callback is
        // listener's callback, and capture is listener's capture, then
        // append listener to eventTarget's event listener list.
        let listener_id = self.next_listener_id.get().wrapping_add(1);
        // Test membership under a shared borrow so `equals` can use the
        // execution context; the mutable borrow is scoped to the append.
        let duplicate = {
            let listeners = self.event_listener_list.borrow(ec);
            listeners.iter().any(|listener| {
                listener.type_ == type_
                    && listener.capture == capture
                    && listener
                        .callback
                        .as_ref()
                        .is_some_and(|existing| existing.equals(&callback, ec))
            })
        };

        if !duplicate {
            self.next_listener_id.set(listener_id);
            self.event_listener_list.borrow_mut(ec).push(EventListener {
                id: listener_id,
                type_,
                callback: Some(callback),
                capture,
                passive,
                once,
                signal: signal.clone(),
                removed: false,
            });

            // Step 6: If listener's signal is non-null, then add the
            // following abort steps to it: Remove an event listener with
            // eventTarget and listener.
            if let Some(signal) = signal {
                signal.add_abort_algorithm(
                    AbortAlgorithm::RemoveEventListener {
                        event_target: event_target.clone(),
                        listener_id,
                    },
                    ec,
                );
            }
        }
    }

    /// <https://dom.spec.whatwg.org/#remove-an-event-listener>
    pub(crate) fn remove_event_listener_entry(
        &self,
        type_: &str,
        callback: &Callback,
        capture: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 2: Set listener's removed to true and remove listener from
        // eventTarget's event listener list.
        let matching_ids: Vec<u64> = {
            let listeners = self.event_listener_list.borrow(ec);
            listeners
                .iter()
                .filter(|listener| {
                    listener.type_ == type_
                        && listener.capture == capture
                        && listener
                            .callback
                            .as_ref()
                            .is_some_and(|existing| existing.equals(callback, ec))
                })
                .map(|listener| listener.id)
                .collect()
        };
        let mut listeners = self.event_listener_list.borrow_mut(ec);
        for listener in listeners.iter_mut() {
            if matching_ids.contains(&listener.id) {
                listener.removed = true;
            }
        }

        listeners.retain(|listener| !listener.removed);
    }

    /// <https://dom.spec.whatwg.org/#remove-an-event-listener>
    pub(crate) fn remove_event_listener_by_id(
        &self,
        listener_id: u64,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 2: Set listener's removed to true and remove listener from
        // eventTarget's event listener list.
        let mut listeners = self.event_listener_list.borrow_mut(ec);
        for listener in listeners.iter_mut() {
            if listener.id == listener_id {
                listener.removed = true;
            }
        }

        listeners.retain(|listener| !listener.removed);
    }

    /// Release every listener callback and event handler on this target.
    /// Called during document teardown so the callbacks' strong JS handles
    /// stop rooting the realm once the document is gone. Returns how many
    /// callbacks were released.
    pub(crate) fn clear_all_listeners_and_handlers(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> usize {
        let listeners = self.event_listener_list.borrow_mut(ec).len();
        let handlers = self.event_handlers.borrow_mut(ec).len();
        self.event_listener_list.borrow_mut(ec).clear();
        self.event_handlers.borrow_mut(ec).clear();
        listeners + handlers
    }

    // Note: Defined by the spec but not yet used by the current dispatch code.
    // <https://dom.spec.whatwg.org/#concept-event-listener>
    #[allow(dead_code)]
    pub(crate) fn listener_is_active(
        &self,
        listener_id: u64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> bool {
        self.event_listener_list
            .borrow(ec)
            .iter()
            .any(|listener| listener.id == listener_id && !listener.removed)
    }

    /// <https://dom.spec.whatwg.org/#dom-eventtarget-dispatchevent>
    pub(crate) fn dispatch_event(
        &self,
        event: &Event,
        path: &[super::EventPathItem],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        // Step 1: If event's dispatch flag is set, or if its initialized flag is not set,
        // then throw an "InvalidStateError" DOMException.
        if *event.dispatch_flag.borrow(ec) || !*event.initialized_flag.borrow(ec) {
            return Err(ec.new_type_error(
                "InvalidStateError: event is already being dispatched or not initialized",
            ));
        }

        // Step 2: Initialize event's isTrusted attribute to false.
        *event.is_trusted.borrow_mut(ec) = false;

        // Step 3: Return the result of dispatching event to this.
        crate::dom::dispatch_event(ec, path, event)
    }
}

/// <https://dom.spec.whatwg.org/#event>
#[gc_struct]
pub struct Event {
    pub(crate) reflector: Option<JsObject>,

    /// <https://dom.spec.whatwg.org/#dom-event-type>
    #[ignore_trace]
    pub type_: String,

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub target: GcCell<Option<EventTarget>>,

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub current_target: GcCell<Option<EventTarget>>,

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
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            reflector: None,
            type_,
            target: gc_cell_new(None::<EventTarget>, ec),
            current_target: gc_cell_new(None, ec),
            event_phase: gc_cell_new(NONE, ec),
            bubbles: gc_cell_new(bubbles, ec),
            cancelable: gc_cell_new(cancelable, ec),
            composed: gc_cell_new(composed, ec),
            is_trusted: gc_cell_new(is_trusted, ec),
            time_stamp: gc_cell_new(time_stamp, ec),
            stop_propagation_flag: gc_cell_new(false, ec),
            stop_immediate_propagation_flag: gc_cell_new(false, ec),
            canceled_flag: gc_cell_new(false, ec),
            in_passive_listener_flag: gc_cell_new(false, ec),
            dispatch_flag: gc_cell_new(false, ec),
            initialized_flag: gc_cell_new(true, ec),
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-type>
    pub(crate) fn type_value(&self) -> &str {
        &self.type_
    }

    /// <https://dom.spec.whatwg.org/#dom-event-target>
    pub(crate) fn target_value(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<EventTarget> {
        self.target.borrow(ec).clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-currenttarget>
    pub(crate) fn current_target_value(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<EventTarget> {
        self.current_target.borrow(ec).clone()
    }

    /// <https://dom.spec.whatwg.org/#dom-event-eventphase>
    pub(crate) fn event_phase_value(&self, ec: &mut dyn ExecutionContext<Types>) -> u16 {
        *self.event_phase.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-bubbles>
    pub(crate) fn bubbles_value(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        *self.bubbles.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelable>
    pub(crate) fn cancelable_value(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        *self.cancelable.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-defaultprevented>
    pub(crate) fn default_prevented(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        *self.canceled_flag.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn cancel_bubble(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        *self.stop_propagation_flag.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-cancelbubble>
    pub(crate) fn set_cancel_bubble(&self, value: bool, ec: &mut dyn ExecutionContext<Types>) {
        if value {
            *self.stop_propagation_flag.borrow_mut(ec) = true;
        }
    }

    /// <https://dom.spec.whatwg.org/#dom-event-istrusted>
    pub(crate) fn is_trusted(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        *self.is_trusted.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-timestamp>
    pub(crate) fn time_stamp_value(&self, ec: &mut dyn ExecutionContext<Types>) -> f64 {
        *self.time_stamp.borrow(ec)
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stoppropagation>
    pub(crate) fn stop_propagation(&self, ec: &mut dyn ExecutionContext<Types>) {
        *self.stop_propagation_flag.borrow_mut(ec) = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-stopimmediatepropagation>
    pub(crate) fn stop_immediate_propagation(&self, ec: &mut dyn ExecutionContext<Types>) {
        *self.stop_propagation_flag.borrow_mut(ec) = true;
        *self.stop_immediate_propagation_flag.borrow_mut(ec) = true;
    }

    /// <https://dom.spec.whatwg.org/#dom-event-preventdefault>
    pub(crate) fn prevent_default(&self, ec: &mut dyn ExecutionContext<Types>) {
        if *self.cancelable.borrow(ec) && !*self.in_passive_listener_flag.borrow(ec) {
            *self.canceled_flag.borrow_mut(ec) = true;
        }
    }
}
