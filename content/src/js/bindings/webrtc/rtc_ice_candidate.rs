use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webrtc::RTCIceCandidate;
use crate::webrtc::rtc_ice_candidate::{nullable_string_value, string_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::rtc_peer_connection::convert_ice_candidate_init;
use super::{member, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn candidate(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCIceCandidate, Types> {
    this_as::<RTCIceCandidate>(this, "RTCIceCandidate", ec)
}

impl WebIdlInterface<Types> for RTCIceCandidate {
    const NAME: &'static str = "RTCIceCandidate";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let candidate_init_dict = convert_ice_candidate_init(args.first(), ec)?;
        RTCIceCandidate::constructor(candidate_init_dict, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "candidate", get_candidate);
        member!(def, attribute "sdpMid", sdp_mid);
        member!(def, attribute "sdpMLineIndex", sdp_m_line_index);
        member!(def, attribute "foundation", foundation);
        member!(def, attribute "component", component);
        member!(def, attribute "priority", priority);
        member!(def, attribute "address", address);
        member!(def, attribute "protocol", protocol);
        member!(def, attribute "port", port);
        member!(def, attribute "type", type_);
        member!(def, attribute "tcpType", tcp_type);
        member!(def, attribute "relatedAddress", related_address);
        member!(def, attribute "relatedPort", related_port);
        member!(def, attribute "usernameFragment", username_fragment);
        member!(def, attribute "relayProtocol", relay_protocol);
        member!(def, attribute "url", url);
        member!(def, operation "toJSON", 0, to_json, promise false);
    }
}

fn nullable_number(value: Option<f64>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match value {
        Some(value) => ec.value_from_number(value),
        None => ec.value_null(),
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-candidate>
fn get_candidate(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(string_value(&candidate.init.candidate, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-sdpmid>
fn sdp_mid(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.init.sdp_mid.as_deref(), ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-sdpmlineindex>
fn sdp_m_line_index(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_number(
        candidate.init.sdp_m_line_index.map(f64::from),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-foundation>
fn foundation(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(
        candidate.parsed.foundation.as_deref(),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-component>
fn component(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.parsed.component, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-priority>
fn priority(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_number(
        candidate.parsed.priority.map(f64::from),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-address>
fn address(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(
        candidate.parsed.address.as_deref(),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-protocol>
fn protocol(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.parsed.protocol, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-port>
fn port(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_number(candidate.parsed.port.map(f64::from), ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-type>
fn type_(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.parsed.type_, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-tcptype>
fn tcp_type(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.parsed.tcp_type, ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-relatedaddress>
fn related_address(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(
        candidate.parsed.related_address.as_deref(),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-relatedport>
fn related_port(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_number(
        candidate.parsed.related_port.map(f64::from),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-usernamefragment>
fn username_fragment(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(
        candidate.init.username_fragment.as_deref(),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-relayprotocol>
fn relay_protocol(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(
        candidate.init.relay_protocol.as_deref(),
        ec,
    ))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-url>
fn url(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    Ok(nullable_string_value(candidate.init.url.as_deref(), ec))
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-tojson>
fn to_json(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let candidate = candidate(this, ec)?;
    candidate.to_json(ec).map(Types::value_from_object)
}
