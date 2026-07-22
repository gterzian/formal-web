//! Generic platform-object downcast helpers.
//!
//! These use [`ExecutionContext::with_object_any`] / `with_object_any_mut`
//! to extract native Rust data from JavaScript platform objects.

use crate::dom::{AbortController, AbortSignal, Document, Element, Event, EventTarget, Node, UIEvent};
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement,
    HTMLVideoElement, Window,
};
use crate::js::Types;
use js_engine::{Completion, ExecutionContext, JsTypes};

pub(crate) fn try_with_abort_signal_mut<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut AbortSignal) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("abort signal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any_mut(&obj) {
        if let Some(signal) = data.downcast_mut::<AbortSignal>() {
            return Ok(f(signal));
        }
    }
    Err(ec.new_type_error("receiver is not an AbortSignal"))
}

pub(crate) fn try_with_abort_signal_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortSignal) -> R,
) -> Completion<R, Types> {
    if let Some(data) = ec.with_object_any(object) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(f(signal));
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}

pub(crate) fn try_with_abort_controller_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortController) -> R,
) -> Completion<R, Types> {
    if let Some(data) = ec.with_object_any(object) {
        if let Some(controller) = data.downcast_ref::<AbortController>() {
            return Ok(f(controller));
        }
    }
    Err(ec.new_type_error("object is not an AbortController"))
}

pub(crate) fn try_set_event_target_reflector(
    value: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) {
    if let Some(obj) = <Types as JsTypes>::value_as_object(value) {
        let obj_clone = obj.clone();
        if let Some(data) = ec.with_object_any_mut(&obj) {
            // Walk all known platform object types that embed an EventTarget.
            if let Some(window) = data.downcast_mut::<Window>() {
                window.event_target.reflector = Some(obj_clone);
            } else if let Some(document) = data.downcast_mut::<Document>() {
                document.node.event_target.reflector = Some(obj_clone);
            } else if let Some(element) = data.downcast_mut::<Element>() {
                element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(html_element) = data.downcast_mut::<HTMLElement>() {
                html_element.element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(anchor) = data.downcast_mut::<HTMLAnchorElement>() {
                anchor.html_element.element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(iframe) = data.downcast_mut::<HTMLIFrameElement>() {
                iframe.html_element.element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                media.html_element.element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(input) = data.downcast_mut::<HTMLInputElement>() {
                input.html_element.element.node.event_target.reflector = Some(obj_clone);
            } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                video
                    .media_element
                    .html_element
                    .element
                    .node
                    .event_target
                    .reflector = Some(obj_clone);
            } else if let Some(node) = data.downcast_mut::<Node>() {
                node.event_target.reflector = Some(obj_clone);
            } else if let Some(target) = data.downcast_mut::<EventTarget>() {
                target.reflector = Some(obj_clone);
            } else if let Some(signal) = data.downcast_mut::<AbortSignal>() {
                signal.with_event_target_mut(|et| et.reflector = Some(obj_clone));
            } else if let Some(event) = data.downcast_mut::<Event>() {
                event.reflector = Some(obj_clone);
            } else if let Some(ui_event) = data.downcast_mut::<UIEvent>() {
                ui_event.event.reflector = Some(obj_clone);
            }
        }
    }
}

pub(crate) fn event_target_from_js_object(
    ec: &mut dyn ExecutionContext<Types>,
    object: &<Types as JsTypes>::JsObject,
) -> Option<EventTarget> {
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
        } else if let Some(event_target) = data.downcast_ref::<EventTarget>() {
            Some(event_target.clone())
        } else {
            None
        }
    })
}

pub(crate) fn try_with_event_target_mut<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut EventTarget) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("event target receiver is not an object"))?;

    if let Some(data) = ec.with_object_any_mut(&obj) {
        if let Some(window) = data.downcast_mut::<Window>() {
            return Ok(f(&mut window.event_target));
        }
        if let Some(document) = data.downcast_mut::<Document>() {
            return Ok(f(&mut document.node.event_target));
        }
        if let Some(element) = data.downcast_mut::<Element>() {
            return Ok(f(&mut element.node.event_target));
        }
        if let Some(html_element) = data.downcast_mut::<HTMLElement>() {
            return Ok(f(&mut html_element.element.node.event_target));
        }
        if let Some(anchor) = data.downcast_mut::<HTMLAnchorElement>() {
            return Ok(f(&mut anchor.html_element.element.node.event_target));
        }
        if let Some(iframe) = data.downcast_mut::<HTMLIFrameElement>() {
            return Ok(f(&mut iframe.html_element.element.node.event_target));
        }
        if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
            return Ok(f(&mut media.html_element.element.node.event_target));
        }
        if let Some(input) = data.downcast_mut::<HTMLInputElement>() {
            return Ok(f(&mut input.html_element.element.node.event_target));
        }
        if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
            return Ok(f(&mut video
                .media_element
                .html_element
                .element
                .node
                .event_target));
        }
        if let Some(node) = data.downcast_mut::<Node>() {
            return Ok(f(&mut node.event_target));
        }
        if let Some(target) = data.downcast_mut::<EventTarget>() {
            return Ok(f(target));
        }
    }
    // `data` borrow dropped; use immutable downcast for AbortSignal.
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(signal.with_event_target_mut(f));
        }
    }
    Err(ec.new_type_error("receiver is not an EventTarget"))
}

pub(crate) fn with_abort_signal_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortSignal) -> R,
) -> Completion<R, Types> {
    let type_error = ec.new_type_error("object is not an AbortSignal");
    let signal = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<AbortSignal>())
        .ok_or(type_error)?;
    Ok(f(signal))
}
