//! Generic platform-object downcast helpers.
//!
//! These use [`ExecutionContext::with_object_any`] / `with_object_any_mut`
//! to extract native Rust data from JavaScript platform objects.

use crate::dom::{
    AbortController, AbortSignal, Attr, Document, Element, Event, EventTarget, HasEvent,
    NamedNodeMap, Node,
};
use crate::fetch::Headers;
use crate::html::{
    CanvasRenderingContext2D, DedicatedWorkerGlobalScope, HTMLAnchorElement, HTMLCanvasElement,
    HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLLinkElement, HTMLMediaElement,
    HTMLScriptElement, HTMLVideoElement, MessageEvent, MessagePort, OffscreenCanvas,
    OffscreenCanvasRenderingContext2D, PromiseRejectionEvent, Window, Worker, WorkerGlobalScope,
};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
#[cfg(feature = "webrtc")]
use crate::mediacapture_streams::{
    MediaDevices, MediaStream, MediaStreamTrack, MediaStreamTrackEvent,
};
use crate::ui_events::{MouseEvent, UIEvent};
use crate::url_standard::URLSearchParams;
#[cfg(feature = "webrtc")]
use crate::webrtc::{
    RTCDataChannel, RTCDataChannelEvent, RTCIceCandidate, RTCPeerConnection,
    RTCPeerConnectionIceEvent, RTCRtpReceiver, RTCRtpSender, RTCRtpTransceiver,
    RTCSessionDescription, RTCTrackEvent,
};
use crate::websockets::{CloseEvent, WebSocket};
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
            .or_else(|| {
                data.downcast_ref::<PromiseRejectionEvent>()
                    .map(|rejection_event| rejection_event.event().clone())
            })
            .or_else(|| webrtc_event(data))
            .or_else(|| {
                data.downcast_ref::<CloseEvent>()
                    .map(|close_event| close_event.event().clone())
            })
    })
}

/// The embedded `Event` of the WebRTC and Media Capture event types.
#[cfg(feature = "webrtc")]
fn webrtc_event(data: &dyn Any) -> Option<Event> {
    data.downcast_ref::<RTCPeerConnectionIceEvent>()
        .map(|ice_event| ice_event.event().clone())
        .or_else(|| {
            data.downcast_ref::<RTCDataChannelEvent>()
                .map(|channel_event| channel_event.event().clone())
        })
        .or_else(|| {
            data.downcast_ref::<RTCTrackEvent>()
                .map(|track_event| track_event.event().clone())
        })
        .or_else(|| {
            data.downcast_ref::<MediaStreamTrackEvent>()
                .map(|track_event| track_event.event().clone())
        })
}

