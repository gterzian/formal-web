//! Bindings for Media Capture and Streams
//! (<https://w3c.github.io/mediacapture-main/>): argument conversion to the
//! IDL types of `crate::mediacapture_streams`, then the domain call.

mod events;
mod media_devices;
mod media_stream;
mod media_stream_track;

use crate::js::Types;
use crate::mediacapture_streams::{MediaStream, MediaStreamTrack};
use js_engine::{ExecutionContext, JsTypes};

pub(super) use super::{event_handlers, string_value, this_as};

type JsValue = <Types as JsTypes>::JsValue;

macro_rules! member {
    ($def:expr, attribute $id:literal, $getter:expr) => {
        $def.add_attribute(crate::webidl::bindings::AttributeDef {
            id: $id,
            getter: $getter,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        })
    };
    ($def:expr, attribute $id:literal, $getter:expr, $setter:expr) => {
        $def.add_attribute(crate::webidl::bindings::AttributeDef {
            id: $id,
            getter: $getter,
            setter: Some($setter),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        })
    };
    ($def:expr, operation $id:literal, $length:expr, $method:expr, promise $promise:expr) => {
        $def.add_operation(crate::webidl::bindings::OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: false,
            unforgeable: false,
            promise_type: $promise,
            exposed: None,
        })
    };
}
pub(super) use member;

pub(crate) fn track_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<MediaStreamTrack> {
    let object = Types::value_as_object(value)?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<MediaStreamTrack>().cloned())
}

pub(crate) fn stream_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<MediaStream> {
    let object = Types::value_as_object(value)?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<MediaStream>().cloned())
}

/// The platform object of a track, as a value.
pub(crate) fn track_value(
    track: &MediaStreamTrack,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsValue {
    match track.object() {
        Some(object) => Types::value_from_object(object),
        None => ec.value_null(),
    }
}

/// The platform object of a stream, as a value.
pub(crate) fn stream_value(stream: &MediaStream, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match stream.object() {
        Some(object) => Types::value_from_object(object),
        None => ec.value_null(),
    }
}

/// A JavaScript array of the tracks' platform objects.
pub(crate) fn tracks_array(
    tracks: &[MediaStreamTrack],
    ec: &mut dyn ExecutionContext<Types>,
) -> js_engine::Completion<JsValue, Types> {
    let array = ec.create_empty_array();
    for track in tracks {
        let value = track_value(track, ec);
        ec.array_push(&array, value)?;
    }
    Ok(Types::value_from_object(array))
}
