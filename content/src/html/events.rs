//! Blitz-to-DOM event bridge.
//!
//! This module connects blitz's compositor event system (pointer moves,
//! key events, wheel, etc.) to the DOM event dispatch algorithm. The code
//! here is content-process infrastructure — it is not specified by any
//! Web standard. The standard algorithms live in:
//!   - `content/src/dom/dispatch.rs` (DOM event dispatch)
//!   - `content/src/html/dispatch.rs`  (HTML beforeunload steps)

use std::{cell::RefCell, collections::HashSet, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::SmolStr;
use blitz_traits::events::{BlitzKeyEvent, DomEvent, DomEventData, EventState, UiEvent};
use ipc::IpcSender;
use ipc_messages::content::{DocumentId, Event as ContentEvent, NavigableId};
#[cfg(target_os = "macos")]
use keyboard_types::{Key, Modifiers as KeyboardModifiers};
use log::{error, trace};
use js_engine::{ExecutionContext, JsTypes};

use crate::dom::event::{Event, EventTarget};
use crate::dom::{
    Document, Element, EventPathItem, Node, UIEvent as JsUiEvent, dispatch_with_path,
};
use crate::html::{EnvironmentSettingsObject, HTMLElement, Window};
use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

type JsObject = <Types as JsTypes>::JsObject;

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
        let _ = (command_mod, control_mod, option_mod, shift_mod);

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
        return Some("#text".into());
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
        if let Some(element) = node.element_data() {
            let tag_name = element.name.local.as_ref();
            if tag_name == "html" {
                html_scroll = Some((node_id, node.scroll_offset.x, node.scroll_offset.y));
            } else if tag_name == "body" {
                body_scroll = Some((node_id, node.scroll_offset.x, node.scroll_offset.y));
            }
        }
    });
    format!("viewport=({:.1},{:.1}) html={html_scroll:?} body={body_scroll:?}", viewport.x, viewport.y)
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
        UiEvent::PointerMove(e) | UiEvent::PointerUp(e) | UiEvent::PointerDown(e) => {
            e.coords.client_x -= viewport_offset_x;
            e.coords.client_y -= viewport_offset_y;
            e.coords.page_x = e.coords.client_x + scroll_x;
            e.coords.page_y = e.coords.client_y + scroll_y;
        }
        UiEvent::Wheel(e) => {
            e.coords.client_x -= viewport_offset_x;
            e.coords.client_y -= viewport_offset_y;
            e.coords.page_x = e.coords.client_x + scroll_x;
            e.coords.page_y = e.coords.client_y + scroll_y;
        }
        UiEvent::KeyUp(_) | UiEvent::KeyDown(_) | UiEvent::Ime(_) | UiEvent::AppleStandardKeybinding(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Event path building from JsObjects
// ---------------------------------------------------------------------------

fn event_target_from_js_object(ec: &mut dyn ExecutionContext<Types>, object: &JsObject) -> Option<EventTarget> {
    ec.with_object_any(object).and_then(|data| {
        if let Some(window) = data.downcast_ref::<Window>() {
            Some(window.event_target.clone())
        } else if let Some(document) = data.downcast_ref::<Document>() {
            Some(document.node.event_target.clone())
        } else if let Some(element) = data.downcast_ref::<Element>() {
            Some(element.node.event_target.clone())
        } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            Some(html_element.element.node.event_target.clone())
        } else if let Some(node) = data.downcast_ref::<Node>() {
            Some(node.event_target.clone())
        } else if let Some(et) = data.downcast_ref::<EventTarget>() {
            Some(et.clone())
        } else {
            None
        }
    })
}

