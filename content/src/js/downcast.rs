//! Generic platform-object downcast helpers.
//!
//! These use [`ExecutionContext::with_object_any`] / `with_object_any_mut`
//! to extract native Rust data from JavaScript platform objects.

use crate::dom::{
    AbortController, AbortSignal, Document, Element, Event, EventTarget, HasEvent, Node,
};
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement,
    HTMLVideoElement, MessageEvent, MessagePort, Window, Worker, WorkerGlobalScope,
};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::ui_events::{MouseEvent, UIEvent};
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;

/// Downcasts a JS platform object to its embedded `Event` (the base `Event`
/// itself, or the `event` field of an Event subclass). Event subclasses must
/// embed the base `Event` and implement `HasEvent` so this single walk finds it.
pub(crate) fn event_from_js_object(
    ec: &dyn ExecutionContext<Types>,
    object: &<Types as JsTypes>::JsObject,
) -> Option<Event> {
    ec.with_object_any(object).and_then(|data| {
        data.downcast_ref::<Event>()
            .map(|event| event.event().clone())
            .or_else(|| {
                data.downcast_ref::<UIEvent>()
                    .map(|ui_event| ui_event.event().clone())
            })
            .or_else(|| {
                data.downcast_ref::<MessageEvent>()
                    .map(|message_event| message_event.event().clone())
            })
            .or_else(|| {
                data.downcast_ref::<MouseEvent>()
                    .map(|mouse_event| mouse_event.event().clone())
            })
    })
}

pub(crate) fn try_with_abort_signal_mut<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut AbortSignal, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("abort signal receiver is not an object"))?;
    let mut result = Err(ec.new_type_error("receiver is not an AbortSignal"));
    ec.with_object_any_mut_with(
        &obj,
        Box::new(|data, ec| {
            if let Some(signal) = data.downcast_mut::<AbortSignal>() {
                result = Ok(f(signal, ec));
            }
        }),
    );
    result
}

pub(crate) fn try_with_abort_signal_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortSignal, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let signal = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(signal) = signal else {
        return Err(ec.new_type_error("object is not an AbortSignal"));
    };
    Ok(f(&signal, ec))
}

pub(crate) fn try_with_abort_controller_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortController, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let controller = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<AbortController>().cloned());
    let Some(controller) = controller else {
        return Err(ec.new_type_error("object is not an AbortController"));
    };
    Ok(f(&controller, ec))
}

