use blitz_dom::{Document as BlitzDocument, EventHandler};
use blitz_traits::events::{DomEvent, EventState};
use boa_engine::class::Class;

use super::{bindings, execution_context::JsExecutionContext};
use crate::dom::UIEvent;

/// <https://html.spec.whatwg.org/#event-firing>
pub struct BlitzEventHandler<'a> {
    execution_context: &'a mut JsExecutionContext,
}

impl<'a> BlitzEventHandler<'a> {
    pub fn new(execution_context: &'a mut JsExecutionContext) -> Self {
        Self { execution_context }
    }
}

impl EventHandler for BlitzEventHandler<'_> {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        _doc: &mut dyn BlitzDocument,
        event_state: &mut EventState,
    ) {
        let time_stamp = self.execution_context.navigation_start.elapsed().as_secs_f64() * 1000.0;
        let view = Some(self.execution_context.context.global_object());
        let ui_event = UIEvent::from_dom_event(event, view, time_stamp);
        let event_object = UIEvent::from_data(ui_event, &mut self.execution_context.context)
            .expect("UIEvent construction must succeed");
        if let Err(error) =
            bindings::dispatch_with_chain(chain, &event_object, &mut self.execution_context.context)
        {
            eprintln!("failed to dispatch UI event through JavaScript listeners: {error}");
            return;
        }

        if let Some(ui_event) = event_object.downcast_ref::<UIEvent>() {
            ui_event.apply_to_event_state(event_state);
        }
    }
}