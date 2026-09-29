use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::Event;
use crate::dom::event::{EventTarget, HasEvent};
use crate::dom::{dispatch_with_path, simple_path};
use crate::js::Types;
use crate::webidl::bindings::{WebIdlInterface, create_interface_instance};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceevent>
#[gc_struct]
pub(crate) struct RTCPeerConnectionIceEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub(crate) event: Event,

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceevent-candidate>
    pub(crate) candidate: GcCell<Option<JsObject>>,

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceevent-url>
    #[ignore_trace]
    pub(crate) url: Option<String>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceeventinit>
pub(crate) struct RTCPeerConnectionIceEventInit {
    pub(crate) bubbles: bool,
    pub(crate) cancelable: bool,
    pub(crate) composed: bool,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceeventinit-candidate>
    pub(crate) candidate: Option<JsObject>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnectioniceeventinit-url>
    pub(crate) url: Option<String>,
}

impl HasEvent for RTCPeerConnectionIceEvent {
    fn event(&self) -> &Event {
        &self.event
    }

    fn event_mut(&mut self) -> &mut Event {
        &mut self.event
    }
}

impl RTCPeerConnectionIceEvent {
    /// <https://dom.spec.whatwg.org/#concept-event-constructor>
    pub(crate) fn new(
        type_: String,
        init: RTCPeerConnectionIceEventInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            event: Event::new(
                type_,
                init.bubbles,
                init.cancelable,
                init.composed,
                false,
                0.0,
                ec,
            ),
            candidate: gc_cell_new(init.candidate, ec),
            url: init.url,
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelevent>
#[gc_struct]
pub(crate) struct RTCDataChannelEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub(crate) event: Event,

    /// <https://w3c.github.io/webrtc-pc/#dom-datachannelevent-channel>
    pub(crate) channel: JsObject,
}

impl HasEvent for RTCDataChannelEvent {
    fn event(&self) -> &Event {
        &self.event
    }

    fn event_mut(&mut self) -> &mut Event {
        &mut self.event
    }
}

impl RTCDataChannelEvent {
    /// <https://dom.spec.whatwg.org/#concept-event-constructor>
    pub(crate) fn new(
        type_: String,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
        channel: JsObject,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            event: Event::new(type_, bubbles, cancelable, composed, false, 0.0, ec),
            channel,
        }
    }
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
