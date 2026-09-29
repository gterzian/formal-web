use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webrtc::rtc_ice_candidate::RTCIceCandidateInit;
use crate::webrtc::rtc_peer_connection::{
    RTCConfiguration, RTCIceServer, RTCLocalSessionDescriptionInit,
};
use crate::webrtc::rtc_session_description::{RTCSdpType, RTCSessionDescriptionInit};
use crate::webrtc::{RTCDataChannelInit, RTCPeerConnection};
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
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration>
/// (dictionary members in lexicographical order).
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
        if !sequence(&certificates, ec)?.is_empty() {
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
        for server in sequence(&servers, ec)? {
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver>
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
        Some(_) => sequence(&urls, ec)?
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

/// <https://webidl.spec.whatwg.org/#js-enumeration>
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

/// <https://webidl.spec.whatwg.org/#js-sequence>
pub(super) fn sequence(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<JsValue>, Types> {
    if Types::value_as_object(value).is_none() {
        return Err(ec.new_type_error("value is not a sequence"));
    }
    let mut iterator = ec.get_iterator(value.clone(), js_engine::IteratorKind::Sync, None)?;
    let mut items = Vec::new();
    while let Some(item) = ec.iterator_step_value(&mut iterator)? {
        items.push(item);
    }
    Ok(items)
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescriptioninit>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtclocalsessiondescriptioninit>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-createoffer>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-createanswer>
fn create_answer(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    connection.create_answer(ec).map(promise_value)
}

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setlocaldescription>
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

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setremotedescription>
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

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-addicecandidate>
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

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-createdatachannel>
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

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-close>
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

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-signaling-state>
fn signaling_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    Ok(string(connection.signaling_state(), ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-ice-gathering-state>
fn ice_gathering_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.ice_gathering_state();
    Ok(string(&state, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-ice-connection-state>
fn ice_connection_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.ice_connection_state();
    Ok(string(&state, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-connection-state>
fn connection_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let connection = connection(this, ec)?;
    let state = connection.connection_state();
    Ok(string(&state, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-cantrickleicecandidates>
fn can_trickle_ice_candidates(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Note: Whether the remote peer can trickle is not read from its
    // description; the attribute is null, its value before a remote
    // description is set.
    let _ = connection(this, ec)?;
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
);
