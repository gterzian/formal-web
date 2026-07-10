// ── HTMLVideoElement JS bindings ──

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLVideoElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLVideoElement {
    const NAME: &'static str = "HTMLVideoElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLMediaElement")
    }

    /// When the `media` feature is disabled, the constructor throws
    /// `NotSupportedError` and the interface has no members.
    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        #[cfg(not(feature = "media"))]
        {
            return Err(ec.new_type_error(
                "NotSupportedError: Media not available (media feature disabled)",
            ));
        }
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        #[cfg(not(feature = "media"))]
        {
            // No members when media is disabled — the interface exists but is empty.
            let _ = def;
            return;
        }

        def.add_attribute(AttributeDef {
            id: "videoWidth",
            getter: get_video_width,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "videoHeight",
            getter: get_video_height,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "poster",
            getter: get_poster,
            setter: Some(set_poster),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "playsInline",
            getter: get_plays_inline,
            setter: Some(set_plays_inline),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "width",
            getter: get_width,
            setter: Some(set_width),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "height",
            getter: get_height,
            setter: Some(set_height),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
    }
}

fn try_with_video_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLVideoElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            return Ok(f(video));
        }
    }
    Err(ec.new_type_error("expected HTMLVideoElement"))
}

fn get_video_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let width = try_with_video_ref(this, ec, |v| v.video_width())?;
    Ok(ec.value_from_number(width as f64))
}

fn get_video_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let height = try_with_video_ref(this, ec, |v| v.video_height())?;
    Ok(ec.value_from_number(height as f64))
}

fn get_poster(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let poster = try_with_video_ref(this, ec, |v| v.poster())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&poster)))
}

fn set_poster(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let poster = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_video_ref(this, ec, |v| v.set_poster(&poster))?;
    Ok(ec.value_undefined())
}

fn get_plays_inline(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let plays = try_with_video_ref(this, ec, |v| v.plays_inline())?;
    Ok(ec.value_from_bool(plays))
}

fn set_plays_inline(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_video_ref(this, ec, |v| v.set_plays_inline(value))?;
    Ok(ec.value_undefined())
}

fn get_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let width = try_with_video_ref(this, ec, |v| v.width())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&width)))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_video_ref(this, ec, |v| v.set_width(&value))?;
    Ok(ec.value_undefined())
}

fn get_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let height = try_with_video_ref(this, ec, |v| v.height())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&height)))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_video_ref(this, ec, |v| v.set_height(&value))?;
    Ok(ec.value_undefined())
}
