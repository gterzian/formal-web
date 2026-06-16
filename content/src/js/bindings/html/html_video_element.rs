// ── HTMLVideoElement JS bindings ──

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue,
};

use crate::html::HTMLVideoElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

impl WebIdlInterface for HTMLVideoElement {
    const NAME: &'static str = "HTMLVideoElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLMediaElement")
    }

    /// When the `media` feature is disabled, the constructor throws
    /// `NotSupportedError` and the interface has no members.
    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        #[cfg(not(feature = "media"))]
        {
            return Err(JsNativeError::typ()
                .with_message("NotSupportedError: Media not available (media feature disabled)")
                .into());
        }
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn define_members(def: &mut InterfaceDefinition) {
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
        });
    }
}

fn get_video_width(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(video.video_width()))
}

fn get_video_height(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(video.video_height()))
}

fn get_poster(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(JsString::from(video.poster())))
}

fn set_poster(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    let poster = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    video.set_poster(&poster);
    Ok(JsValue::undefined())
}

fn get_plays_inline(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(video.plays_inline()))
}

fn set_plays_inline(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    let value = args.first().map(|v| v.to_boolean()).unwrap_or(false);
    video.set_plays_inline(value);
    Ok(JsValue::undefined())
}

fn get_width(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(JsString::from(video.width())))
}

fn set_width(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    let value = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    video.set_width(&value);
    Ok(JsValue::undefined())
}

fn get_height(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    Ok(JsValue::from(JsString::from(video.height())))
}

fn set_height(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let video = obj.downcast_ref::<HTMLVideoElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLVideoElement")
    })?;
    let value = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    video.set_height(&value);
    Ok(JsValue::undefined())
}
