use crate::js::Types;
use crate::mediacapture_streams::{MediaStream, MediaStreamInit};
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webidl::convert_js_to_sequence;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{
    event_handlers, member, stream_from_value, stream_value, string_value, this_as,
    track_from_value, track_value, tracks_array,
};

type JsValue = <Types as JsTypes>::JsValue;

fn stream(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<MediaStream, Types> {
    this_as::<MediaStream>(this, "MediaStream", ec)
}

fn track_argument(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<crate::mediacapture_streams::MediaStreamTrack, Types> {
    let undefined = ec.value_undefined();
    track_from_value(args.first().unwrap_or(&undefined), ec)
        .ok_or_else(|| ec.new_type_error("the argument is not a MediaStreamTrack"))
}

impl WebIdlInterface<Types> for MediaStream {
    const NAME: &'static str = "MediaStream";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let init = match args.first() {
            None => MediaStreamInit::Empty,
            Some(value) if Types::value_is_undefined(value) => MediaStreamInit::Empty,
            Some(value) => match stream_from_value(value, ec) {
                Some(stream) => MediaStreamInit::Stream(stream),
                None => MediaStreamInit::Tracks(convert_js_to_sequence(
                    value,
                    |item, ec| {
                        track_from_value(&item, ec).ok_or_else(|| {
                            ec.new_type_error("MediaStream tracks must be MediaStreamTrack objects")
                        })
                    },
                    ec,
                )?),
            },
        };
        MediaStream::constructor(init, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "id", id);
        member!(def, attribute "active", active);
        member!(def, operation "getAudioTracks", 0, get_audio_tracks, promise false);
        member!(def, operation "getVideoTracks", 0, get_video_tracks, promise false);
        member!(def, operation "getTracks", 0, get_tracks, promise false);
        member!(def, operation "getTrackById", 1, get_track_by_id, promise false);
        member!(def, operation "addTrack", 1, add_track, promise false);
        member!(def, operation "removeTrack", 1, remove_track, promise false);
        member!(def, operation "clone", 0, clone_method, promise false);
        member!(def, attribute "onaddtrack", get_onaddtrack, set_onaddtrack);
        member!(def, attribute "onremovetrack", get_onremovetrack, set_onremovetrack);
    }
}

event_handlers!(
    get_onaddtrack, set_onaddtrack, "addtrack";
    get_onremovetrack, set_onremovetrack, "removetrack";
);

fn id(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let id = stream(this, ec)?.id();
    Ok(string_value(&id, ec))
}

fn active(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let active = stream(this, ec)?.active(ec);
    Ok(ec.value_from_bool(active))
}

fn get_audio_tracks(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let tracks = stream(this, ec)?.get_audio_tracks(ec);
    tracks_array(&tracks, ec)
}

fn get_video_tracks(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let tracks = stream(this, ec)?.get_video_tracks(ec);
    tracks_array(&tracks, ec)
}

fn get_tracks(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let tracks = stream(this, ec)?.get_tracks(ec);
    tracks_array(&tracks, ec)
}

fn get_track_by_id(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream = stream(this, ec)?;
    let undefined = ec.value_undefined();
    let track_id = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    Ok(match stream.get_track_by_id(&track_id, ec) {
        Some(track) => track_value(&track, ec),
        None => ec.value_null(),
    })
}

fn add_track(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream = stream(this, ec)?;
    let track = track_argument(args, ec)?;
    stream.add_track(track, ec);
    Ok(ec.value_undefined())
}

fn remove_track(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let stream = stream(this, ec)?;
    let track = track_argument(args, ec)?;
    stream.remove_track(&track, ec);
    Ok(ec.value_undefined())
}

fn clone_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let clone = stream(this, ec)?.clone_stream(ec)?;
    Ok(stream_value(&clone, ec))
}
