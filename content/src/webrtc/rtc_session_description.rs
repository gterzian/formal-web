use ipc_messages::webrtc::{SdpType, SessionDescription};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;
use crate::webidl::bindings::create_interface_instance;

type JsObject = <Types as JsTypes>::JsObject;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsdptype>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RTCSdpType {
    Offer,
    Pranswer,
    Answer,
    Rollback,
}

impl RTCSdpType {
    /// <https://webidl.spec.whatwg.org/#js-enumeration>
    pub(crate) fn from_idl(value: &str) -> Option<Self> {
        match value {
            "offer" => Some(Self::Offer),
            "pranswer" => Some(Self::Pranswer),
            "answer" => Some(Self::Answer),
            "rollback" => Some(Self::Rollback),
            _ => None,
        }
    }

    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::Offer => "offer",
            Self::Pranswer => "pranswer",
            Self::Answer => "answer",
            Self::Rollback => "rollback",
        }
    }

    pub(crate) fn to_ipc(self) -> SdpType {
        match self {
            Self::Offer => SdpType::Offer,
            Self::Pranswer => SdpType::Pranswer,
            Self::Answer => SdpType::Answer,
            Self::Rollback => SdpType::Rollback,
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescriptioninit>
#[derive(Debug, Clone)]
pub(crate) struct RTCSessionDescriptionInit {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescriptioninit-type>
    pub(crate) type_: RTCSdpType,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescriptioninit-sdp>
    pub(crate) sdp: String,
}

impl RTCSessionDescriptionInit {
    pub(crate) fn to_ipc(&self) -> SessionDescription {
        SessionDescription {
            kind: self.type_.to_ipc(),
            sdp: self.sdp.clone(),
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescription>
#[gc_struct]
pub(crate) struct RTCSessionDescription {
    /// <https://w3c.github.io/webrtc-pc/#dom-sessiondescription-type>
    #[ignore_trace]
    pub(crate) type_: RTCSdpType,

    /// <https://w3c.github.io/webrtc-pc/#dom-sessiondescription-sdp>
    #[ignore_trace]
    pub(crate) sdp: String,
}

impl RTCSessionDescription {
    /// <https://w3c.github.io/webrtc-pc/#dom-sessiondescription>
    pub(crate) fn constructor(description_init_dict: RTCSessionDescriptionInit) -> Self {
        // Note: The constructor has no numbered steps: it "takes a dictionary
        // argument, description, whose content is used to initialize the new
        // RTCSessionDescription object".
        Self {
            type_: description_init_dict.type_,
            sdp: description_init_dict.sdp,
        }
    }

    /// A new RTCSessionDescription platform object in the current realm,
    /// "constructed from description" as the set-description steps say.
    pub(crate) fn new_object(
        description: &RTCSessionDescriptionInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        create_interface_instance::<Types, RTCSessionDescription>(
            Self::constructor(description.clone()),
            ec,
        )
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescription-tojson>
    pub(crate) fn to_json(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Note: A default toJSON: an object with the type and sdp attributes
        // (<https://webidl.spec.whatwg.org/#default-tojson-steps>).
        description_init_object(self.type_, &self.sdp, ec)
    }
}

/// An RTCSessionDescriptionInit dictionary converted to a JavaScript object
/// (<https://webidl.spec.whatwg.org/#js-dictionary>, IDL to JavaScript).
pub(crate) fn description_init_object(
    type_: RTCSdpType,
    sdp: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let object = ec.create_plain_object(None);
    let sdp_value = {
        let string = ec.js_string_from_str(sdp);
        ec.value_from_string(string)
    };
    let type_value = {
        let string = ec.js_string_from_str(type_.as_idl());
        ec.value_from_string(string)
    };
    // Dictionary members are converted in lexicographical order.
    let sdp_key = ec.property_key_from_str("sdp");
    ec.create_data_property(object.clone(), sdp_key, sdp_value)?;
    let type_key = ec.property_key_from_str("type");
    ec.create_data_property(object.clone(), type_key, type_value)?;
    Ok(object)
}
