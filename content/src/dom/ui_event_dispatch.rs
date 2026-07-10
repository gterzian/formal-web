use log::{error, trace};
use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::SmolStr;
use blitz_traits::events::{BlitzKeyEvent, DomEvent, DomEventData, EventState, UiEvent};
use js_engine::JsTypes;

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
use ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId};
#[cfg(target_os = "macos")]
use keyboard_types::{Key, Modifiers as KeyboardModifiers};

use crate::html::{EnvironmentSettingsObject, HTMLAnchorElement};
use crate::webidl::bindings::create_interface_instance;
use js_engine::{Completion, ExecutionContext};

use super::{Event, EventDispatchHost, UIEvent as JsUiEvent, dispatch, dispatch_with_chain};

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

#[derive(Clone, Copy, Debug, Default)]
struct DeferredAppleStandardKeybinding {
    command: Option<&'static str>,
    keydown_default_prevented: bool,
}

fn apple_standard_keybinding_for_key_down(event: &BlitzKeyEvent) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        if !event.state.is_pressed() {
            return None;
        }

        let command_mod = event.modifiers.contains(KeyboardModifiers::SUPER);
        let control_mod = event.modifiers.contains(KeyboardModifiers::CONTROL);
        let option_mod = event.modifiers.contains(KeyboardModifiers::ALT);
        let shift_mod = event.modifiers.contains(KeyboardModifiers::SHIFT);

        if command_mod {
            match &event.key {
                Key::Backspace => return Some("deleteToBeginningOfLine:"),
                Key::Delete => return Some("deleteToEndOfLine:"),
                Key::ArrowLeft if shift_mod => {
                    return Some("moveToBeginningOfLineAndModifySelection:");
                }
                Key::ArrowLeft => return Some("moveToBeginningOfLine:"),
                Key::ArrowRight if shift_mod => {
                    return Some("moveToEndOfLineAndModifySelection:");
                }
                Key::ArrowRight => return Some("moveToEndOfLine:"),
                Key::ArrowUp if shift_mod => {
                    return Some("moveToBeginningOfDocumentAndModifySelection:");
                }
                Key::ArrowUp => return Some("moveToBeginningOfDocument:"),
                Key::ArrowDown if shift_mod => {
                    return Some("moveToEndOfDocumentAndModifySelection:");
                }
                Key::ArrowDown => return Some("moveToEndOfDocument:"),
                _ => {}
            }
        }

        if option_mod {
            match &event.key {
                Key::Backspace => return Some("deleteWordBackward:"),
                Key::Delete => return Some("deleteWordForward:"),
                Key::ArrowLeft if shift_mod => return Some("moveWordLeftAndModifySelection:"),
                Key::ArrowLeft => return Some("moveWordLeft:"),
                Key::ArrowRight if shift_mod => return Some("moveWordRightAndModifySelection:"),
                Key::ArrowRight => return Some("moveWordRight:"),
                _ => {}
            }
        }

        if control_mod && let Key::Character(value) = &event.key {
            return match value.to_lowercase().as_str() {
                "a" if shift_mod => Some("moveToBeginningOfParagraphAndModifySelection:"),
                "a" => Some("moveToBeginningOfParagraph:"),
                "b" if shift_mod => Some("moveBackwardAndModifySelection:"),
                "b" => Some("moveBackward:"),
                "d" => Some("deleteForward:"),
                "e" if shift_mod => Some("moveToEndOfParagraphAndModifySelection:"),
                "e" => Some("moveToEndOfParagraph:"),
                "f" if shift_mod => Some("moveForwardAndModifySelection:"),
                "f" => Some("moveForward:"),
                "h" => Some("deleteBackward:"),
                "k" => Some("deleteToEndOfParagraph:"),
                "n" if shift_mod => Some("moveDownAndModifySelection:"),
                "n" => Some("moveDown:"),
                "o" => Some("insertNewlineIgnoringFieldEditor:"),
                "p" if shift_mod => Some("moveUpAndModifySelection:"),
                "p" => Some("moveUp:"),
                _ => None,
            };
        }

        match &event.key {
            Key::Backspace => Some("deleteBackward:"),
            _ => None,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = event;
        None
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
    let id = element
        .id
        .as_ref()
        .map(|id| id.as_ref())
        .filter(|id| !id.is_empty());
    let prefix = if node.is_anonymous() {
        "anonymous:"
    } else {
        ""
    };
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
        viewport.x, viewport.y,
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
        UiEvent::PointerMove(event) | UiEvent::PointerUp(event) | UiEvent::PointerDown(event) => {
            if input_debug_enabled() {
                trace!(
                    "[input-debug][content] localize pointer before client=({:.1},{:.1}) page=({:.1},{:.1}) offset=({:.1},{:.1}) scroll=({:.1},{:.1})",
                    event.coords.client_x,
                    event.coords.client_y,
                    event.coords.page_x,
                    event.coords.page_y,
                    viewport_offset_x,
                    viewport_offset_y,
                    scroll_x,
                    scroll_y,
                );
            }
            event.coords.client_x -= viewport_offset_x;
            event.coords.client_y -= viewport_offset_y;
            event.coords.page_x = event.coords.client_x + scroll_x;
            event.coords.page_y = event.coords.client_y + scroll_y;
            if input_debug_enabled() {
                trace!(
                    "[input-debug][content] localize pointer after client=({:.1},{:.1}) page=({:.1},{:.1})",
                    event.coords.client_x,
                    event.coords.client_y,
                    event.coords.page_x,
                    event.coords.page_y,
                );
            }
        }
        UiEvent::Wheel(event) => {
            if input_debug_enabled() {
                trace!(
                    "[input-debug][content] localize wheel before client=({:.1},{:.1}) page=({:.1},{:.1}) offset=({:.1},{:.1}) scroll=({:.1},{:.1})",
                    event.coords.client_x,
                    event.coords.client_y,
                    event.coords.page_x,
                    event.coords.page_y,
                    viewport_offset_x,
                    viewport_offset_y,
                    scroll_x,
                    scroll_y,
                );
            }
            event.coords.client_x -= viewport_offset_x;
            event.coords.client_y -= viewport_offset_y;
            event.coords.page_x = event.coords.client_x + scroll_x;
            event.coords.page_y = event.coords.client_y + scroll_y;
            if input_debug_enabled() {
                trace!(
                    "[input-debug][content] localize wheel after client=({:.1},{:.1}) page=({:.1},{:.1})",
                    event.coords.client_x,
                    event.coords.client_y,
                    event.coords.page_x,
                    event.coords.page_y,
                );
            }
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
        trace!(
            "[input-debug][content] document={} traversable={} event={}",
            document_id,
            source_navigable_id,
            ui_event_kind(&event),
        );
    }
    let mut event = event;
    {
        let document = document.borrow();
        localize_ui_event_for_document(&document, viewport_offset_x, viewport_offset_y, &mut event);
    }

    let mut document = document;
    let deferred_apple_keybinding =
        Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default()));
    let handler = BlitzJSEventHandler::new(
        document_id,
        source_navigable_id,
        parent_navigable_id,
        top_level_navigable_id,
        Rc::clone(&document),
        settings,
        event_sender,
        Rc::clone(&deferred_apple_keybinding),
    );
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    let deferred_apple_keybinding = *deferred_apple_keybinding.borrow();
    if let Some(command) = deferred_apple_keybinding.command
        && !deferred_apple_keybinding.keydown_default_prevented
    {
        driver.handle_ui_event(UiEvent::AppleStandardKeybinding(SmolStr::new(command)));
    }
    if is_wheel && input_debug_enabled() {
        let document = document.borrow();
        trace!(
            "[input-debug][scroll] document={} traversable={} {}",
            document_id,
            source_navigable_id,
            debug_scroll_state(&document),
        );
    }
    Ok(())
}

