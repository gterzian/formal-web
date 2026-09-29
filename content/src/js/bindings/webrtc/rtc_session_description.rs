use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webrtc::RTCSessionDescription;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::rtc_peer_connection::convert_session_description_init;
use super::{member, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn description(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RTCSessionDescription, Types> {
    this_as::<RTCSessionDescription>(this, "RTCSessionDescription", ec)
}

impl WebIdlInterface<Types> for RTCSessionDescription {
    const NAME: &'static str = "RTCSessionDescription";

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let description_init_dict = convert_session_description_init(args.first(), ec)?;
        Ok(RTCSessionDescription::constructor(description_init_dict))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "type", type_);
        member!(def, attribute "sdp", sdp);
        member!(def, operation "toJSON", 0, to_json, promise false);
    }
}

fn type_(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let type_ = description(this, ec)?.type_;
    let string = ec.js_string_from_str(type_.as_idl());
    Ok(ec.value_from_string(string))
}

fn sdp(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sdp = description(this, ec)?.sdp.clone();
    let string = ec.js_string_from_str(&sdp);
    Ok(ec.value_from_string(string))
}

fn to_json(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let description = description(this, ec)?;
    description.to_json(ec).map(Types::value_from_object)
}
