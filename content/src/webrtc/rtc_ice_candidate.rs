use ipc_messages::webrtc::IceCandidate;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit>
#[derive(Debug, Clone, Default)]
pub(crate) struct RTCIceCandidateInit {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-candidate>
    pub(crate) candidate: String,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-sdpmid>
    pub(crate) sdp_mid: Option<String>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-sdpmlineindex>
    pub(crate) sdp_m_line_index: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-usernamefragment>
    pub(crate) username_fragment: Option<String>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-relayprotocol>
    pub(crate) relay_protocol: Option<String>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit-url>
    pub(crate) url: Option<String>,
}

impl RTCIceCandidateInit {
    pub(crate) fn to_ipc(&self) -> IceCandidate {
        IceCandidate {
            candidate: self.candidate.clone(),
            sdp_mid: self.sdp_mid.clone(),
            sdp_m_line_index: self.sdp_m_line_index,
            username_fragment: self.username_fragment.clone(),
        }
    }

    pub(crate) fn from_ipc(candidate: IceCandidate) -> Self {
        Self {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_m_line_index: candidate.sdp_m_line_index,
            username_fragment: candidate.username_fragment,
            relay_protocol: None,
            url: None,
        }
    }
}

/// The fields of a candidate-attribute
/// (<https://www.rfc-editor.org/rfc/rfc8839#section-5.1>) that
/// RTCIceCandidate exposes.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedCandidate {
    pub(crate) foundation: Option<String>,
    pub(crate) component: Option<&'static str>,
    pub(crate) priority: Option<u32>,
    pub(crate) address: Option<String>,
    pub(crate) protocol: Option<&'static str>,
    pub(crate) port: Option<u16>,
    pub(crate) type_: Option<&'static str>,
    pub(crate) tcp_type: Option<&'static str>,
    pub(crate) related_address: Option<String>,
    pub(crate) related_port: Option<u16>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate>
#[gc_struct]
pub(crate) struct RTCIceCandidate {
    /// The attributes initialized from the candidateInitDict.
    #[ignore_trace]
    pub(crate) init: RTCIceCandidateInit,

    /// The attributes parsed from the candidate-attribute.
    #[ignore_trace]
    pub(crate) parsed: ParsedCandidate,
}

