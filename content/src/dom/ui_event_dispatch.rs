use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::events::{DomEvent, EventState, UiEvent};
use boa_engine::class::Class;
use boa_engine::{Context, JsResult, object::JsObject};
use ipc_channel::ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId};

use crate::html::{EnvironmentSettingsObject, HTMLAnchorElement};
use crate::webidl::EcmascriptHost;

use super::{Event, EventDispatchHost, UIEvent as JsUiEvent, dispatch_with_chain};

fn input_debug_enabled() -> bool {
    std::env::var_os("FORMAL_WEB_DEBUG_INPUT").is_some()
}

fn ui_event_kind(event: &UiEvent) -> &'static str {
    match event {
        UiEvent::PointerMove(_) => "PointerMove",
        UiEvent::PointerUp(_) => "PointerUp",
        UiEvent::PointerDown(_) => "PointerDown",
        UiEvent::Wheel(_) => "Wheel",
        UiEvent::KeyUp(_) => "KeyUp",
        UiEvent::KeyDown(_) => "KeyDown",
        UiEvent::Ime(_) => "Ime",
        UiEvent::AppleStandardKeybinding(_) => "AppleStandardKeybinding",
    }
}

fn debug_blitz_node_label(document: &dyn BlitzDocument, node_id: usize) -> Option<String> {
    let document = document.inner();
    let node = document.get_node(node_id)?;
    if node.is_text_node() {
        return Some(String::from("#text"));
    }

    let element = node.element_data()?;
    let tag_name = element.name.local.as_ref();
    let id = element.id.as_ref().map(|id| id.as_ref()).filter(|id| !id.is_empty());
    let prefix = if node.is_anonymous() { "anonymous:" } else { "" };
    Some(match id {
        Some(id) => format!("{prefix}{tag_name}#{id}"),
        None => format!("{prefix}{tag_name}"),
    })
}

fn debug_scroll_state(document: &BaseDocument) -> String {
    let viewport = document.viewport_scroll();
    let mut html_scroll = None;
    let mut body_scroll = None;

    document.visit(|node_id, node| {
        let Some(element) = node.element_data() else {
            return;
        };
        let tag_name = element.name.local.as_ref();
        if tag_name == "html" {
            html_scroll = Some((node_id, node.scroll_offset.x, node.scroll_offset.y));
        } else if tag_name == "body" {
            body_scroll = Some((node_id, node.scroll_offset.x, node.scroll_offset.y));
        }
    });

    format!(
        "viewport=({:.1},{:.1}) html={html_scroll:?} body={body_scroll:?}",
        viewport.x,
        viewport.y,
    )
}

fn localize_ui_event_for_document(
    document: &BaseDocument,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
    event: &mut UiEvent,
) {
    let viewport_scroll = document.viewport_scroll();
    let scroll_x = viewport_scroll.x as f32;
    let scroll_y = viewport_scroll.y as f32;

    match event {
        UiEvent::PointerMove(event)
        | UiEvent::PointerUp(event)
        | UiEvent::PointerDown(event) => {
            event.coords.client_x -= viewport_offset_x;
            event.coords.client_y -= viewport_offset_y;
            event.coords.page_x = event.coords.client_x + scroll_x;
            event.coords.page_y = event.coords.client_y + scroll_y;
        }
        UiEvent::Wheel(event) => {
            event.coords.client_x -= viewport_offset_x;
            event.coords.client_y -= viewport_offset_y;
            event.coords.page_x = event.coords.client_x + scroll_x;
            event.coords.page_y = event.coords.client_y + scroll_y;
        }
        UiEvent::KeyUp(_)
        | UiEvent::KeyDown(_)
        | UiEvent::Ime(_)
        | UiEvent::AppleStandardKeybinding(_) => {}
    }
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
/// Note: This bridges Blitz input events into the DOM dispatch algorithm by first letting Blitz compute the native event path and then dispatching the corresponding JavaScript `UIEvent`.
pub(crate) fn dispatch_ui_event(
    document_id: DocumentId,
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    event_sender: &IpcSender<ContentEvent>,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
    event: UiEvent,
) -> Result<(), String> {
    let is_wheel = matches!(event, UiEvent::Wheel(_));
    if input_debug_enabled() {
        eprintln!(
            "[input-debug][content] document={} traversable={} event={}",
            document_id,
            source_navigable_id,
            ui_event_kind(&event),
        );
    }
    let mut event = event;
    {
        let document = document.borrow();
        localize_ui_event_for_document(
            &document,
            viewport_offset_x,
            viewport_offset_y,
            &mut event,
        );
    }

    let mut document = document;
    let handler = BlitzJSEventHandler::new(
        document_id,
        source_navigable_id,
        parent_navigable_id,
        top_level_navigable_id,
        Rc::clone(&document),
        settings,
        event_sender,
    );
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    if is_wheel && input_debug_enabled() {
        let document = document.borrow();
        eprintln!(
            "[input-debug][scroll] document={} traversable={} {}",
            document_id,
            source_navigable_id,
            debug_scroll_state(&document),
        );
    }
    Ok(())
}

struct BlitzJSEventHandler<'a> {
    document_id: DocumentId,
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    _document: Rc<RefCell<BaseDocument>>,
    settings: &'a mut EnvironmentSettingsObject,
    event_sender: &'a IpcSender<ContentEvent>,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(
        document_id: DocumentId,
        source_navigable_id: NavigableId,
        parent_navigable_id: Option<NavigableId>,
        top_level_navigable_id: NavigableId,
        document: Rc<RefCell<BaseDocument>>,
        settings: &'a mut EnvironmentSettingsObject,
        event_sender: &'a IpcSender<ContentEvent>,
    ) -> Self {
        Self {
            document_id,
            source_navigable_id,
            parent_navigable_id,
            top_level_navigable_id,
            _document: document,
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
                self.parent_navigable_id,
                self.top_level_navigable_id,
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
        doc: &mut dyn BlitzDocument,
        event_state: &mut EventState,
    ) {
        if input_debug_enabled() {
            let target_label = debug_blitz_node_label(doc, event.target);
            let chain_labels = chain
                .iter()
                .map(|node_id| debug_blitz_node_label(doc, *node_id).unwrap_or_else(|| format!("node#{node_id}")))
                .collect::<Vec<_>>();
            eprintln!(
                "[input-debug][content-dom] document={} traversable={} type={} target_node={} target_label={:?} chain={:?} chain_labels={:?}",
                self.document_id,
                self.source_navigable_id,
                event.name(),
                event.target,
                target_label,
                chain,
                chain_labels,
            );
        }

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