/// Build an event path from a target JsObject by walking the DOM parent chain.
pub(crate) fn build_path_from_target_js_object(
    target_object: &JsObject,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    let mut path: Vec<EventPathItem> = Vec::new();
    let node_info = ec.with_object_any(target_object).and_then(|data| {
        if let Some(element) = data.downcast_ref::<Element>() {
            Some((element.node.node_id, element.node.document.clone()))
        } else if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            Some((html_element.element.node.node_id, html_element.element.node.document.clone()))
        } else if let Some(node) = data.downcast_ref::<Node>() {
            Some((node.node_id, node.document.clone()))
        } else if let Some(document) = data.downcast_ref::<Document>() {
            Some((document.node.node_id, document.node.document.clone()))
        } else {
            None
        }
    });

    if let Some((node_id, document)) = node_info {
        if let Some(event_target) = event_target_from_js_object(ec, target_object) {
            path.push(EventPathItem { invocation_target: event_target.clone(), shadow_adjusted_target: Some(event_target) });
        }
        let mut current_node_id = node_id;
        let mut visited = HashSet::new();
        visited.insert(node_id);
        loop {
            let parent_id = { let doc = document.borrow(); doc.get_node(current_node_id).and_then(|n| n.parent) };
            match parent_id {
                Some(pid) if !visited.contains(&pid) => {
                    visited.insert(pid);
                    if let Ok(parent_object) = crate::js::platform_objects::resolve_element_object(pid, ec) {
                        if let Some(parent_et) = event_target_from_js_object(ec, &parent_object) {
                            path.push(EventPathItem { invocation_target: parent_et, shadow_adjusted_target: None });
                        }
                        current_node_id = pid;
                    } else {
                        current_node_id = pid;
                    }
                }
                _ => break,
            }
        }
    } else {
        if let Some(event_target) = event_target_from_js_object(ec, target_object) {
            path.push(EventPathItem { invocation_target: event_target.clone(), shadow_adjusted_target: Some(event_target) });
        }
    }
    path
}

fn build_event_path(
    chain: &[usize],
    document_event_target: EventTarget,
    global_event_target: Option<EventTarget>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    let mut path = Vec::with_capacity(chain.len() + 2);
    for (index, node_id) in chain.iter().enumerate() {
        if let Ok(object) = crate::js::platform_objects::resolve_element_object(*node_id, ec) {
            if let Some(et) = event_target_from_js_object(ec, &object) {
                path.push(EventPathItem { invocation_target: et.clone(), shadow_adjusted_target: (index == 0).then_some(et) });
            }
        }
    }
    path.push(EventPathItem { invocation_target: document_event_target, shadow_adjusted_target: None });
    if let Some(get) = global_event_target {
        path.push(EventPathItem { invocation_target: get, shadow_adjusted_target: None });
    }
    path
}

// ---------------------------------------------------------------------------
// Blitz event handler
// ---------------------------------------------------------------------------

struct BlitzJSEventHandler<'a> {
    document_id: DocumentId,
    source_navigable_id: NavigableId,
    _document: Rc<RefCell<BaseDocument>>,
    settings: &'a mut EnvironmentSettingsObject,
    deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(
        document_id: DocumentId,
        source_navigable_id: NavigableId,
        _parent_navigable_id: Option<NavigableId>,
        _top_level_navigable_id: NavigableId,
        document: Rc<RefCell<BaseDocument>>,
        settings: &'a mut EnvironmentSettingsObject,
        _event_sender: &'a IpcSender<ContentEvent>,
        deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
    ) -> Self {
        Self { document_id, source_navigable_id, _document: document, settings, deferred_apple_keybinding }
    }
}

