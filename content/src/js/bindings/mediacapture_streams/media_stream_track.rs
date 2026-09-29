use crate::js::Types;
use crate::mediacapture_streams::MediaStreamTrack;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{event_handlers, member, string_value, this_as, track_value};

type JsValue = <Types as JsTypes>::JsValue;

fn track(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<MediaStreamTrack, Types> {
    this_as::<MediaStreamTrack>(this, "MediaStreamTrack", ec)
}

impl WebIdlInterface<Types> for MediaStreamTrack {
    const NAME: &'static str = "MediaStreamTrack";

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
        member!(def, attribute "kind", kind);
        member!(def, attribute "id", id);
        member!(def, attribute "label", label);
        member!(def, attribute "enabled", enabled, set_enabled);
        member!(def, attribute "muted", muted);
        member!(def, attribute "readyState", ready_state);
        member!(def, attribute "contentHint", content_hint);
        member!(def, operation "clone", 0, clone_method, promise false);
        member!(def, operation "stop", 0, stop, promise false);
        member!(def, operation "getCapabilities", 0, empty_dictionary, promise false);
        member!(def, operation "getConstraints", 0, empty_dictionary, promise false);
        member!(def, operation "getSettings", 0, get_settings, promise false);
        member!(def, operation "applyConstraints", 0, apply_constraints, promise true);
        member!(def, attribute "onmute", get_onmute, set_onmute);
        member!(def, attribute "onunmute", get_onunmute, set_onunmute);
        member!(def, attribute "onended", get_onended, set_onended);
    }
}

event_handlers!(
    get_onmute, set_onmute, "mute";
    get_onunmute, set_onunmute, "unmute";
    get_onended, set_onended, "ended";
);

fn kind(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let kind = track(this, ec)?.kind_idl();
    Ok(string_value(kind, ec))
}

fn id(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let id = track(this, ec)?.id();
    Ok(string_value(&id, ec))
}

fn label(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let label = track(this, ec)?.label();
    Ok(string_value(&label, ec))
}

fn enabled(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let enabled = track(this, ec)?.enabled();
    Ok(ec.value_from_bool(enabled))
}

fn set_enabled(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let track = track(this, ec)?;
    let undefined = ec.value_undefined();
    let enabled = ec.to_boolean(args.first().unwrap_or(&undefined));
    track.set_enabled(enabled);
    Ok(ec.value_undefined())
}

fn muted(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let muted = track(this, ec)?.muted();
    Ok(ec.value_from_bool(muted))
}

fn ready_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let state = track(this, ec)?.ready_state();
    Ok(string_value(state.as_idl(), ec))
}

fn content_hint(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    track(this, ec)?;
    Ok(string_value("", ec))
}

fn clone_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let clone = track(this, ec)?.clone_track(ec)?;
    Ok(track_value(&clone, ec))
}

fn stop(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    track(this, ec)?.stop();
    Ok(ec.value_undefined())
}

fn empty_dictionary(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    track(this, ec)?;
    let object = ec.create_plain_object(None);
    Ok(Types::value_from_object(object))
}

fn get_settings(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let track = track(this, ec)?;
    let object = ec.create_plain_object(None);
    let device_id = match track.source() {
        crate::mediacapture_streams::TrackSource::Capture { device_id } => Some(device_id),
        crate::mediacapture_streams::TrackSource::Remote { .. } => None,
    };
    if let Some(device_id) = device_id {
        let key = ec.property_key_from_str("deviceId");
        let value = string_value(&device_id, ec);
        ec.create_data_property(object.clone(), key, value)?;
    }
    Ok(Types::value_from_object(object))
}

fn apply_constraints(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    track(this, ec)?;
    let undefined = ec.value_undefined();
    let promise = crate::webidl::resolved_promise(undefined, ec)?;
    Ok(Types::value_from_object(promise))
}