pub(crate) fn try_set_event_target_reflector(
    value: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) {
    if let Some(obj) = <Types as JsTypes>::value_as_object(value) {
        let reflector = obj.clone();
        // Walk all known platform object types that embed an EventTarget.
        // The reflector slot is written through `store_js_object` so the V8
        // backend converts the stored handle into a cppgc edge (the cycle
        // between the wrapper and its platform object becomes collectable).
        ec.with_object_any_mut_with(
            &obj,
            Box::new(move |data, ec| {
                if let Some(window) = data.downcast_mut::<Window>() {
                    ec.store_js_object(&mut window.event_target.reflector, reflector);
                } else if let Some(document) = data.downcast_mut::<Document>() {
                    ec.store_js_object(&mut document.node.event_target.reflector, reflector);
                } else if let Some(element) = data.downcast_mut::<Element>() {
                    ec.store_js_object(&mut element.node.event_target.reflector, reflector);
                } else if let Some(html_element) = data.downcast_mut::<HTMLElement>() {
                    ec.store_js_object(
                        &mut html_element.element.node.event_target.reflector,
                        reflector,
                    );
                } else if let Some(anchor) = data.downcast_mut::<HTMLAnchorElement>() {
                    ec.store_js_object(
                        &mut anchor.html_element.element.node.event_target.reflector,
                        reflector,
                    );
                } else if let Some(iframe) = data.downcast_mut::<HTMLIFrameElement>() {
                    ec.store_js_object(
                        &mut iframe.html_element.element.node.event_target.reflector,
                        reflector,
                    );
                } else if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                    ec.store_js_object(
                        &mut media.html_element.element.node.event_target.reflector,
                        reflector,
                    );
                } else if let Some(input) = data.downcast_mut::<HTMLInputElement>() {
                    ec.store_js_object(
                        &mut input.html_element.element.node.event_target.reflector,
                        reflector,
                    );
                } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                    ec.store_js_object(
                        &mut video
                            .media_element
                            .html_element
                            .element
                            .node
                            .event_target
                            .reflector,
                        reflector,
                    );
                } else if let Some(node) = data.downcast_mut::<Node>() {
                    ec.store_js_object(&mut node.event_target.reflector, reflector);
                } else if let Some(target) = data.downcast_mut::<EventTarget>() {
                    ec.store_js_object(&mut target.reflector, reflector);
                } else if let Some(port) = data.downcast_mut::<MessagePort>() {
                    ec.store_js_object(&mut port.event_target.reflector, reflector);
                } else if let Some(worker) = data.downcast_mut::<Worker>() {
                    ec.store_js_object(&mut worker.event_target.reflector, reflector.clone());
                    // The owner realm's GlobalScope registered a clone of
                    // this event target (the target the worker's message and
                    // error events fire at); EventTarget clones share their
                    // listener state but not their reflector slot, so mirror
                    // the reflector onto the registered copy.
                    let worker_id = worker.worker_id;
                    if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
                        global_scope.sync_owned_worker_reflector(worker_id, reflector, ec);
                        Ok(())
                    }) {
                        error!(
                            "failed to sync the reflector of owned worker {worker_id}: {}",
                            error.display()
                        );
                    }
                } else if let Some(worker_global_scope) = data.downcast_mut::<WorkerGlobalScope>() {
                    ec.store_js_object(&mut worker_global_scope.event_target.reflector, reflector);
                } else if let Some(signal) = data.downcast_mut::<AbortSignal>() {
                    // AbortSignal exposes its EventTarget through a shared
                    // cell, so its setter borrows the cell (the clone shares
                    // the same cell).
                    let signal = signal.clone();
                    signal.with_event_target_mut(
                        move |event_target, ec| {
                            ec.store_js_object(&mut event_target.reflector, reflector)
                        },
                        ec,
                    );
                } else if let Some(event) = data.downcast_mut::<Event>() {
                    ec.store_js_object(&mut event.event_mut().reflector, reflector);
                } else if let Some(message_event) = data.downcast_mut::<MessageEvent>() {
                    ec.store_js_object(&mut message_event.event_mut().reflector, reflector);
                } else if let Some(ui_event) = data.downcast_mut::<UIEvent>() {
                    ec.store_js_object(&mut ui_event.event_mut().reflector, reflector);
                } else if let Some(mouse_event) = data.downcast_mut::<MouseEvent>() {
                    ec.store_js_object(&mut mouse_event.event_mut().reflector, reflector);
                }
            }),
        );
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
        } else if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            Some(anchor.html_element.element.node.event_target.clone())
        } else if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            Some(iframe.html_element.element.node.event_target.clone())
        } else if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            Some(input.html_element.element.node.event_target.clone())
        } else if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
            Some(media.html_element.element.node.event_target.clone())
        } else if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            Some(
                video
                    .media_element
                    .html_element
                    .element
                    .node
                    .event_target
                    .clone(),
            )
        } else if let Some(node) = data.downcast_ref::<Node>() {
            Some(node.event_target.clone())
        } else if let Some(port) = data.downcast_ref::<MessagePort>() {
            Some(port.event_target.clone())
        } else if let Some(worker) = data.downcast_ref::<Worker>() {
            Some(worker.event_target.clone())
        } else if let Some(worker_global_scope) = data.downcast_ref::<WorkerGlobalScope>() {
            Some(worker_global_scope.event_target.clone())
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
    f: impl FnOnce(&mut EventTarget, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("event target receiver is not an object"))?;

    // `with_object_any_mut_with` passes both the registry data and the
    // execution context to the closure, so the platform object can be
    // mutated in place while `f` uses `ec`. The AbortSignal path (which
    // exposes its EventTarget through the shared cell) is handled in the
    // same closure so `f` runs exactly once.
    let mut result = Err(ec.new_type_error("receiver is not an EventTarget"));
    ec.with_object_any_mut_with(
        &obj,
        Box::new(|data, ec| {
            // Walk all known platform object types that embed an EventTarget.
            if let Some(window) = data.downcast_mut::<Window>() {
                result = Ok(f(&mut window.event_target, ec));
            } else if let Some(document) = data.downcast_mut::<Document>() {
                result = Ok(f(&mut document.node.event_target, ec));
            } else if let Some(element) = data.downcast_mut::<Element>() {
                result = Ok(f(&mut element.node.event_target, ec));
            } else if let Some(html_element) = data.downcast_mut::<HTMLElement>() {
                result = Ok(f(&mut html_element.element.node.event_target, ec));
            } else if let Some(anchor) = data.downcast_mut::<HTMLAnchorElement>() {
                result = Ok(f(&mut anchor.html_element.element.node.event_target, ec));
            } else if let Some(iframe) = data.downcast_mut::<HTMLIFrameElement>() {
                result = Ok(f(&mut iframe.html_element.element.node.event_target, ec));
            } else if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                result = Ok(f(&mut media.html_element.element.node.event_target, ec));
            } else if let Some(input) = data.downcast_mut::<HTMLInputElement>() {
                result = Ok(f(&mut input.html_element.element.node.event_target, ec));
            } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                result = Ok(f(
                    &mut video.media_element.html_element.element.node.event_target,
                    ec,
                ));
            } else if let Some(node) = data.downcast_mut::<Node>() {
                result = Ok(f(&mut node.event_target, ec));
            } else if let Some(target) = data.downcast_mut::<EventTarget>() {
                result = Ok(f(target, ec));
            } else if let Some(port) = data.downcast_mut::<MessagePort>() {
                result = Ok(f(&mut port.event_target, ec));
            } else if let Some(worker) = data.downcast_mut::<Worker>() {
                result = Ok(f(&mut worker.event_target, ec));
            } else if let Some(worker_global_scope) = data.downcast_mut::<WorkerGlobalScope>() {
                result = Ok(f(&mut worker_global_scope.event_target, ec));
            } else if let Some(signal) = data.downcast_mut::<AbortSignal>() {
                // The closure receives the execution context that
                // `with_event_target_mut` passes alongside the borrowed
                // event target.
                result =
                    Ok(signal.with_event_target_mut(|event_target, ec| f(event_target, ec), ec));
            }
        }),
    );
    result
}

pub(crate) fn with_abort_signal_ref<R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&AbortSignal, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let signal = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned())
        .ok_or_else(|| ec.new_type_error("object is not an AbortSignal"))?;
    Ok(f(&signal, ec))
}
