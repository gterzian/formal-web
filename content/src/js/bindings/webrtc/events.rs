use crate::js::Types;
use crate::js::bindings::initialization::init_flag;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webrtc::events::RTCPeerConnectionIceEventInit;
use crate::webrtc::{
    RTCDataChannel, RTCDataChannelEvent, RTCIceCandidate, RTCPeerConnectionIceEvent,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{dictionary, member, nullable_string_member, this_as};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for RTCPeerConnectionIceEvent {
    const NAME: &'static str = "RTCPeerConnectionIceEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // <https://webidl.spec.whatwg.org/#dfn-overload-resolution>: the
        // type argument is required.
        if args.is_empty() {
            return Err(ec.new_type_error("RTCPeerConnectionIceEvent needs a type"));
        }
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined.clone()))?;
        let init = args.get(1).cloned().unwrap_or(undefined);
        let dict = dictionary(Some(&init), ec)?;
        // candidate: RTCIceCandidate?
        let candidate = match dict.get_member("candidate", ec)? {
            Some(value) if !Types::value_is_null(&value) => {
                let object = Types::value_as_object(&value)
                    .filter(|object| {
                        ec.with_object_any(object)
                            .is_some_and(|data| data.downcast_ref::<RTCIceCandidate>().is_some())
                    })
                    .ok_or_else(|| ec.new_type_error("candidate is not an RTCIceCandidate"))?;
                Some(object)
            }
            _ => None,
        };
        let url = nullable_string_member(&dict, "url", ec)?;
        Ok(RTCPeerConnectionIceEvent::new(
            type_,
            RTCPeerConnectionIceEventInit {
                bubbles: init_flag(&init, "bubbles", ec)?,
                cancelable: init_flag(&init, "cancelable", ec)?,
                composed: init_flag(&init, "composed", ec)?,
                candidate,
                url,
            },
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "candidate", ice_event_candidate);
        member!(def, attribute "url", ice_event_url);
    }
}

fn ice_event_candidate(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<RTCPeerConnectionIceEvent>(this, "RTCPeerConnectionIceEvent", ec)?;
    let candidate = event.candidate.borrow(ec).clone();
    Ok(match candidate {
        Some(candidate) => Types::value_from_object(candidate),
        None => ec.value_null(),
    })
}

fn ice_event_url(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<RTCPeerConnectionIceEvent>(this, "RTCPeerConnectionIceEvent", ec)?;
    Ok(crate::webrtc::rtc_ice_candidate::nullable_string_value(
        event.url.as_deref(),
        ec,
    ))
}

impl WebIdlInterface<Types> for RTCDataChannelEvent {
    const NAME: &'static str = "RTCDataChannelEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn constructor_length() -> usize {
        2
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined.clone()))?;
        let init = args.get(1).cloned().unwrap_or(undefined);
        let dict = dictionary(Some(&init), ec)?;
        // channel: RTCDataChannel, required.
        let channel = dict
            .get_member("channel", ec)?
            .and_then(|value| Types::value_as_object(&value))
            .filter(|object| {
                ec.with_object_any(object)
                    .is_some_and(|data| data.downcast_ref::<RTCDataChannel>().is_some())
            })
            .ok_or_else(|| {
                ec.new_type_error("RTCDataChannelEventInit.channel must be an RTCDataChannel")
            })?;
        Ok(RTCDataChannelEvent::new(
            type_,
            init_flag(&init, "bubbles", ec)?,
            init_flag(&init, "cancelable", ec)?,
            init_flag(&init, "composed", ec)?,
            channel,
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "channel", data_channel_event_channel);
    }
}

fn data_channel_event_channel(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<RTCDataChannelEvent>(this, "RTCDataChannelEvent", ec)?;
    Ok(Types::value_from_object(event.channel.clone()))
}
