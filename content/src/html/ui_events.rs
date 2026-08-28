use std::{cell::RefCell, rc::Rc};

use blitz_dom::{BaseDocument, Document as BlitzDocument, EventDriver, EventHandler};
use blitz_traits::SmolStr;
use blitz_traits::events::{BlitzKeyEvent, DomEvent, DomEventData, EventState, UiEvent};
use js_engine::ExecutionContext;
#[cfg(target_os = "macos")]
use keyboard_types::{Key, Modifiers as KeyboardModifiers};
use log::error;

use crate::dom::event::{Event, EventTarget};
use crate::dom::{EventPathItem, dispatch_with_path};
use crate::html::{EnvironmentSettingsObject, HTMLAnchorElement, Window};
use crate::js::Types;
use crate::ui_events::UIEvent as JsUiEvent;
use crate::webidl::bindings::create_interface_instance;

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
        UiEvent::KeyUp(_)
        | UiEvent::KeyDown(_)
        | UiEvent::Ime(_)
        | UiEvent::AppleStandardKeybinding(_) => {}
    }
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: This is the content-process portion of the dispatch algorithm's
// path-building steps (6.3, 6.9.6.2, and 6.9.9): chain[0] is the event
// target (step 6.3) and the remaining items are the ancestors appended by
// step 6.9.6.2 while walking "get the parent" (step 6.9.9); the document
// and window event targets close the path.
fn build_event_path(
    chain: &[usize],
    document_event_target: EventTarget,
    global_event_target: Option<EventTarget>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Vec<EventPathItem> {
    let mut path = Vec::with_capacity(chain.len() + 2);
    for (index, node_id) in chain.iter().enumerate() {
        // Step 6.3 / Step 6.9.6.2: Append to an event path with the event
        // target, then each ancestor, in target-to-root order.
        // <https://dom.spec.whatwg.org/#concept-event-dispatch>
        if let Ok(object) = crate::js::platform_objects::resolve_element_object(*node_id, ec) {
            if let Some(event_target) =
                crate::js::downcast::event_target_from_js_object(ec, &object)
            {
                // Step 6.9.6.1: If isActivationEvent is true, event's bubbles
                //               attribute is true, activationTarget is null,
                //               and parent has activation behavior, then set
                //               activationTarget to parent.
                // <https://dom.spec.whatwg.org/#concept-event-dispatch>
                // <https://html.spec.whatwg.org/#links-created-by-a-and-area-elements:activation-behaviour-2>
                let has_activation_behavior = ec
                    .with_object_any(&object)
                    .and_then(|data| data.downcast_ref::<HTMLAnchorElement>())
                    .map(|anchor| anchor.href_attribute().is_some())
                    .unwrap_or(false);
                path.push(EventPathItem {
                    invocation_target: event_target.clone(),
                    shadow_adjusted_target: (index == 0).then_some(event_target),
                    has_activation_behavior,
                });
            }
        }
    }
    // Step 6.9.9: If parent is non-null, then set parent to the result of
    // invoking parent's get the parent with event. (The document and window
    // close the parent chain.)
    // <https://dom.spec.whatwg.org/#concept-event-dispatch>
    path.push(EventPathItem {
        invocation_target: document_event_target,
        shadow_adjusted_target: None,
        has_activation_behavior: false,
    });
    if let Some(global_event_target) = global_event_target {
        path.push(EventPathItem {
            invocation_target: global_event_target,
            shadow_adjusted_target: None,
            has_activation_behavior: false,
        });
    }
    path
}

struct BlitzJSEventHandler<'a> {
    settings: &'a mut EnvironmentSettingsObject,
    deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
}

impl<'a> BlitzJSEventHandler<'a> {
    fn new(
        settings: &'a mut EnvironmentSettingsObject,
        deferred_apple_keybinding: Rc<RefCell<DeferredAppleStandardKeybinding>>,
    ) -> Self {
        Self {
            settings,
            deferred_apple_keybinding,
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
        let ui_event = JsUiEvent::from_dom_event(event, view, time_stamp, ec);
        let event_object = create_interface_instance::<Types, JsUiEvent>(ui_event, ec)
            .expect("UIEvent construction must succeed");
        let domain_event: Event = ec
            .with_object_any(&event_object)
            .and_then(|data| data.downcast_ref::<JsUiEvent>())
            .map(|uie| uie.event.clone())
            .expect("event_object must wrap a UIEvent");

        let path = build_event_path(chain, doc_et, global_et, ec);
        if let Err(error) = dispatch_with_path(ec, &path, &domain_event) {
            let error_msg = self
                .settings
                .ec()
                .to_rust_string(error.clone())
                .unwrap_or_else(|_| format!("{error:?}"));
            error!("failed to dispatch UI event through JavaScript listeners: {error_msg}");
            return;
        }

        if let Some(ui_event) = ec
            .with_object_any(&event_object)
            .and_then(|d| d.downcast_ref::<JsUiEvent>().cloned())
        {
            ui_event.apply_to_event_state(event_state, ec);
        }

        if let DomEventData::KeyDown(key_event) = &event.data
            && let Some(command) = apple_standard_keybinding_for_key_down(key_event)
        {
            *self.deferred_apple_keybinding.borrow_mut() = DeferredAppleStandardKeybinding {
                command: Some(command),
                keydown_default_prevented: event_state.is_cancelled(),
            };
            event_state.prevent_default();
        }

        if let Err(error) = self.settings.perform_a_microtask_checkpoint() {
            error!("failed to run a microtask checkpoint after UI event dispatch: {error}");
        }
    }
}

pub(crate) fn dispatch_ui_event(
    document: Rc<RefCell<BaseDocument>>,
    settings: &mut EnvironmentSettingsObject,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
    event: UiEvent,
) -> Result<(), String> {
    let mut event = event;
    {
        let d = document.borrow();
        localize_ui_event_for_document(&d, viewport_offset_x, viewport_offset_y, &mut event);
    }
    let mut document = document;
    let deferred = Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default()));
    let handler = BlitzJSEventHandler::new(settings, Rc::clone(&deferred));
    let mut driver = EventDriver::new(&mut document, handler);
    driver.handle_ui_event(event);
    let dak = *deferred.borrow();
    if let Some(command) = dak.command
        && !dak.keydown_default_prevented
    {
        driver.handle_ui_event(UiEvent::AppleStandardKeybinding(SmolStr::new(command)));
    }
    Ok(())
}

