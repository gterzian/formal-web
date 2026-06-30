use boa_engine::{JsNativeError, JsResult, JsValue, object::JsObject};

use crate::dom::{
    AbortController, AbortSignal, Document, Element, Event, EventTarget, Node, UIEvent,
};

use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement,
    HTMLVideoElement, Window,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

pub(crate) fn with_abort_controller_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&AbortController) -> R,
) -> JsResult<R> {
    let controller = object
        .downcast_ref::<AbortController>()
        .ok_or_else(|| JsNativeError::typ().with_message("object is not an AbortController"))?;
    Ok(f(&controller))
}

pub(crate) fn try_with_abort_controller_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&AbortController) -> R,
) -> Completion<R, crate::js::Types> {
    if let Some(data) = ec.with_object_any(object) {
        if let Some(controller) = data.downcast_ref::<AbortController>() {
            return Ok(f(controller));
        }
    }
    Err(ec.new_type_error("object is not an AbortController"))
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
    if let Some(mut html_iframe_element) = object.downcast_mut::<HTMLIFrameElement>() {
        return Ok(f(&mut html_iframe_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(mut html_media_element) = object.downcast_mut::<HTMLMediaElement>() {
        return Ok(f(&mut html_media_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(mut html_input_element) = object.downcast_mut::<HTMLInputElement>() {
        return Ok(f(&mut html_input_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(mut html_video_element) = object.downcast_mut::<HTMLVideoElement>() {
        return Ok(f(&mut html_video_element
            .media_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(mut node) = object.downcast_mut::<Node>() {
        return Ok(f(&mut node.event_target));
    }
    if let Some(signal) = object.downcast_ref::<AbortSignal>() {
        return Ok(signal.with_event_target_mut(f));
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
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&html_iframe_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(html_media_element) = object.downcast_ref::<HTMLMediaElement>() {
        return Ok(f(&html_media_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(html_input_element) = object.downcast_ref::<HTMLInputElement>() {
        return Ok(f(&html_input_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(html_video_element) = object.downcast_ref::<HTMLVideoElement>() {
        return Ok(f(&html_video_element
            .media_element
            .html_element
            .element
            .node
            .event_target));
    }
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(f(&node.event_target));
    }
    if let Some(signal) = object.downcast_ref::<AbortSignal>() {
        return Ok(signal.with_event_target_ref(f));
    }
    if let Some(target) = object.downcast_ref::<EventTarget>() {
        return Ok(f(&target));
    }
    Err(JsNativeError::typ()
        .with_message("object is not an EventTarget")
        .into())
}

// ═══════════════════════════════════════════════════════════════════════════
// Generic try_* variants — use ec.with_object_any() / with_object_any_mut()
// instead of JsObject::downcast_ref / downcast_mut.
// ═══════════════════════════════════════════════════════════════════════════

pub(crate) fn try_with_event_target_mut<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&mut EventTarget) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("event target receiver is not an object"))?;

    // Mutable downcast chain — all types that expose &mut EventTarget.
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
            return Ok(f(
                &mut video.media_element.html_element.element.node.event_target,
            ));
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

pub(crate) fn try_with_event_target_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&EventTarget) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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
            return Ok(f(
                &video.media_element.html_element.element.node.event_target,
            ));
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

pub(crate) fn try_with_abort_signal_mut<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&mut AbortSignal) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("abort signal receiver is not an object"))?;
    if let Some(data) = ec.with_object_any_mut(&obj) {
        if let Some(signal) = data.downcast_mut::<AbortSignal>() {
            return Ok(f(signal));
        }
    }
    Err(ec.new_type_error("receiver is not an AbortSignal"))
}

pub(crate) fn try_with_event_mut<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&mut Event) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
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

pub(crate) fn try_with_abort_signal_ref<R>(
    object: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&AbortSignal) -> R,
) -> Completion<R, crate::js::Types> {
    if let Some(data) = ec.with_object_any(object) {
        if let Some(signal) = data.downcast_ref::<AbortSignal>() {
            return Ok(f(signal));
        }
    }
    Err(ec.new_type_error("object is not an AbortSignal"))
}