pub(crate) fn dispatch_trusted_click_event(
    document_id: DocumentId,
    source_navigable_id: NavigableId,
    parent_navigable_id: Option<NavigableId>,
    top_level_navigable_id: NavigableId,
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    event_sender: &IpcSender<ContentEvent>,
    target_node_id: usize,
) -> Result<(), String> {
    let mut handler = BlitzJSEventHandler::new(
        document_id,
        source_navigable_id,
        parent_navigable_id,
        top_level_navigable_id,
        document,
        settings,
        event_sender,
        Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default())),
    );
    let target = handler
        .resolve_element_object(target_node_id)
        .map_err(|error| format!("failed to resolve click target element: {error:?}"))?;
    let event = handler
        .create_event_object(Event::new(
            String::from("click"),
            true,
            true,
            true,
            true,
            handler.current_time_millis(),
        ))
        .map_err(|error| format!("failed to construct trusted click event: {error:?}"))?;
    dispatch(&mut handler, &target, &event, false)
        .map_err(|error| format!("failed to dispatch trusted click event: {error:?}"))?;
    handler
        .settings
        .perform_a_microtask_checkpoint()
        .map_err(|error| {
            format!("failed to run a microtask checkpoint after trusted click dispatch: {error:?}")
        })?;
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
    deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
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
        deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
    ) -> Self {
        Self {
            document_id,
            source_navigable_id,
            parent_navigable_id,
            top_level_navigable_id,
            _document: document,
            settings,
            event_sender,
            deferred_apple_keybinding,
        }
    }
}

