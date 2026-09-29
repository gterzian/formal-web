use crate::js::Types;
use crate::js::bindings::initialization::init_flag;
use crate::mediacapture_streams::MediaStreamTrackEvent;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::dictionary;
use super::{member, this_as, track_from_value, track_value};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for MediaStreamTrackEvent {
    const NAME: &'static str = "MediaStreamTrackEvent";

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
        let track = dict
            .get_member("track", ec)?
            .and_then(|value| track_from_value(&value, ec))
            .ok_or_else(|| {
                ec.new_type_error("MediaStreamTrackEventInit: member track is required")
            })?;
        Ok(MediaStreamTrackEvent::new(
            type_,
            init_flag(&init, "bubbles", ec)?,
            init_flag(&init, "cancelable", ec)?,
            init_flag(&init, "composed", ec)?,
            track,
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "track", track);
    }
}

fn track(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<MediaStreamTrackEvent>(this, "MediaStreamTrackEvent", ec)?;
    Ok(track_value(&event.track, ec))
}
