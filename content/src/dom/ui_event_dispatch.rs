use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::events::{DomEvent, EventState, UiEvent};
use boa_engine::class::Class;

use crate::html::EnvironmentSettingsObject;

use super::{UIEvent as JsUiEvent, dispatch_with_chain};

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
/// Note: This bridges Blitz input events into the DOM dispatch algorithm by first letting Blitz compute the native event path and then dispatching the corresponding JavaScript `UIEvent`.
pub(crate) fn dispatch_ui_event(
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    event: UiEvent,
) -> Result<(), String> {
    let mut document = document;
    let handler = BlitzJSEventHandler::new(settings);
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    Ok(())
}

struct BlitzJSEventHandler<'a> {
    settings: &'a mut EnvironmentSettingsObject,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(settings: &'a mut EnvironmentSettingsObject) -> Self {
        Self { settings }
    }
}

impl EventHandler for BlitzJSEventHandler<'_> {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        _doc: &mut dyn BlitzDocument,
        event_state: &mut EventState,
    ) {
        let time_stamp = self.settings.current_time_millis();
        let view = Some(self.settings.context.global_object());
        let ui_event = JsUiEvent::from_dom_event(event, view, time_stamp);
        let event_object = JsUiEvent::from_data(ui_event, &mut self.settings.context)
            .expect("UIEvent construction must succeed");
        if let Err(error) = dispatch_with_chain(self.settings, chain, &event_object) {
            eprintln!("failed to dispatch UI event through JavaScript listeners: {error}");
            return;
        }

        if let Some(ui_event) = event_object.downcast_ref::<JsUiEvent>() {
            ui_event.apply_to_event_state(event_state);
        }
    }
}
