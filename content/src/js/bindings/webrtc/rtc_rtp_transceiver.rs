use crate::js::Types;
use crate::js::bindings::mediacapture_streams::{track_from_value, track_value};
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webidl::resolved_promise;
use crate::webrtc::rtc_rtp_transceiver::{direction_as_idl, direction_from_idl, replace_track};
use crate::webrtc::{RTCRtpReceiver, RTCRtpSender, RTCRtpTransceiver};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{member, this_as};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

fn sender(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<RTCRtpSender, Types> {
    this_as::<RTCRtpSender>(this, "RTCRtpSender", ec)
}

fn receiver(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCRtpReceiver, Types> {
    this_as::<RTCRtpReceiver>(this, "RTCRtpReceiver", ec)
}

fn transceiver(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCRtpTransceiver, Types> {
    this_as::<RTCRtpTransceiver>(this, "RTCRtpTransceiver", ec)
}

pub(crate) fn sender_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<RTCRtpSender> {
    let object = Types::value_as_object(value)?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<RTCRtpSender>().cloned())
}

fn string(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

fn reflector_value(
    reflector: Option<JsObject>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    reflector
        .map(Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("the platform object has no reflector"))
}

/// The connection a sender or transceiver belongs to, cloned out of the
/// realm's registry.
fn connection_of(
    peer: ipc_messages::webrtc::PeerConnectionId,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<crate::webrtc::RTCPeerConnection>, Types> {
    with_global_scope(ec, |global_scope, ec| {
        Ok(global_scope.peer_connection(peer, ec))
    })
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpsendparameters>, the default
/// parameters of a sender or receiver: no encodings, codecs or header
/// extensions are negotiated by content.
fn default_parameters(ec: &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> {
    let object = ec.create_plain_object(None);
    for name in ["codecs", "headerExtensions", "encodings"] {
        let key = ec.property_key_from_str(name);
        let array = ec.create_empty_array();
        ec.create_data_property(object.clone(), key, Types::value_from_object(array))?;
    }
    let rtcp = ec.create_plain_object(None);
    let key = ec.property_key_from_str("rtcp");
    ec.create_data_property(object.clone(), key, Types::value_from_object(rtcp))?;
    Ok(Types::value_from_object(object))
}

impl WebIdlInterface<Types> for RTCRtpSender {
    const NAME: &'static str = "RTCRtpSender";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "track", sender_track);
        member!(def, attribute "transport", null_attribute);
        member!(def, attribute "dtmf", null_attribute);
        member!(def, operation "replaceTrack", 1, sender_replace_track, promise true);
        member!(def, operation "getParameters", 0, get_parameters, promise false);
        member!(def, operation "setParameters", 1, set_parameters, promise true);
        member!(def, operation "setStreams", 0, set_streams, promise false);
        member!(def, operation "getStats", 0, sender_get_stats, promise true);
    }
}

fn null_attribute(
    _this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(ec.value_null())
}

fn sender_track(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sender = sender(this, ec)?;
    Ok(match sender.track(ec) {
        Some(track) => track_value(&track, ec),
        None => ec.value_null(),
    })
}

fn sender_replace_track(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sender = sender(this, ec)?;
    let undefined = ec.value_undefined();
    let value = args.first().unwrap_or(&undefined);
    let with_track =
        if Types::value_is_null(value) || Types::value_is_undefined(value) {
            None
        } else {
            Some(track_from_value(value, ec).ok_or_else(|| {
                ec.new_type_error("replaceTrack needs a MediaStreamTrack or null")
            })?)
        };
    let peer = sender.slots.borrow().peer;
    let transceiver_id = sender.transceiver_id();
    let transceiver = connection_of(peer, ec)?.and_then(|connection| {
        connection
            .get_transceivers(ec)
            .into_iter()
            .find(|transceiver| transceiver.id() == transceiver_id)
    });
    let promise = replace_track(&sender, transceiver.as_ref(), with_track, ec)?;
    Ok(Types::value_from_object(promise))
}

fn get_parameters(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if sender_from_value(this, ec).is_none() {
        receiver(this, ec)?;
    }
    default_parameters(ec)
}

fn set_parameters(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    sender(this, ec)?;
    let undefined = ec.value_undefined();
    let promise = resolved_promise(undefined, ec)?;
    Ok(Types::value_from_object(promise))
}

fn set_streams(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sender = sender(this, ec)?;
    let mut ids = Vec::new();
    for value in args {
        let stream = crate::js::bindings::mediacapture_streams::stream_from_value(value, ec)
            .ok_or_else(|| ec.new_type_error("setStreams needs MediaStream objects"))?;
        let id = stream.id();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    sender.set_streams(ids);
    Ok(ec.value_undefined())
}

fn sender_get_stats(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sender = sender(this, ec)?;
    let peer = sender.slots.borrow().peer;
    let Some(connection) = connection_of(peer, ec)? else {
        return Err(ec.new_type_error("the sender's connection is gone"));
    };
    let promise = connection.get_stats(ec)?;
    Ok(Types::value_from_object(promise))
}

impl WebIdlInterface<Types> for RTCRtpReceiver {
    const NAME: &'static str = "RTCRtpReceiver";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "track", receiver_track);
        member!(def, attribute "transport", null_attribute);
        member!(def, operation "getParameters", 0, get_parameters, promise false);
        member!(def, operation "getContributingSources", 0, empty_array, promise false);
        member!(def, operation "getSynchronizationSources", 0, empty_array, promise false);
        member!(def, operation "getStats", 0, receiver_get_stats, promise true);
    }
}

fn receiver_track(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let track = receiver(this, ec)?.track();
    Ok(track_value(&track, ec))
}

fn empty_array(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    receiver(this, ec)?;
    let array = ec.create_empty_array();
    Ok(Types::value_from_object(array))
}

fn receiver_get_stats(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let receiver = receiver(this, ec)?;
    let peer = match receiver.track().source() {
        crate::mediacapture_streams::TrackSource::Remote { peer, .. } => peer,
        crate::mediacapture_streams::TrackSource::Capture { .. } => {
            return Err(ec.new_type_error("the receiver's track is not remote"));
        }
    };
    let Some(connection) = connection_of(peer, ec)? else {
        return Err(ec.new_type_error("the receiver's connection is gone"));
    };
    let promise = connection.get_stats(ec)?;
    Ok(Types::value_from_object(promise))
}

impl WebIdlInterface<Types> for RTCRtpTransceiver {
    const NAME: &'static str = "RTCRtpTransceiver";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "mid", mid);
        member!(def, attribute "sender", transceiver_sender);
        member!(def, attribute "receiver", transceiver_receiver);
        member!(def, attribute "direction", direction, set_direction);
        member!(def, attribute "currentDirection", current_direction);
        member!(def, operation "stop", 0, stop, promise false);
        member!(def, operation "setCodecPreferences", 1, set_codec_preferences, promise false);
    }
}

fn mid(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(match transceiver(this, ec)?.mid() {
        Some(mid) => string(&mid, ec),
        None => ec.value_null(),
    })
}

fn transceiver_sender(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reflector = transceiver(this, ec)?.sender.reflector.clone();
    reflector_value(reflector, ec)
}

fn transceiver_receiver(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reflector = transceiver(this, ec)?.receiver.reflector.clone();
    reflector_value(reflector, ec)
}

fn direction(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let direction = transceiver(this, ec)?.direction();
    Ok(string(direction_as_idl(direction), ec))
}

fn peer_of(transceiver: &RTCRtpTransceiver) -> ipc_messages::webrtc::PeerConnectionId {
    transceiver.sender.slots.borrow().peer
}

fn set_direction(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let transceiver = transceiver(this, ec)?;
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let Some(new_direction) = direction_from_idl(&value) else {
        return Err(ec.new_type_error(&format!(
            "'{value}' is not a valid value for enumeration RTCRtpTransceiverDirection"
        )));
    };
    let Some(connection) = connection_of(peer_of(&transceiver), ec)? else {
        return Err(ec.new_type_error("the transceiver's connection is gone"));
    };
    connection.set_transceiver_direction(&transceiver, new_direction, ec)?;
    Ok(ec.value_undefined())
}

fn current_direction(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(match transceiver(this, ec)?.current_direction() {
        Some(direction) => string(direction_as_idl(direction), ec),
        None => ec.value_null(),
    })
}

fn stop(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let transceiver = transceiver(this, ec)?;
    let Some(connection) = connection_of(peer_of(&transceiver), ec)? else {
        return Err(ec.new_type_error("the transceiver's connection is gone"));
    };
    connection.stop_transceiver(&transceiver, 0.0, ec)?;
    Ok(ec.value_undefined())
}

fn set_codec_preferences(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    transceiver(this, ec)?;
    Ok(ec.value_undefined())
}

/// A JavaScript array of the objects' reflectors.
pub(crate) fn reflectors_array(
    reflectors: Vec<Option<JsObject>>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let array = ec.create_empty_array();
    for reflector in reflectors {
        let value = reflector_value(reflector, ec)?;
        ec.array_push(&array, value)?;
    }
    Ok(Types::value_from_object(array))
}
