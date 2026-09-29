use js_engine::ExecutionContext;
use js_engine::gc_struct;

use crate::dom::Event;
use crate::dom::event::HasEvent;
use crate::js::Types;

use super::media_stream_track::MediaStreamTrack;

/// <https://w3c.github.io/mediacapture-main/#dom-mediastreamtrackevent>
#[gc_struct]
pub(crate) struct MediaStreamTrackEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub(crate) event: Event,

    /// <https://w3c.github.io/mediacapture-main/#dom-mediastreamtrackevent-track>
    pub(crate) track: MediaStreamTrack,
}

impl HasEvent for MediaStreamTrackEvent {
    fn event(&self) -> &Event {
        &self.event
    }

    fn event_mut(&mut self) -> &mut Event {
        &mut self.event
    }
}

impl MediaStreamTrackEvent {
    /// <https://dom.spec.whatwg.org/#concept-event-constructor>
    pub(crate) fn new(
        type_: String,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
        track: MediaStreamTrack,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            event: Event::new(type_, bubbles, cancelable, composed, false, 0.0, ec),
            track,
        }
    }
}
