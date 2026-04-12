use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::events::{DomEvent, EventState, UiEvent};
use boa_engine::class::Class;
use html5ever::local_name;
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, NavigateRequest, UserNavigationInvolvement};

use crate::html::{EnvironmentSettingsObject, HTMLAnchorElement};

use super::{UIEvent as JsUiEvent, dispatch_with_chain};

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
/// Note: This bridges Blitz input events into the DOM dispatch algorithm by first letting Blitz compute the native event path and then dispatching the corresponding JavaScript `UIEvent`.
pub(crate) fn dispatch_ui_event(
    document_id: u64,
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    event_sender: &IpcSender<ContentEvent>,
    event: UiEvent,
) -> Result<(), String> {
    let mut document = document;
    let handler = BlitzJSEventHandler::new(document_id, Rc::clone(&document), settings, event_sender);
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    Ok(())
}

struct BlitzJSEventHandler<'a> {
    document_id: u64,
    document: Rc<RefCell<BaseDocument>>,
    settings: &'a mut EnvironmentSettingsObject,
    event_sender: &'a IpcSender<ContentEvent>,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(
        document_id: u64,
        document: Rc<RefCell<BaseDocument>>,
        settings: &'a mut EnvironmentSettingsObject,
        event_sender: &'a IpcSender<ContentEvent>,
    ) -> Self {
        Self {
            document_id,
            document,
            settings,
            event_sender,
        }
    }

    fn maybe_follow_hyperlink(
        &mut self,
        chain: &[usize],
        event: &DomEvent,
        event_state: &mut EventState,
    ) {
        if event.name() != "click" || event_state.is_cancelled() {
            return;
        }

        let anchor_node_id = {
            let document = self.document.borrow();
            chain.iter().copied().find(|node_id| {
                document.get_node(*node_id).is_some_and(|node| {
                    node.data.is_element_with_tag_name(&local_name!("a"))
                })
            })
        };

        let Some(anchor_node_id) = anchor_node_id else {
            return;
        };

        let anchor = HTMLAnchorElement::new(Rc::clone(&self.document), anchor_node_id);
        if anchor.has_download_attribute() {
            return;
        }

        let Some(destination_url) = anchor.follow_hyperlink(&self.settings.creation_url, None) else {
            return;
        };

        let request = NavigateRequest {
            document_id: self.document_id,
            destination_url,
            target: anchor.target(),
            user_involvement: UserNavigationInvolvement::Activation,
            noopener: anchor.noopener(),
        };
        if self
            .event_sender
            .send(ContentEvent::NavigationRequested(request))
            .is_ok()
        {
            event_state.prevent_default();
        }
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

        self.maybe_follow_hyperlink(chain, event, event_state);
    }
}
