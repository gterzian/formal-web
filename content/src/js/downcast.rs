//! Generic platform-object downcast helpers.
//!
//! These use [`ExecutionContext::with_object_any`] / `with_object_any_mut`
//! to extract native Rust data from JavaScript platform objects.

use crate::dom::{
    AbortController, AbortSignal, Document, Element, Event, EventTarget, HasEvent, Node,
};
use crate::html::{
    CanvasRenderingContext2D, DedicatedWorkerGlobalScope, HTMLAnchorElement, HTMLCanvasElement,
    HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement, HTMLVideoElement,
    MessageEvent, MessagePort, OffscreenCanvas, OffscreenCanvasRenderingContext2D, Window, Worker,
    WorkerGlobalScope,
};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::ui_events::{MouseEvent, UIEvent};
use js_engine::{Completion, ExecutionContext, JsTypes};
use log::error;
use std::any::Any;

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

/// Run `f` on the `EventTarget` embedded in a platform object's native data.
///
/// The reference is passed to `f` while the caller holds
/// [`ExecutionContext::with_object_any_mut`]'s borrow, so no engine method can
/// run while `f` uses it.
fn with_platform_event_target_mut<R>(
    data: &mut dyn Any,
    f: impl FnOnce(&mut EventTarget) -> R,
) -> Option<R> {
    macro_rules! target {
        ($ty:ty, $value:ident, $field:expr) => {
            if let Some($value) = data.downcast_mut::<$ty>() {
                return Some(f(&mut $field));
            }
        };
    }
    target!(Window, window, window.event_target);
    target!(Document, document, document.node.event_target);
    target!(Element, element, element.node.event_target);
    target!(
        HTMLElement,
        html_element,
        html_element.element.node.event_target
    );
    target!(
        HTMLAnchorElement,
        anchor,
        anchor.html_element.element.node.event_target
    );
    target!(
        HTMLCanvasElement,
        canvas,
        canvas.html_element.element.node.event_target
    );
    target!(
        HTMLIFrameElement,
        iframe,
        iframe.html_element.element.node.event_target
    );
    target!(
        HTMLMediaElement,
        media,
        media.html_element.element.node.event_target
    );
    target!(
        HTMLInputElement,
        input,
        input.html_element.element.node.event_target
    );
    target!(
        HTMLVideoElement,
        video,
        video.media_element.html_element.element.node.event_target
    );
    target!(Node, node, node.event_target);
    if let Some(target_value) = data.downcast_mut::<EventTarget>() {
        return Some(f(target_value));
    }
    target!(MessagePort, port, port.event_target);
    target!(Worker, worker, worker.event_target);
    target!(
        DedicatedWorkerGlobalScope,
        dedicated_scope,
        dedicated_scope.worker_global_scope.event_target
    );
    target!(
        WorkerGlobalScope,
        worker_global_scope,
        worker_global_scope.event_target
    );
    None
}

/// Run `f` on the JS reflector slot of a platform object's native data.
/// Event subclasses store the reflector on their embedded `Event`.
fn with_platform_reflector_slot_mut<R>(
    data: &mut dyn Any,
    f: impl FnOnce(&mut Option<<Types as JsTypes>::JsObject>) -> R,
) -> Option<R> {
    macro_rules! slot {
        ($ty:ty, $value:ident, $field:expr) => {
            if let Some($value) = data.downcast_mut::<$ty>() {
                return Some(f(&mut $field.reflector));
            }
        };
    }
    slot!(Window, window, window.event_target);
    slot!(Document, document, document.node.event_target);
    slot!(Element, element, element.node.event_target);
    slot!(
        HTMLElement,
        html_element,
        html_element.element.node.event_target
    );
    slot!(
        HTMLAnchorElement,
        anchor,
        anchor.html_element.element.node.event_target
    );
    slot!(
        HTMLCanvasElement,
        canvas,
        canvas.html_element.element.node.event_target
    );
    slot!(
        HTMLIFrameElement,
        iframe,
        iframe.html_element.element.node.event_target
    );
    slot!(
        HTMLMediaElement,
        media,
        media.html_element.element.node.event_target
    );
    slot!(
        HTMLInputElement,
        input,
        input.html_element.element.node.event_target
    );
    slot!(
        HTMLVideoElement,
        video,
        video.media_element.html_element.element.node.event_target
    );
    slot!(Node, node, node.event_target);
    if let Some(target_value) = data.downcast_mut::<EventTarget>() {
        return Some(f(&mut target_value.reflector));
    }
    slot!(MessagePort, port, port.event_target);
    slot!(Worker, worker, worker.event_target);
    slot!(
        DedicatedWorkerGlobalScope,
        dedicated_scope,
        dedicated_scope.worker_global_scope.event_target
    );
    slot!(
        WorkerGlobalScope,
        worker_global_scope,
        worker_global_scope.event_target
    );
    if let Some(context) = data.downcast_mut::<CanvasRenderingContext2D>() {
        return Some(f(&mut context.reflector));
    }
    if let Some(context) = data.downcast_mut::<OffscreenCanvasRenderingContext2D>() {
        return Some(f(&mut context.reflector));
    }
    if let Some(canvas) = data.downcast_mut::<OffscreenCanvas>() {
        return Some(f(&mut canvas.reflector));
    }
    if let Some(event) = data.downcast_mut::<Event>() {
        return Some(f(&mut event.event_mut().reflector));
    }
    if let Some(message_event) = data.downcast_mut::<MessageEvent>() {
        return Some(f(&mut message_event.event_mut().reflector));
    }
    if let Some(ui_event) = data.downcast_mut::<UIEvent>() {
        return Some(f(&mut ui_event.event_mut().reflector));
    }
    if let Some(mouse_event) = data.downcast_mut::<MouseEvent>() {
        return Some(f(&mut mouse_event.event_mut().reflector));
    }
    None
}

