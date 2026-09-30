use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{ExecutionContext, JsTypes};

use crate::dom::Event;
use crate::dom::event::HasEvent;
use crate::js::Types;

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
