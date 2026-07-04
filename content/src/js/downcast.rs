//! Generic platform-object downcast helpers.
//!
//! These use [`ExecutionContext::with_object_any`] / `with_object_any_mut`
//! to extract native Rust data from JavaScript platform objects.

use crate::dom::{
    AbortController, AbortSignal, Document, Element, Event, EventTarget, Node, UIEvent,
};
#[cfg(boa_backend)]
use crate::html::HTMLMediaElement;
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLVideoElement, Window,
};
use crate::js::Types;
use js_engine::{Completion, ExecutionContext, JsTypes};

#[cfg(boa_backend)]
use boa_engine::{JsNativeError, JsResult, object::JsObject};

/// Downcast `this` to `&mut AbortSignal` via `with_object_any_mut`.
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

/// Downcast `object` to `&AbortSignal` via `with_object_any`.
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

/// Downcast `object` to `&AbortController` via `with_object_any`.
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

/// Downcast `this` to `&mut Event` (or [`UIEvent`] delegate) via `with_object_any_mut`.
pub(crate) fn try_with_event_mut<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut Event) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("event receiver is not an object"))?;
    if let Some(data) = ec.with_object_any_mut(&obj) {
        if let Some(event) = data.downcast_mut::<Event>() {
            return Ok(f(event));
        }
        if let Some(ui_event) = data.downcast_mut::<UIEvent>() {
            return Ok(f(&mut ui_event.event));
        }
    }
    Err(ec.new_type_error("receiver is not an Event"))
}

/// Downcast `this` to `&mut EventTarget` via `with_object_any_mut`.
///
/// Walks all known platform-object types that contain an `EventTarget` field.
/// Gated behind `#[cfg(boa_backend)]` because `HTMLMediaElement` still requires `boa_engine`.
#[cfg(boa_backend)]
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

/// Downcast `this` to `&EventTarget` via `with_object_any`.
#[cfg(boa_backend)]
pub(crate) fn try_with_event_target_ref<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&EventTarget) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("event target receiver is not an object"))?;

    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(window) = data.downcast_ref::<Window>() {
            return Ok(f(&window.event_target));
        }
        if let Some(document) = data.downcast_ref::<Document>() {
            return Ok(f(&document.node.event_target));
        }
        if let Some(element) = data.downcast_ref::<Element>() {
            return Ok(f(&element.node.event_target));
        }
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(f(&html_element.element.node.event_target));
        }
        if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&anchor.html_element.element.node.event_target));
        }
        if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(&iframe.html_element.element.node.event_target));
        }
        if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
            return Ok(f(&media.html_element.element.node.event_target));
        }
        if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            return Ok(f(&input.html_element.element.node.event_target));
        }
        if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            return Ok(f(&video
                .media_element
                .html_element
                .element
                .node
                .event_target));
        }
        if let Some(node) = data.downcast_ref::<Node>() {
            return Ok(f(&node.event_target));
        }
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(signal.with_event_target_ref(f));
        }
        if let Some(target) = data.downcast_ref::<EventTarget>() {
            return Ok(f(target));
        }
    }
    Err(ec.new_type_error("receiver is not an EventTarget"))
}

// ── Boa-specific helpers — used by unconverted modules ────────────────

/// Boa-specific: downcast via `JsObject::downcast_ref`.
#[cfg(boa_backend)]
pub(crate) fn with_abort_signal_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortSignal) -> R,
) -> JsResult<R> {
    let signal = object
        .downcast_ref::<AbortSignal>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not an AbortSignal"))?;
    Ok(f(&signal))
}