/// Clone `T` out of a platform object, run `f` with the execution context,
/// and write the clone back.
///
/// The engine calls in `f` run while no reference into the platform data is
/// live, so an allocation that triggers a cppgc trace cannot alias a mutable
/// borrow. `T`'s shared cells carry their mutations; its direct fields are
/// carried back by the write-back.
pub(crate) fn with_cloned_platform_mut<T, R>(
    object: &<Types as JsTypes>::JsObject,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut T, &mut dyn ExecutionContext<Types>) -> R,
) -> Option<R>
where
    T: Clone + 'static,
{
    let mut platform = ec
        .with_object_any(object)
        .and_then(|data| data.downcast_ref::<T>().cloned())?;
    let result = f(&mut platform, ec);
    if let Some(slot) = ec
        .with_object_any_mut(object)
        .and_then(|data| data.downcast_mut::<T>())
    {
        *slot = platform;
    }
    Some(result)
}

pub(crate) fn try_with_abort_signal_mut<R>(
    this: &<Types as JsTypes>::JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&mut AbortSignal, &mut dyn ExecutionContext<Types>) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("abort signal receiver is not an object"))?;
    // The signal's state lives in a shared cell, so the clone sees every
    // mutation and no write-back is needed.
    let signal = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned());
    let Some(mut signal) = signal else {
        return Err(ec.new_type_error("receiver is not an AbortSignal"));
    };
    Ok(f(&mut signal, ec))
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
        // Convert the reflector into a cppgc edge before taking any platform
        // borrow, so the assignment below needs no engine call.
        let mut reflector = None;
        ec.store_js_object(&mut reflector, obj.clone());

        // AbortSignal keeps its event target in a shared cell; its own method
        // borrows the cell and writes the slot.
        if let Some(signal) = ec
            .with_object_any(&obj)
            .and_then(|data| data.downcast_ref::<AbortSignal>().cloned())
        {
            signal.with_event_target_mut(|target, _ec| target.reflector = reflector.clone(), ec);
            return;
        }

        // Set the reflector on the embedded target; capture the Worker id so
        // the owner realm's registered clone can be synced after the borrow
        // is released.
        let worker_id = ec.with_object_any_mut(&obj).and_then(|data| {
            let worker_id = data.downcast_ref::<Worker>().map(|worker| worker.worker_id);
            with_platform_reflector_slot_mut(data, |slot| *slot = reflector.clone());
            worker_id
        });

        if let Some(worker_id) = worker_id {
            // The owner realm's GlobalScope registered a clone of this event
            // target (the target the worker's message and error events fire
            // at); EventTarget clones share their listener state but not
            // their reflector slot, so mirror the reflector onto it.
            if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
                global_scope.sync_owned_worker_reflector(
                    worker_id,
                    reflector.expect("the worker reflector was just stored"),
                    ec,
                );
                Ok(())
            }) {
                error!(
                    "failed to sync the reflector of owned worker {worker_id}: {}",
                    error.display()
                );
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
        } else if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            Some(anchor.html_element.element.node.event_target.clone())
        } else if let Some(canvas) = data.downcast_ref::<HTMLCanvasElement>() {
            Some(canvas.html_element.element.node.event_target.clone())
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
        } else if let Some(dedicated_scope) = data.downcast_ref::<DedicatedWorkerGlobalScope>() {
            Some(dedicated_scope.worker_global_scope.event_target.clone())
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

    // AbortSignal exposes its EventTarget through a shared cell; clone the
    // signal and let its own method borrow the cell soundly.
    if let Some(signal) = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<AbortSignal>().cloned())
    {
        return Ok(signal.with_event_target_mut(|target, ec| f(target, ec), ec));
    }

    // Clone the embedded target out, run `f` with the execution context, and
    // write the clone back without holding a platform borrow while `ec` runs.
    let target = ec
        .with_object_any_mut(&obj)
        .and_then(|data| with_platform_event_target_mut(data, |target| target.clone()));
    let Some(mut target) = target else {
        return Err(ec.new_type_error("receiver is not an EventTarget"));
    };
    let result = f(&mut target, ec);
    ec.with_object_any_mut(&obj)
        .and_then(|data| with_platform_event_target_mut(data, |slot| *slot = target));
    Ok(result)
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
