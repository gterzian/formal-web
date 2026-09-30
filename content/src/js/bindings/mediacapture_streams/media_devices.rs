use crate::js::Types;
use crate::mediacapture_streams::{MediaDeviceInfo, MediaDevices, MediaStreamConstraints};
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::dictionary;
use super::{event_handlers, member, string_value, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn devices(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<MediaDevices, Types> {
    this_as::<MediaDevices>(this, "MediaDevices", ec)
}

impl WebIdlInterface<Types> for MediaDevices {
    const NAME: &'static str = "MediaDevices";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, operation "enumerateDevices", 0, enumerate_devices, promise true);
        member!(def, operation "getSupportedConstraints", 0, get_supported_constraints, promise false);
        member!(def, operation "getUserMedia", 0, get_user_media, promise true);
        member!(def, operation "getDisplayMedia", 0, get_display_media, promise true);
        member!(def, attribute "ondevicechange", get_ondevicechange, set_ondevicechange);
    }
}

event_handlers!(
    get_ondevicechange, set_ondevicechange, "devicechange";
);

fn enumerate_devices(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = devices(this, ec)?.enumerate_devices(ec)?;
    Ok(Types::value_from_object(promise))
}

fn get_supported_constraints(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    devices(this, ec)?;
    let object = ec.create_plain_object(None);
    for name in [
        "deviceId",
        "groupId",
        "echoCancellation",
        "noiseSuppression",
        "autoGainControl",
    ] {
        let key = ec.property_key_from_str(name);
        let value = ec.value_from_bool(true);
        ec.create_data_property(object.clone(), key, value)?;
    }
    Ok(Types::value_from_object(object))
}

fn requested(
    dict: &crate::webidl::dictionary::DictionaryAccess<Types>,
    key: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<bool, Types> {
    Ok(match dict.get_member(key, ec)? {
        None => false,
        Some(value) if Types::value_as_object(&value).is_some() => true,
        Some(value) => ec.to_boolean(&value),
    })
}

fn get_user_media(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let devices = devices(this, ec)?;
    let dict = dictionary(args.first(), ec)?;
    let constraints = MediaStreamConstraints {
        audio: requested(&dict, "audio", ec)?,
        video: requested(&dict, "video", ec)?,
    };
    let promise = devices.get_user_media(constraints, ec)?;
    Ok(Types::value_from_object(promise))
}

fn get_display_media(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = devices(this, ec)?.get_display_media(ec)?;
    Ok(Types::value_from_object(promise))
}

fn info(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<MediaDeviceInfo, Types> {
    this_as::<MediaDeviceInfo>(this, "MediaDeviceInfo", ec)
}

impl WebIdlInterface<Types> for MediaDeviceInfo {
    const NAME: &'static str = "MediaDeviceInfo";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "deviceId", device_id);
        member!(def, attribute "kind", kind);
        member!(def, attribute "label", label);
        member!(def, attribute "groupId", group_id);
        member!(def, operation "toJSON", 0, to_json, promise false);
    }
}

fn device_id(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = info(this, ec)?.device_id.clone();
    Ok(string_value(&value, ec))
}

fn kind(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = info(this, ec)?.kind.as_idl();
    Ok(string_value(value, ec))
}

fn label(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = info(this, ec)?.label.clone();
    Ok(string_value(&value, ec))
}

fn group_id(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = info(this, ec)?.group_id.clone();
    Ok(string_value(&value, ec))
}

fn to_json(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let info = info(this, ec)?;
    let object = ec.create_plain_object(None);
    for (name, value) in [
        ("deviceId", info.device_id.clone()),
        ("kind", info.kind.as_idl().to_owned()),
        ("label", info.label.clone()),
        ("groupId", info.group_id.clone()),
    ] {
        let key = ec.property_key_from_str(name);
        let value = string_value(&value, ec);
        ec.create_data_property(object.clone(), key, value)?;
    }
    Ok(Types::value_from_object(object))
}
