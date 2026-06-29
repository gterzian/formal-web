// ── HTMLVideoElement JS bindings ──

use boa_engine::{JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::html::HTMLVideoElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<js_engine::boa::BoaTypes> for HTMLVideoElement {
    const NAME: &'static str = "HTMLVideoElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLMediaElement")
    }

    /// When the `media` feature is disabled, the constructor throws
    /// `NotSupportedError` and the interface has no members.
    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        #[cfg(not(feature = "media"))]
        {
            return Err(ec.new_type_error(
                "NotSupportedError: Media not available (media feature disabled)",
            ));
        }
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        #[cfg(not(feature = "media"))]
        {
            // No members when media is disabled — the interface exists but is empty.
            let _ = def;
            return;
        }

        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
        });
    }
}

fn get_video_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_number(video.video_width() as f64))
}

fn get_video_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_number(video.video_height() as f64))
}

fn get_poster(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&video.poster())))
}

fn set_poster(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
        let video = obj
            .downcast_ref::<HTMLVideoElement>()
            .ok_or_else(|| JsNativeError::typ().with_message("expected HTMLVideoElement"))?;
        let poster = args
            .first()
            .and_then(|v| v.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        video.set_poster(&poster);
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_plays_inline(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_bool(video.plays_inline()))
}

fn set_plays_inline(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    let value = args.first().map_or(false, |v| v.to_boolean());
    video.set_plays_inline(value);
    Ok(ec.value_undefined())
}

fn get_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&video.width())))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
        let video = obj
            .downcast_ref::<HTMLVideoElement>()
            .ok_or_else(|| JsNativeError::typ().with_message("expected HTMLVideoElement"))?;
        let value = args
            .first()
            .and_then(|v| v.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        video.set_width(&value);
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let obj = BoaTypes::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let video = obj
        .downcast_ref::<HTMLVideoElement>()
        .ok_or_else(|| ec.new_type_error("expected HTMLVideoElement"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&video.height())))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
        let video = obj
            .downcast_ref::<HTMLVideoElement>()
            .ok_or_else(|| JsNativeError::typ().with_message("expected HTMLVideoElement"))?;
        let value = args
            .first()
            .and_then(|v| v.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        video.set_height(&value);
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
