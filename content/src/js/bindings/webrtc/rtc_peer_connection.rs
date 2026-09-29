use crate::js::Types;
use crate::js::bindings::mediacapture_streams::{stream_from_value, track_from_value};
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webidl::{any_value, convert_js_to_sequence};
use crate::webrtc::rtc_ice_candidate::RTCIceCandidateInit;
use crate::webrtc::rtc_peer_connection::{
    RTCConfiguration, RTCIceServer, RTCLocalSessionDescriptionInit,
};
use crate::webrtc::rtc_rtp_transceiver::direction_from_idl;
use crate::webrtc::rtc_session_description::{RTCSdpType, RTCSessionDescriptionInit};
use crate::webrtc::{RTCDataChannelInit, RTCPeerConnection, RTCRtpTransceiverInit, TrackOrKind};
use ipc_messages::webrtc::{TrackKind, TransceiverDirection};

use super::rtc_rtp_transceiver::{reflectors_array, sender_from_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{
    boolean_member, dictionary, event_handlers, member, nullable_string_member, string_member,
    this_as,
};

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

fn connection(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCPeerConnection, Types> {
    this_as::<RTCPeerConnection>(this, "RTCPeerConnection", ec)
}

impl WebIdlInterface<Types> for RTCPeerConnection {
    const NAME: &'static str = "RTCPeerConnection";

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
        let configuration = convert_configuration(args.first(), ec)?;
        RTCPeerConnection::constructor(configuration, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, operation "createOffer", 0, create_offer, promise true);
        member!(def, operation "createAnswer", 0, create_answer, promise true);
        member!(def, operation "setLocalDescription", 0, set_local_description, promise true);
        member!(def, operation "setRemoteDescription", 1, set_remote_description, promise true);
        member!(def, operation "addIceCandidate", 0, add_ice_candidate, promise true);
        member!(def, operation "createDataChannel", 1, create_data_channel, promise false);
        member!(def, operation "addTrack", 1, add_track, promise false);
        member!(def, operation "addTransceiver", 1, add_transceiver, promise false);
        member!(def, operation "removeTrack", 1, remove_track, promise false);
        member!(def, operation "getSenders", 0, get_senders, promise false);
        member!(def, operation "getReceivers", 0, get_receivers, promise false);
        member!(def, operation "getTransceivers", 0, get_transceivers, promise false);
        member!(def, operation "getStats", 0, get_stats, promise true);
        member!(def, operation "close", 0, close, promise false);
        member!(def, attribute "localDescription", local_description);
        member!(def, attribute "currentLocalDescription", current_local_description);
        member!(def, attribute "pendingLocalDescription", pending_local_description);
        member!(def, attribute "remoteDescription", remote_description);
        member!(def, attribute "currentRemoteDescription", current_remote_description);
        member!(def, attribute "pendingRemoteDescription", pending_remote_description);
        member!(def, attribute "signalingState", signaling_state);
        member!(def, attribute "iceGatheringState", ice_gathering_state);
        member!(def, attribute "iceConnectionState", ice_connection_state);
        member!(def, attribute "connectionState", connection_state);
        member!(def, attribute "canTrickleIceCandidates", can_trickle_ice_candidates);
        member!(def, attribute "onnegotiationneeded", get_onnegotiationneeded, set_onnegotiationneeded);
        member!(def, attribute "onicecandidate", get_onicecandidate, set_onicecandidate);
        member!(def, attribute "onicecandidateerror", get_onicecandidateerror, set_onicecandidateerror);
        member!(def, attribute "onsignalingstatechange", get_onsignalingstatechange, set_onsignalingstatechange);
        member!(def, attribute "oniceconnectionstatechange", get_oniceconnectionstatechange, set_oniceconnectionstatechange);
        member!(def, attribute "onicegatheringstatechange", get_onicegatheringstatechange, set_onicegatheringstatechange);
        member!(def, attribute "onconnectionstatechange", get_onconnectionstatechange, set_onconnectionstatechange);
        member!(def, attribute "ondatachannel", get_ondatachannel, set_ondatachannel);
        member!(def, attribute "ontrack", get_ontrack, set_ontrack);
    }
}

fn convert_configuration(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCConfiguration, Types> {
    let dict = dictionary(value, ec)?;
    let mut configuration = RTCConfiguration::default();
    if let Some(policy) = string_member(&dict, "bundlePolicy", ec)? {
        configuration.bundle_policy =
            enumeration(&policy, &["balanced", "max-compat", "max-bundle"], ec)?;
    }
    if let Some(certificates) = dict.get_member("certificates", ec)? {
        // sequence<RTCCertificate>: RTCCertificate is not implemented, so
        // no element converts (<https://webidl.spec.whatwg.org/#js-interface>).
        if !convert_js_to_sequence(&certificates, any_value, ec)?.is_empty() {
            return Err(ec.new_type_error("value is not an RTCCertificate"));
        }
    }
    if let Some(size) = dict.get_member("iceCandidatePoolSize", ec)? {
        // [EnforceRange] octet.
        let size = crate::webidl::enforce_range_unsigned_short(&size, ec)?;
        configuration.ice_candidate_pool_size = u8::try_from(size)
            .map_err(|_| ec.new_type_error("iceCandidatePoolSize is out of range"))?;
    }
    if let Some(servers) = dict.get_member("iceServers", ec)? {
        for server in convert_js_to_sequence(&servers, any_value, ec)? {
            configuration
                .ice_servers
                .push(convert_ice_server(&server, ec)?);
        }
    }
    if let Some(policy) = string_member(&dict, "iceTransportPolicy", ec)? {
        configuration.ice_transport_policy = enumeration(&policy, &["relay", "all"], ec)?;
    }
    if let Some(policy) = string_member(&dict, "rtcpMuxPolicy", ec)? {
        configuration.rtcp_mux_policy = enumeration(&policy, &["require"], ec)?;
    }
    Ok(configuration)
}

fn convert_ice_server(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCIceServer, Types> {
    let dict = dictionary(Some(value), ec)?;
    let credential = string_member(&dict, "credential", ec)?;
    // urls: (DOMString or sequence<DOMString>), required.
    let Some(urls) = dict.get_member("urls", ec)? else {
        return Err(ec.new_type_error("RTCIceServer.urls is required"));
    };
    let urls = match Types::value_as_object(&urls) {
        Some(_) => convert_js_to_sequence(&urls, any_value, ec)?
            .into_iter()
            .map(|url| ec.to_rust_string(url))
            .collect::<Completion<Vec<_>, Types>>()?,
        None => vec![ec.to_rust_string(urls)?],
    };
    let username = string_member(&dict, "username", ec)?;
    Ok(RTCIceServer {
        urls,
        username,
        credential,
    })
}

fn enumeration(
    value: &str,
    allowed: &[&str],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    if allowed.contains(&value) {
        Ok(value.to_owned())
    } else {
        Err(ec.new_type_error(&format!("'{value}' is not a valid enumeration value")))
    }
}

pub(super) fn convert_session_description_init(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCSessionDescriptionInit, Types> {
    let dict = dictionary(value, ec)?;
    let sdp = string_member(&dict, "sdp", ec)?.unwrap_or_default();
    let Some(type_) = string_member(&dict, "type", ec)? else {
        return Err(ec.new_type_error("RTCSessionDescriptionInit.type is required"));
    };
    let type_ = RTCSdpType::from_idl(&type_)
        .ok_or_else(|| ec.new_type_error(&format!("'{type_}' is not a valid RTCSdpType")))?;
    Ok(RTCSessionDescriptionInit { type_, sdp })
}

fn convert_local_session_description_init(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCLocalSessionDescriptionInit, Types> {
    let dict = dictionary(value, ec)?;
    let sdp = string_member(&dict, "sdp", ec)?.unwrap_or_default();
    let type_ =
        match string_member(&dict, "type", ec)? {
            Some(type_) => Some(RTCSdpType::from_idl(&type_).ok_or_else(|| {
                ec.new_type_error(&format!("'{type_}' is not a valid RTCSdpType"))
            })?),
            None => None,
        };
    Ok(RTCLocalSessionDescriptionInit { type_, sdp })
}

pub(super) fn convert_ice_candidate_init(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCIceCandidateInit, Types> {
    let dict = dictionary(value, ec)?;
    let candidate = string_member(&dict, "candidate", ec)?.unwrap_or_default();
    let sdp_m_line_index = match dict.get_member("sdpMLineIndex", ec)? {
        Some(value) if !Types::value_is_null(&value) => {
            Some(crate::webidl::unsigned_short(&value, ec)?)
        }
        _ => None,
    };
    let relay_protocol = match string_member(&dict, "relayProtocol", ec)? {
        Some(protocol) => Some(enumeration(&protocol, &["udp", "tcp", "tls"], ec)?),
        None => None,
    };
    let sdp_mid = nullable_string_member(&dict, "sdpMid", ec)?;
    let url = string_member(&dict, "url", ec)?;
    let username_fragment = nullable_string_member(&dict, "usernameFragment", ec)?;
    Ok(RTCIceCandidateInit {
        candidate,
        sdp_mid,
        sdp_m_line_index,
        username_fragment,
        relay_protocol,
        url,
    })
}

fn convert_data_channel_init(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCDataChannelInit, Types> {
    let dict = dictionary(value, ec)?;
    let enforce =
        |key: &str, ec: &mut dyn ExecutionContext<Types>| -> Completion<Option<u16>, Types> {
            match dict.get_member(key, ec)? {
                Some(value) => Ok(Some(crate::webidl::enforce_range_unsigned_short(
                    &value, ec,
                )?)),
                None => Ok(None),
            }
        };
    let id = enforce("id", ec)?;
    let max_packet_life_time = enforce("maxPacketLifeTime", ec)?;
    let max_retransmits = enforce("maxRetransmits", ec)?;
    let negotiated = boolean_member(&dict, "negotiated", false, ec)?;
    let ordered = boolean_member(&dict, "ordered", true, ec)?;
    let protocol = string_member(&dict, "protocol", ec)?.unwrap_or_default();
    Ok(RTCDataChannelInit {
        ordered,
        max_packet_life_time,
        max_retransmits,
        protocol,
        negotiated,
        id,
    })
}

fn promise_value(promise: JsObject) -> JsValue {
    Types::value_from_object(promise)
}

fn create_offer(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Note: RTCOfferOptions has no member this implementation acts on; the
    // legacy callback overload is not implemented.
    let connection = connection(this, ec)?;
    connection.create_offer(ec).map(promise_value)
}

fn create_answer(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    connection.create_answer(ec).map(promise_value)
}

fn set_local_description(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let description = convert_local_session_description_init(args.first(), ec)?;
    connection
        .set_local_description(description, ec)
        .map(promise_value)
}

fn set_remote_description(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let description = convert_session_description_init(args.first(), ec)?;
    connection
        .set_remote_description(description, ec)
        .map(promise_value)
}

fn add_ice_candidate(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    // (RTCIceCandidateInit or RTCIceCandidate): an RTCIceCandidate converts
    // through its attributes, as a dictionary would read them.
    let candidate = match args
        .first()
        .and_then(Types::value_as_object)
        .and_then(|object| {
            ec.with_object_any(&object).and_then(|data| {
                data.downcast_ref::<crate::webrtc::RTCIceCandidate>()
                    .cloned()
            })
        }) {
        Some(candidate) => candidate.init.clone(),
        None => convert_ice_candidate_init(args.first(), ec)?,
    };
    connection
        .add_ice_candidate(candidate, ec)
        .map(promise_value)
}

fn create_data_channel(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let undefined = ec.value_undefined();
    // label: USVString.
    let label = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let options = convert_data_channel_init(args.get(1), ec)?;
    let channel = connection.create_data_channel(label, options, ec)?;
    let object = channel
        .object()
        .ok_or_else(|| ec.new_type_error("RTCDataChannel without its object"))?;
    Ok(Types::value_from_object(object))
}

fn close(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    connection.close(ec);
    Ok(ec.value_undefined())
}

fn nullable_object(object: Option<JsObject>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match object {
        Some(object) => Types::value_from_object(object),
        None => ec.value_null(),
    }
}

fn string(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

macro_rules! description_getter {
    ($($name:ident),*) => {
        $(
            fn $name(this: &JsValue, _args: &[JsValue], ec: &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types> {
                let connection = connection(this, ec)?;
                let object = connection.$name(ec);
                Ok(nullable_object(object, ec))
            }
        )*
    };
}

description_getter!(
    local_description,
    current_local_description,
    pending_local_description,
    remote_description,
    current_remote_description,
    pending_remote_description
);

fn signaling_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    Ok(string(connection.signaling_state(), ec))
}

fn ice_gathering_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.ice_gathering_state();
    Ok(string(&state, ec))
}

fn ice_connection_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.ice_connection_state();
    Ok(string(&state, ec))
}

fn connection_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.connection_state();
    Ok(string(&state, ec))
}

fn can_trickle_ice_candidates(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Note: Whether the remote peer can trickle is not read from its
    // description; the attribute is null, its value before a remote
    // description is set.
    connection(this, ec)?;
    Ok(ec.value_null())
}

event_handlers!(
    get_onnegotiationneeded, set_onnegotiationneeded, "negotiationneeded";
    get_onicecandidate, set_onicecandidate, "icecandidate";
    get_onicecandidateerror, set_onicecandidateerror, "icecandidateerror";
    get_onsignalingstatechange, set_onsignalingstatechange, "signalingstatechange";
    get_oniceconnectionstatechange, set_oniceconnectionstatechange, "iceconnectionstatechange";
    get_onicegatheringstatechange, set_onicegatheringstatechange, "icegatheringstatechange";
    get_onconnectionstatechange, set_onconnectionstatechange, "connectionstatechange";
    get_ondatachannel, set_ondatachannel, "datachannel";
    get_ontrack, set_ontrack, "track";
);

fn add_track(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let undefined = ec.value_undefined();
    let track = track_from_value(args.first().unwrap_or(&undefined), ec)
        .ok_or_else(|| ec.new_type_error("addTrack needs a MediaStreamTrack"))?;
    let mut streams = Vec::new();
    for value in args.iter().skip(1) {
        let stream = stream_from_value(value, ec)
            .ok_or_else(|| ec.new_type_error("addTrack streams must be MediaStream objects"))?;
        streams.push(stream);
    }
    let sender = connection.add_track(track, streams, ec)?;
    sender
        .reflector
        .clone()
        .map(Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("RTCRtpSender without its object"))
}

fn add_transceiver(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let undefined = ec.value_undefined();
    let first = args.first().unwrap_or(&undefined);
    let track_or_kind = match track_from_value(first, ec) {
        Some(track) => TrackOrKind::Track(track),
        None => {
            let kind = ec.to_rust_string(first.clone())?;
            match kind.as_str() {
                "audio" => TrackOrKind::Kind(TrackKind::Audio),
                "video" => TrackOrKind::Kind(TrackKind::Video),
                _ => {
                    return Err(ec.new_type_error(&format!(
                        "'{kind}' is not a legal MediaStreamTrack kind"
                    )));
                }
            }
        }
    };
    let init = dictionary(args.get(1), ec)?;
    let direction = match string_member(&init, "direction", ec)? {
        Some(value) => direction_from_idl(&value).ok_or_else(|| {
            ec.new_type_error(&format!(
                "'{value}' is not a valid value for enumeration RTCRtpTransceiverDirection"
            ))
        })?,
        None => TransceiverDirection::Sendrecv,
    };
    let streams = match init.get_member("streams", ec)? {
        Some(value) => convert_js_to_sequence(
            &value,
            |item, ec| {
                stream_from_value(&item, ec)
                    .ok_or_else(|| ec.new_type_error("streams must hold MediaStream objects"))
            },
            ec,
        )?,
        None => Vec::new(),
    };
    let transceiver = connection.add_transceiver(
        track_or_kind,
        RTCRtpTransceiverInit { direction, streams },
        ec,
    )?;
    transceiver
        .reflector
        .clone()
        .map(Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("RTCRtpTransceiver without its object"))
}

fn remove_track(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let undefined = ec.value_undefined();
    let sender = sender_from_value(args.first().unwrap_or(&undefined), ec)
        .ok_or_else(|| ec.new_type_error("removeTrack needs an RTCRtpSender"))?;
    connection.remove_track(sender, ec)?;
    Ok(ec.value_undefined())
}

fn get_senders(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let senders = connection(this, ec)?.get_senders(ec);
    reflectors_array(
        senders
            .iter()
            .map(|sender| sender.reflector.clone())
            .collect(),
        ec,
    )
}

fn get_receivers(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let receivers = connection(this, ec)?.get_receivers(ec);
    reflectors_array(
        receivers
            .iter()
            .map(|receiver| receiver.reflector.clone())
            .collect(),
        ec,
    )
}

fn get_transceivers(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let transceivers = connection(this, ec)?.get_transceivers(ec);
    reflectors_array(
        transceivers
            .iter()
            .map(|transceiver| transceiver.reflector.clone())
            .collect(),
        ec,
    )
}

fn get_stats(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = connection(this, ec)?.get_stats(ec)?;
    Ok(Types::value_from_object(promise))
}
