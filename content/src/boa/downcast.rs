use boa_engine::{JsNativeError, JsResult, JsValue, object::JsObject};

use crate::dom::{
    AbortController, AbortSignal, Document, Element, Event, EventTarget, Node, UIEvent,
};
use crate::html::{HTMLAnchorElement, HTMLElement, Window};

pub(crate) fn with_abort_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortController) -> R,
) -> JsResult<R> {
    let controller = object
        .downcast_ref::<AbortController>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not an AbortController"))?;
    Ok(f(&controller))
}

pub(crate) fn with_abort_signal_mut<R>(
    this: &JsValue,
    f: impl FnOnce(&mut AbortSignal) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("abort signal receiver is not an object")
    })?;
    let Some(mut signal) = object.downcast_mut::<AbortSignal>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not an AbortSignal")
            .into());
    };
    Ok(f(&mut signal))
}

pub(crate) fn with_abort_signal_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortSignal) -> R,
) -> JsResult<R> {
    let signal = object
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not an AbortSignal"))?;
    Ok(f(&signal))
}

pub(crate) fn is_abort_signal_object(object: &JsObject) -> bool {
    object.downcast_ref::<AbortSignal>().is_some()
}

pub(crate) fn with_event_mut<R>(this: &JsValue, f: impl FnOnce(&mut Event) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("event receiver is not an object"))?;
    if let Some(mut event) = object.downcast_mut::<Event>() {
        return Ok(f(&mut event));
    }
    if let Some(mut ui_event) = object.downcast_mut::<UIEvent>() {
        return Ok(f(&mut ui_event.event));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not an Event")
        .into())
}

pub(crate) fn with_event_target_mut<R>(
    this: &JsValue,
    f: impl FnOnce(&mut EventTarget) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("event target receiver is not an object")
    })?;
    if let Some(mut window) = object.downcast_mut::<Window>() {
        return Ok(f(&mut window.event_target));
    }
    if let Some(mut document) = object.downcast_mut::<Document>() {
        return Ok(f(&mut document.node.event_target));
    }
    if let Some(mut element) = object.downcast_mut::<Element>() {
        return Ok(f(&mut element.node.event_target));
    }
    if let Some(mut html_element) = object.downcast_mut::<HTMLElement>() {
        return Ok(f(&mut html_element.element.node.event_target));
    }
    if let Some(mut html_anchor_element) = object.downcast_mut::<HTMLAnchorElement>() {
        return Ok(f(&mut html_anchor_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(mut node) = object.downcast_mut::<Node>() {
        return Ok(f(&mut node.event_target));
    }
    if let Some(mut signal) = object.downcast_mut::<AbortSignal>() {
        return Ok(f(&mut signal.event_target));
    }
    if let Some(mut target) = object.downcast_mut::<EventTarget>() {
        return Ok(f(&mut target));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not an EventTarget")
        .into())
}

pub(crate) fn with_event_target_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&EventTarget) -> R,
) -> JsResult<R> {
    if let Some(window) = object.downcast_ref::<Window>() {
        return Ok(f(&window.event_target));
    }
    if let Some(document) = object.downcast_ref::<Document>() {
        return Ok(f(&document.node.event_target));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(f(&element.node.event_target));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(f(&html_element.element.node.event_target));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&html_anchor_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(f(&node.event_target));
    }
    if let Some(signal) = object.downcast_ref::<AbortSignal>() {
        return Ok(f(&signal.event_target));
    }
    if let Some(target) = object.downcast_ref::<EventTarget>() {
        return Ok(f(&target));
    }
    Err(JsNativeError::typ()
        .with_message("object is not an EventTarget")
        .into())
}