impl EventDispatchHost for BlitzJSEventHandler<'_> {
    fn ec(&mut self) -> &mut dyn ExecutionContext<crate::js::Types> {
        self.settings.ec()
    }

    fn create_event_object(&mut self, event: Event) -> Completion<JsObject, crate::js::Types> {
        self.settings.create_event_object(event)
    }

    fn document_object(&mut self) -> Completion<JsObject, crate::js::Types> {
        self.settings.document_object()
    }

    fn global_object(&mut self) -> JsObject {
        self.settings.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> Completion<JsObject, crate::js::Types> {
        self.settings.resolve_element_object(node_id)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> Completion<JsObject, crate::js::Types> {
        self.settings
            .resolve_existing_node_object(document, node_id)
    }

    fn current_time_millis(&self) -> f64 {
        self.settings.current_time_millis()
    }

    fn has_activation_behavior(&mut self, target: &JsObject) -> bool {
        self.ec()
            .with_object_any(target)
            .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
            .is_some()
    }

    fn run_activation_behavior(
        &mut self,
        target: &JsObject,
        event: &JsObject,
    ) -> Completion<(), crate::js::Types> {
        let anchor = self
            .ec()
            .with_object_any(target)
            .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
            .map(|a| a.clone());
        if let Some(anchor) = anchor {
            let result = anchor.activation_behavior(
                self.source_navigable_id,
                self.parent_navigable_id,
                self.top_level_navigable_id,
                &self.settings.creation_url,
                event,
                self.event_sender,
            );
            if let Err(error_msg) = result {
                return Err(self.ec().new_type_error(&error_msg));
            }
        }
        Ok(())
    }
}

impl js_engine::EcmascriptHost<crate::js::Types> for BlitzJSEventHandler<'_> {
    fn get(
        &mut self,
        object: &JsObject,
        property: &str,
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        js_engine::EcmascriptHost::get(&mut self.settings.engine, object, property)
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        self.settings.engine.is_callable(value)
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        self.settings.engine.call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> js_engine::Completion<(), crate::js::Types> {
        self.settings.engine.perform_a_microtask_checkpoint()
    }

    fn report_exception(&mut self, error: JsValue) {
        self.settings.engine.report_exception(error)
    }

    fn gc(&mut self) {
        self.settings.engine.gc()
    }

    fn value_undefined(&mut self) -> JsValue {
        self.settings.engine.value_undefined()
    }
    fn value_null(&mut self) -> JsValue {
        self.settings.engine.value_null()
    }
    fn value_from_bool(&mut self, b: bool) -> JsValue {
        self.settings.engine.value_from_bool(b)
    }
    fn value_from_number(&mut self, n: f64) -> JsValue {
        self.settings.engine.value_from_number(n)
    }
    fn value_from_string(&mut self, s: <Types as JsTypes>::JsString) -> JsValue {
        self.settings.engine.value_from_string(s)
    }
    fn js_string_from_str(&self, s: &str) -> <Types as JsTypes>::JsString {
        self.settings.engine.js_string_from_str(s)
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
                .map(|node_id| {
                    debug_blitz_node_label(doc, *node_id)
                        .unwrap_or_else(|| format!("node#{node_id}"))
                })
                .collect::<Vec<_>>();
            trace!(
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
        let view = Some(self.settings.engine.realm_global_object());
        let ui_event = JsUiEvent::from_dom_event(event, view, time_stamp);
        let event_object = create_interface_instance::<crate::js::Types, JsUiEvent>(
            ui_event,
            &mut self.settings.engine,
        )
        .expect("UIEvent construction must succeed");
        if let Err(error) = dispatch_with_chain(self, chain, &event_object) {
            let error_msg = self
                .ec()
                .to_rust_string(error.clone())
                .unwrap_or_else(|_| format!("{error:?}"));
            error!("failed to dispatch UI event through JavaScript listeners: {error_msg}");
            return;
        }

        let ui_event = self
            .ec()
            .with_object_any(&event_object)
            .and_then(|data| data.downcast_ref::<JsUiEvent>())
            .map(|u| u.clone());
        if let Some(ui_event) = ui_event {
            ui_event.apply_to_event_state(event_state);
        }

        if let DomEventData::KeyDown(key_event) = &event.data
            && let Some(command) = apple_standard_keybinding_for_key_down(key_event)
        {
            let keydown_default_prevented = event_state.is_cancelled();
            *self.deferred_apple_keybinding.borrow_mut() = DeferredAppleStandardKeybinding {
                command: Some(command),
                keydown_default_prevented,
            };
            event_state.prevent_default();
        }

        if let Err(error) = self.settings.perform_a_microtask_checkpoint() {
            error!("failed to run a microtask checkpoint after UI event dispatch: {error}");
        }
    }
}