pub(crate) fn dispatch_trusted_click_event(
    settings: &mut EnvironmentSettingsObject,
    target_node_id: usize,
) -> Result<(), String> {
    let deferred = Rc::new(RefCell::new(DeferredAppleStandardKeybinding::default()));
    let handler = BlitzJSEventHandler::new(settings, deferred);
    let time_millis = handler.settings.current_time_millis();
    let event_domain = {
        let ec = handler.settings.ec();
        let event_object = create_interface_instance::<Types, Event>(
            Event::new("click".into(), true, true, true, true, time_millis, ec),
            ec,
        )
        .map_err(|error| format!("failed to create trusted click event: {error:?}"))?;
        ec.with_object_any(&event_object)
            .and_then(|data| data.downcast_ref::<Event>())
            .cloned()
            .ok_or_else(|| String::from("trusted click object does not contain an Event"))?
    };
    let path = {
        let ec = handler.settings.ec();
        let target = crate::js::platform_objects::resolve_element_object(target_node_id, ec)
            .map_err(|e| format!("failed to resolve click target: {e:?}"))?;
        crate::js::platform_objects::build_path_from_target_js_object(&target, ec)
    };
    let ec = handler.settings.ec();
    dispatch_with_path(ec, &path, &event_domain)
        .map_err(|e| format!("failed to dispatch click event: {e:?}"))?;
    handler
        .settings
        .perform_a_microtask_checkpoint()
        .map_err(|e| format!("microtask checkpoint after click: {e:?}"))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use blitz_dom::{BaseDocument, DocumentConfig};
    use serde_json::json;
    use url::Url;

    use crate::dom::{Event, dispatch_with_path};
    use crate::html::{
        EnvironmentSettingsObject, execute_parser_scripts, parse_html_into_document,
    };
    use crate::js::Types;
    use crate::js::platform_objects::{build_path_from_target_js_object, resolve_element_object};
    use crate::ui_events::UIEvent as JsUiEvent;
    use crate::webidl::bindings::create_interface_instance;

    use super::dispatch_trusted_click_event;

    fn new_document() -> Rc<RefCell<BaseDocument>> {
        Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())))
    }

    #[test]
    fn click_events_invoke_listener_in_child_realm() {
        let creation_url = Url::parse("about:blank").expect("parse creation URL");
        let mut parent_settings =
            EnvironmentSettingsObject::new(new_document(), creation_url.clone())
                .expect("build parent settings object");
        let child_document = new_document();
        let scripts = parse_html_into_document(
            &mut child_document.borrow_mut(),
            r#"<button id="target">Click</button>
                <script>
                    globalThis.clickCount = 0;
                    document.getElementById("target").addEventListener("click", function() {
                        globalThis.clickCount += 1;
                    });
                </script>"#,
        );
        let mut child_settings = EnvironmentSettingsObject::new_in_realm(
            Some(&mut parent_settings.realm_execution_context),
            Rc::clone(&child_document),
            creation_url,
            None,
            None,
        )
        .expect("build child settings object");
        execute_parser_scripts(&mut child_settings, scripts).expect("execute child script");
        let target_node_id = child_document
            .borrow()
            .query_selector("#target")
            .expect("query selector")
            .expect("find click target");
        dispatch_trusted_click_event(&mut child_settings, target_node_id)
            .expect("dispatch trusted click");

        assert_eq!(
            child_settings
                .evaluate_script_to_json("globalThis.clickCount")
                .expect("read click count"),
            json!(1),
        );

        child_settings
            .evaluate_script_to_json("globalThis.clickCount = 0")
            .expect("reset click count");
        let ui_event = {
            let ec = child_settings.ec();
            let event_object = create_interface_instance::<Types, JsUiEvent>(
                JsUiEvent {
                    event: Event::new("click".into(), true, true, false, true, 0.0, ec),
                    view: None,
                    detail: 0,
                },
                ec,
            )
            .expect("create UIEvent");
            ec.with_object_any(&event_object)
                .and_then(|data| data.downcast_ref::<JsUiEvent>())
                .map(|ui_event| ui_event.event.clone())
                .expect("read embedded Event")
        };
        let path = {
            let ec = child_settings.ec();
            let target = resolve_element_object(target_node_id, ec).expect("resolve click target");
            build_path_from_target_js_object(&target, ec)
        };
        dispatch_with_path(child_settings.ec(), &path, &ui_event).expect("dispatch UIEvent");

        assert_eq!(
            child_settings
                .evaluate_script_to_json("globalThis.clickCount")
                .expect("read UIEvent click count"),
            json!(1),
        );
    }
}