#[cfg(not(feature = "webrtc"))]
fn webrtc_event(_data: &dyn Any) -> Option<Event> {
    None
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
        HTMLScriptElement,
        script,
        script.html_element.element.node.event_target
    );
    target!(
        HTMLLinkElement,
        link,
        link.html_element.element.node.event_target
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
    target!(Attr, attr, attr.event_target);
    if let Some(target_value) = data.downcast_mut::<EventTarget>() {
        return Some(f(target_value));
    }
    target!(MessagePort, port, port.event_target);
    target!(Worker, worker, worker.event_target);
    #[cfg(feature = "webrtc")]
    target!(RTCPeerConnection, connection, connection.event_target);
    #[cfg(feature = "webrtc")]
    target!(RTCDataChannel, channel, channel.event_target);
    target!(WebSocket, socket, socket.event_target);
    #[cfg(feature = "webrtc")]
    target!(MediaStreamTrack, track, track.event_target);
    #[cfg(feature = "webrtc")]
    target!(MediaStream, stream, stream.event_target);
    #[cfg(feature = "webrtc")]
    target!(MediaDevices, devices, devices.event_target);
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
        HTMLScriptElement,
        script,
        script.html_element.element.node.event_target
    );
    slot!(
        HTMLLinkElement,
        link,
        link.html_element.element.node.event_target
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
    slot!(Attr, attr, attr.event_target);
    if let Some(target_value) = data.downcast_mut::<EventTarget>() {
        return Some(f(&mut target_value.reflector));
    }
    if let Some(map) = data.downcast_mut::<NamedNodeMap>() {
        return Some(f(&mut map.reflector));
    }
    slot!(MessagePort, port, port.event_target);
    slot!(Worker, worker, worker.event_target);
    #[cfg(feature = "webrtc")]
    slot!(RTCPeerConnection, connection, connection.event_target);
    #[cfg(feature = "webrtc")]
    slot!(RTCDataChannel, channel, channel.event_target);
    slot!(WebSocket, socket, socket.event_target);
    #[cfg(feature = "webrtc")]
    slot!(MediaStreamTrack, track, track.event_target);
    #[cfg(feature = "webrtc")]
    slot!(MediaStream, stream, stream.event_target);
    #[cfg(feature = "webrtc")]
    slot!(MediaDevices, devices, devices.event_target);
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
    if let Some(params) = data.downcast_mut::<URLSearchParams>() {
        return Some(f(&mut params.reflector));
    }
    if let Some(headers) = data.downcast_mut::<Headers>() {
        return Some(f(&mut headers.reflector));
    }
    #[cfg(feature = "webrtc")]
    {
        if let Some(sender) = data.downcast_mut::<RTCRtpSender>() {
            return Some(f(&mut sender.reflector));
        }
        if let Some(receiver) = data.downcast_mut::<RTCRtpReceiver>() {
            return Some(f(&mut receiver.reflector));
        }
        if let Some(transceiver) = data.downcast_mut::<RTCRtpTransceiver>() {
            return Some(f(&mut transceiver.reflector));
        }
    }
    if let Some(event) = data.downcast_mut::<Event>() {
        return Some(f(&mut event.event_mut().reflector));
    }
    if let Some(close_event) = data.downcast_mut::<CloseEvent>() {
        return Some(f(&mut close_event.event_mut().reflector));
    }
    if let Some(rejection_event) = data.downcast_mut::<PromiseRejectionEvent>() {
        return Some(f(&mut rejection_event.event_mut().reflector));
    }
    #[cfg(feature = "webrtc")]
    {
        if let Some(ice_event) = data.downcast_mut::<RTCPeerConnectionIceEvent>() {
            return Some(f(&mut ice_event.event_mut().reflector));
        }
        if let Some(channel_event) = data.downcast_mut::<RTCDataChannelEvent>() {
            return Some(f(&mut channel_event.event_mut().reflector));
        }
        if let Some(track_event) = data.downcast_mut::<RTCTrackEvent>() {
            return Some(f(&mut track_event.event_mut().reflector));
        }
        if let Some(track_event) = data.downcast_mut::<MediaStreamTrackEvent>() {
            return Some(f(&mut track_event.event_mut().reflector));
        }
        // RTCSessionDescription and RTCIceCandidate keep no reflector: their
        // getters return the objects the connection stores.
        if data.downcast_ref::<RTCSessionDescription>().is_some()
            || data.downcast_ref::<RTCIceCandidate>().is_some()
        {
            return None;
        }
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
        #[cfg(feature = "webrtc")]
        let peer = ec.with_object_any(&obj).and_then(|data| {
            data.downcast_ref::<RTCPeerConnection>()
                .map(|connection| connection.id)
        });
        let (worker_id, socket) = ec
            .with_object_any_mut(&obj)
            .map(|data| {
                let worker_id = data.downcast_ref::<Worker>().map(|worker| worker.worker_id);
                let socket = data.downcast_ref::<WebSocket>().map(|socket| socket.id);
                with_platform_reflector_slot_mut(data, |slot| *slot = reflector.clone());
                (worker_id, socket)
            })
            .unwrap_or((None, None));

        if let Some(socket) = socket {
            // The realm's registry holds a clone of the socket made by its
            // constructor; mirror the reflector onto it, as for workers.
            let reflector = reflector.clone();
            if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
                if let Some(reflector) = reflector {
                    global_scope.sync_web_socket_reflector(socket, reflector, ec);
                }
                Ok(())
            }) {
                error!(
                    "failed to sync the reflector of a WebSocket: {}",
                    error.display()
                );
            }
        }

        #[cfg(feature = "webrtc")]
        if let Some(peer) = peer {
            // The realm's registry holds a clone of the connection made by
            // its constructor; mirror the reflector onto it, as for workers.
            let reflector = reflector.clone();
            if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
                if let Some(reflector) = reflector {
                    global_scope.sync_peer_connection_reflector(peer, reflector, ec);
                }
                Ok(())
            }) {
                error!(
                    "failed to sync the reflector of an RTCPeerConnection: {}",
                    error.display()
                );
            }
        }

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
        } else if let Some(script) = data.downcast_ref::<HTMLScriptElement>() {
            Some(script.html_element.element.node.event_target.clone())
        } else if let Some(link) = data.downcast_ref::<HTMLLinkElement>() {
            Some(link.html_element.element.node.event_target.clone())
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
        } else if let Some(attr) = data.downcast_ref::<Attr>() {
            Some(attr.event_target.clone())
        } else if let Some(port) = data.downcast_ref::<MessagePort>() {
            Some(port.event_target.clone())
        } else if let Some(socket) = data.downcast_ref::<WebSocket>() {
            Some(socket.event_target.clone())
        } else if let Some(target) = webrtc_event_target(data) {
            Some(target)
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

/// The `EventTarget` of the WebRTC and Media Capture platform objects.
#[cfg(feature = "webrtc")]
fn webrtc_event_target(data: &dyn Any) -> Option<EventTarget> {
    if let Some(connection) = data.downcast_ref::<RTCPeerConnection>() {
        Some(connection.event_target.clone())
    } else if let Some(channel) = data.downcast_ref::<RTCDataChannel>() {
        Some(channel.event_target.clone())
    } else if let Some(track) = data.downcast_ref::<MediaStreamTrack>() {
        Some(track.event_target.clone())
    } else if let Some(stream) = data.downcast_ref::<MediaStream>() {
        Some(stream.event_target.clone())
    } else {
        data.downcast_ref::<MediaDevices>()
            .map(|devices| devices.event_target.clone())
    }
}

#[cfg(not(feature = "webrtc"))]
fn webrtc_event_target(_data: &dyn Any) -> Option<EventTarget> {
    None
}
