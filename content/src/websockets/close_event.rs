use js_engine::{ExecutionContext, gc_struct};

use crate::dom::event::{Event, HasEvent};
use crate::js::Types;

/// <https://websockets.spec.whatwg.org/#dictdef-closeeventinit>
pub(crate) struct CloseEventInit {
    /// <https://dom.spec.whatwg.org/#dom-eventinit-bubbles>
    pub(crate) bubbles: bool,
    /// <https://dom.spec.whatwg.org/#dom-eventinit-cancelable>
    pub(crate) cancelable: bool,
    /// <https://dom.spec.whatwg.org/#dom-eventinit-composed>
    pub(crate) composed: bool,
    /// <https://websockets.spec.whatwg.org/#dom-closeeventinit-wasclean>
    pub(crate) was_clean: bool,
    /// <https://websockets.spec.whatwg.org/#dom-closeeventinit-code>
    pub(crate) code: u16,
    /// <https://websockets.spec.whatwg.org/#dom-closeeventinit-reason>
    pub(crate) reason: String,
}

/// <https://websockets.spec.whatwg.org/#closeevent>
#[gc_struct]
pub(crate) struct CloseEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub(crate) event: Event,

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-wasclean>
    #[ignore_trace]
    was_clean: bool,

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-code>
    #[ignore_trace]
    code: u16,

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-reason>
    #[ignore_trace]
    reason: String,
}

impl HasEvent for CloseEvent {
    fn event(&self) -> &Event {
        &self.event
    }

    fn event_mut(&mut self) -> &mut Event {
        &mut self.event
    }
}

impl CloseEvent {
    /// <https://dom.spec.whatwg.org/#concept-event-constructor>
    pub(crate) fn new(
        type_: String,
        init: CloseEventInit,
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
            was_clean: init.was_clean,
            code: init.code,
            reason: init.reason,
        }
    }

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-wasclean>
    pub(crate) fn was_clean(&self) -> bool {
        // The wasClean getter steps are to return this's wasClean.
        self.was_clean
    }

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-code>
    pub(crate) fn code(&self) -> u16 {
        // The code getter steps are to return this's code.
        self.code
    }

    /// <https://websockets.spec.whatwg.org/#dom-closeevent-reason>
    pub(crate) fn reason(&self) -> String {
        // The reason getter steps are to return this's reason.
        self.reason.clone()
    }
}