impl EventHandler for BlitzJSEventHandler<'_> {
    fn handle_event(&mut self, chain: &[usize], event: &mut DomEvent, doc: &mut dyn BlitzDocument, event_state: &mut EventState) {
        if input_debug_enabled() {
            let target_label = debug_blitz_node_label(doc, event.target);
            let chain_labels: Vec<_> = chain.iter().map(|n| debug_blitz_node_label(doc, *n).unwrap_or_else(|| format!("node#{n}"))).collect();
            trace!("[input-debug][content-dom] document={} traversable={} type={} target_node={} target_label={:?} chain={:?} chain_labels={:?}",
                self.document_id, self.source_navigable_id, event.name(), event.target, target_label, chain, chain_labels);
        }

        let time_stamp = self.settings.current_time_millis();
        let doc_et = self.settings.document.node.event_target.clone();
        let global_et = {
            let ec = &mut self.settings.realm_execution_context;
            let global_obj = ec.realm_global_object();
            ec.with_object_any(&global_obj)
                .and_then(|d| d.downcast_ref::<Window>())
                .map(|w| w.event_target.clone())
        };

        let ec = &mut self.settings.realm_execution_context;
        let view = Some(ec.realm_global_object());
        let ui_event = JsUiEvent::from_dom_event(event, view, time_stamp);
        let event_object = create_interface_instance::<Types, JsUiEvent>(ui_event, ec)
            .expect("UIEvent construction must succeed");
        let domain_event: Event = ec
            .with_object_any(&event_object)
            .and_then(|data| data.downcast_ref::<crate::dom::UIEvent>())
            .map(|uie| uie.event.clone())
            .expect("event_object must wrap a UIEvent");

        let path = build_event_path(chain, doc_et, global_et, ec);
        if let Err(error) = dispatch_with_path(ec, &path, &domain_event) {
            let error_msg = self.settings.ec().to_rust_string(error.clone()).unwrap_or_else(|_| format!("{error:?}"));
            error!("failed to dispatch UI event through JavaScript listeners: {error_msg}");
            return;
        }

        if let Some(ui_event) = ec.with_object_any(&event_object).and_then(|d| d.downcast_ref::<JsUiEvent>().cloned()) {
            ui_event.apply_to_event_state(event_state);
        }

        if let DomEventData::KeyDown(key_event) = &event.data && let Some(command) = apple_standard_keybinding_for_key_down(key_event) {
            *self.deferred_apple_keybinding.borrow_mut() = DeferredAppleStandardKeybinding { command: Some(command), keydown_default_prevented: event_state.is_cancelled() };
            event_state.prevent_default();
        }

        if let Err(error) = self.settings.perform_a_microtask_checkpoint() {
            error!("failed to run a microtask checkpoint after UI event dispatch: {error}");
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Dispatch a blitz UI event (pointer, key, wheel) through the DOM event system.
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
        trace!("[input-debug][content] document={} traversable={} event={}", document_id, source_navigable_id, ui_event_kind(&event));
    }
    let mut event = event;
    { let d = document.borrow(); localize_ui_event_for_document(&d, viewport_offset_x, viewport_offset_y, &mut event); }
    let mut document = document;
    let deferred = Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default()));
    let handler = BlitzJSEventHandler::new(
        document_id, source_navigable_id, parent_navigable_id, top_level_navigable_id,
        Rc::clone(&document), settings, event_sender, Rc::clone(&deferred),
    );
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    let dak = *deferred.borrow();
    if let Some(command) = dak.command && !dak.keydown_default_prevented {
        driver.handle_ui_event(UiEvent::AppleStandardKeybinding(SmolStr::new(command)));
    }
    if is_wheel && input_debug_enabled() {
        trace!("[input-debug][scroll] document={} traversable={} {}", document_id, source_navigable_id, debug_scroll_state(&document.borrow()));
    }
    Ok(())
}

/// Dispatch a trusted click event on a specific node.
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
    let deferred = Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default()));
    let handler = BlitzJSEventHandler::new(
        document_id, source_navigable_id, parent_navigable_id, top_level_navigable_id,
        document, settings, event_sender, deferred,
    );
    let time_millis = handler.settings.current_time_millis();
    let (target, event) = {
        let ec = handler.settings.ec();
        let target = crate::js::platform_objects::resolve_element_object(target_node_id, ec)
            .map_err(|e| format!("failed to resolve click target: {e:?}"))?;
        let event_domain = Event::new("click".into(), true, true, true, true, time_millis);
        let event = create_interface_instance::<Types, Event>(event_domain, ec)
            .map_err(|e| format!("failed to construct click event: {e:?}"))?;
        (target, event)
    };
    let ec = handler.settings.ec();
    let event_clone: Event = ec.with_object_any(&event)
        .and_then(|d| d.downcast_ref::<Event>().cloned())
        .ok_or_else(|| "dispatch_trusted_click_event: event is not an Event".to_string())?;
    let path = build_path_from_target_js_object(&target, ec);
    dispatch_with_path(ec, &path, &event_clone).map_err(|e| format!("failed to dispatch click event: {e:?}"))?;
    handler.settings.perform_a_microtask_checkpoint().map_err(|e| format!("microtask checkpoint after click: {e:?}"))
}
