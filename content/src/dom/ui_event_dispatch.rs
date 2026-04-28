use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::events::{DomEvent, EventState, UiEvent};
use boa_engine::class::Class;
use boa_engine::{Context, JsResult, object::JsObject};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::Event as ContentEvent;

use crate::html::{EnvironmentSettingsObject, HTMLAnchorElement};
use crate::webidl::EcmascriptHost;

use super::{Event, EventDispatchHost, UIEvent as JsUiEvent, dispatch_with_chain};

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
/// Note: This bridges Blitz input events into the DOM dispatch algorithm by first letting Blitz compute the native event path and then dispatching the corresponding JavaScript `UIEvent`.
pub(crate) fn dispatch_ui_event(
    document_id: u64,
    source_navigable_id: u64,
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    event_sender: &IpcSender<ContentEvent>,
    event: UiEvent,
) -> Result<(), String> {
    let mut document = document;
    let handler = BlitzJSEventHandler::new(
        document_id,
        source_navigable_id,
        Rc::clone(&document),
        settings,
        event_sender,
    );
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    Ok(())
}

struct BlitzJSEventHandler<'a> {
    document_id: u64,
    source_navigable_id: u64,
    document: Rc<RefCell<BaseDocument>>,
    settings: &'a mut EnvironmentSettingsObject,
    event_sender: &'a IpcSender<ContentEvent>,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(
        document_id: u64,
        source_navigable_id: u64,
        document: Rc<RefCell<BaseDocument>>,
        settings: &'a mut EnvironmentSettingsObject,
        event_sender: &'a IpcSender<ContentEvent>,
    ) -> Self {
        Self {
            document_id,
            source_navigable_id,
            document,
            settings,
            event_sender,
        }
    }
}

impl EventDispatchHost for BlitzJSEventHandler<'_> {
    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        self.settings.create_event_object(event)
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        self.settings.document_object()
    }

    fn global_object(&mut self) -> JsObject {
        self.settings.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        self.settings.resolve_element_object(node_id)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        self.settings
            .resolve_existing_node_object(document, node_id)
    }

    fn current_time_millis(&self) -> f64 {
        self.settings.current_time_millis()
    }

    fn has_activation_behavior(&mut self, target: &JsObject) -> bool {
        target.downcast_ref::<HTMLAnchorElement>().is_some()
    }

    fn run_activation_behavior(&mut self, target: &JsObject, event: &JsObject) -> JsResult<()> {
        if let Some(anchor) = target.downcast_ref::<HTMLAnchorElement>() {
            anchor.activation_behavior(
                self.source_navigable_id,
                &self.settings.creation_url,
                event,
                self.event_sender,
            )?;
        }
        Ok(())
    }
}

impl EcmascriptHost for BlitzJSEventHandler<'_> {
    fn context(&mut self) -> &mut Context {
        &mut self.settings.context
    }

    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<boa_engine::JsValue> {
        self.settings.get(object, property)
    }

    fn is_callable(&self, object: &JsObject) -> bool {
        self.settings.is_callable(object)
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &boa_engine::JsValue,
        args: &[boa_engine::JsValue],
    ) -> JsResult<boa_engine::JsValue> {
        self.settings.call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        <EnvironmentSettingsObject as EcmascriptHost>::perform_a_microtask_checkpoint(self.settings)
    }

    fn report_exception(&mut self, error: boa_engine::JsError, callback: &JsObject) {
        self.settings.report_exception(error, callback)
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
        if let Err(error) = dispatch_with_chain(self, chain, &event_object) {
            eprintln!("failed to dispatch UI event through JavaScript listeners: {error}");
            return;
        }

        if let Some(ui_event) = event_object.downcast_ref::<JsUiEvent>() {
            ui_event.apply_to_event_state(event_state);
        }

        if let Err(error) = self.settings.perform_a_microtask_checkpoint() {
            eprintln!("failed to run a microtask checkpoint after UI event dispatch: {error}");
        }
    }
}
