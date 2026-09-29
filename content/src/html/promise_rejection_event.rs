use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{ExecutionContext, JsTypes};

use crate::dom::Event;
use crate::dom::event::HasEvent;
use crate::js::Types;

type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://html.spec.whatwg.org/#promiserejectionevent>
#[gc_struct]
pub(crate) struct PromiseRejectionEvent {
    /// <https://dom.spec.whatwg.org/#event>
    pub(crate) event: Event,

    /// <https://html.spec.whatwg.org/#dom-promiserejectionevent-promise>
    pub(crate) promise: JsObject,

    /// <https://html.spec.whatwg.org/#dom-promiserejectionevent-reason>
    pub(crate) reason: GcCell<JsValue>,
}

/// <https://html.spec.whatwg.org/#promiserejectioneventinit>
pub(crate) struct PromiseRejectionEventInit {
    pub(crate) bubbles: bool,
    pub(crate) cancelable: bool,
    pub(crate) composed: bool,
    /// <https://html.spec.whatwg.org/#dom-promiserejectioneventinit-promise>
    pub(crate) promise: JsObject,
    /// <https://html.spec.whatwg.org/#dom-promiserejectioneventinit-reason>
    pub(crate) reason: JsValue,
}

impl HasEvent for PromiseRejectionEvent {
    fn event(&self) -> &Event {
        &self.event
    }

    fn event_mut(&mut self) -> &mut Event {
        &mut self.event
    }
}

impl PromiseRejectionEvent {
    /// <https://dom.spec.whatwg.org/#concept-event-constructor>
    pub(crate) fn new(
        type_: String,
        init: PromiseRejectionEventInit,
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
            promise: init.promise,
            reason: gc_cell_new(init.reason, ec),
        }
    }
}