impl RTCIceCandidate {
    /// <https://w3c.github.io/webrtc-pc/#dfn-rtcicecandidate>
    pub(crate) fn constructor(
        candidate_init_dict: RTCIceCandidateInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: If both the sdpMid and sdpMLineIndex members of
        //         candidateInitDict are null, throw a TypeError.
        if candidate_init_dict.sdp_mid.is_none() && candidate_init_dict.sdp_m_line_index.is_none() {
            return Err(ec.new_type_error("sdpMid and sdpMLineIndex are both null"));
        }
        // Step 2: Return the result of creating an RTCIceCandidate with
        //         candidateInitDict.
        Ok(Self::create(candidate_init_dict))
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcicecandidate>
    pub(crate) fn create(candidate_init_dict: RTCIceCandidateInit) -> Self {
        // Step 1: Let iceCandidate be a newly created RTCIceCandidate object.
        // Step 2: Create internal slots for the following attributes of
        //         iceCandidate, initilized to null: foundation, component,
        //         priority, address, protocol, port, type, tcpType,
        //         relatedAddress, and relatedPort.
        let mut parsed = ParsedCandidate::default();
        // Step 3: Create internal slots for the following attributes of
        //         iceCandidate, initilized to their namesakes in
        //         candidateInitDict: candidate, sdpMid, sdpMLineIndex,
        //         usernameFragment, relayProtocol, url.
        // Step 4: Let candidate be the candidate dictionary member of
        //         candidateInitDict. If candidate is not an empty string, run
        //         the following steps:
        if !candidate_init_dict.candidate.is_empty() {
            // Step 4.1: Parse candidate using the candidate-attribute grammar.
            // Step 4.2: If parsing of candidate-attribute has failed, abort
            //           these steps.
            // Step 4.3: If any field in the parse result represents an invalid
            //           value for the corresponding attribute in iceCandidate,
            //           abort these steps.
            // Step 4.4: Set the corresponding internal slots in iceCandidate
            //           to the field values of the parsed result.
            if let Some(result) = parse_candidate_attribute(&candidate_init_dict.candidate) {
                parsed = result;
            }
        }
        // Step 5: Return iceCandidate.
        Self {
            init: candidate_init_dict,
            parsed,
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidate-tojson>
    pub(crate) fn to_json(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let json be a new RTCIceCandidateInit dictionary.
        let json = ec.create_plain_object(None);
        // Step 2: For each attribute identifier attr in «candidate, sdpMid,
        //         sdpMLineIndex, usernameFragment»:
        // Step 2.1: Let value be the result of getting the underlying value
        //           of the attribute identified by attr, given this
        //           RTCIceCandidate object.
        // Step 2.2: Set json[attr] to value.
        let candidate = string_value(&self.init.candidate, ec);
        let sdp_mid = nullable_string_value(self.init.sdp_mid.as_deref(), ec);
        let sdp_m_line_index = match self.init.sdp_m_line_index {
            Some(index) => ec.value_from_number(f64::from(index)),
            None => ec.value_null(),
        };
        let username_fragment = nullable_string_value(self.init.username_fragment.as_deref(), ec);
        for (key, value) in [
            ("candidate", candidate),
            ("sdpMid", sdp_mid),
            ("sdpMLineIndex", sdp_m_line_index),
            ("usernameFragment", username_fragment),
        ] {
            let key = ec.property_key_from_str(key);
            ec.create_data_property(json.clone(), key, value)?;
        }
        // Step 3: Return json.
        Ok(json)
    }
}

pub(crate) fn string_value(value: &str, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

pub(crate) fn nullable_string_value(
    value: Option<&str>,
    ec: &mut dyn ExecutionContext<Types>,
) -> JsValue {
    match value {
        Some(value) => string_value(value, ec),
        None => ec.value_null(),
    }
}

/// candidate-attribute = "candidate" ":" foundation SP component-id SP
/// transport SP priority SP connection-address SP port SP cand-type
/// [SP rel-addr] [SP rel-port] *(SP cand-extension)
/// <https://www.rfc-editor.org/rfc/rfc8839#section-5.1>
fn parse_candidate_attribute(candidate: &str) -> Option<ParsedCandidate> {
    let text = candidate.strip_prefix("a=").unwrap_or(candidate);
    let text = text.strip_prefix("candidate:")?;
    let mut fields = text.split_ascii_whitespace();
    let foundation = fields.next()?.to_owned();
    let component = match fields.next()? {
        "1" => "rtp",
        "2" => "rtcp",
        _ => return None,
    };
    let protocol = match fields.next()?.to_ascii_lowercase().as_str() {
        "udp" => "udp",
        "tcp" => "tcp",
        _ => return None,
    };
    let priority = fields.next()?.parse::<u32>().ok()?;
    let address = fields.next()?.to_owned();
    let port = fields.next()?.parse::<u16>().ok()?;
    if fields.next()? != "typ" {
        return None;
    }
    let type_ = match fields.next()? {
        "host" => "host",
        "srflx" => "srflx",
        "prflx" => "prflx",
        "relay" => "relay",
        _ => return None,
    };
    let mut parsed = ParsedCandidate {
        foundation: Some(foundation),
        component: Some(component),
        priority: Some(priority),
        address: Some(address),
        protocol: Some(protocol),
        port: Some(port),
        type_: Some(type_),
        ..ParsedCandidate::default()
    };
    while let (Some(name), Some(value)) = (fields.next(), fields.next()) {
        match name {
            "raddr" => parsed.related_address = Some(value.to_owned()),
            "rport" => parsed.related_port = value.parse().ok(),
            "tcptype" => {
                parsed.tcp_type = match value {
                    "active" => Some("active"),
                    "passive" => Some("passive"),
                    "so" => Some("so"),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    Some(parsed)
}
