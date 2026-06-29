// ── HTMLMediaElement JS bindings ──
//
// Note: These bindings define *which members* HTMLMediaElement exposes.
// Domain methods on HTMLMediaElement implement *what those members do*.
//
// Only a subset of the full IDL is exposed for the initial video cut.

use boa_engine::{JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::html::HTMLMediaElement;
use crate::html::HTMLVideoElement;
use crate::webidl::bindings::{
    AttributeDef, ConstantDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLMediaElement {
    const NAME: &'static str = "HTMLMediaElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
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

        // Constants
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NETWORK_EMPTY",
            value: JsValue::from(HTMLMediaElement::NETWORK_EMPTY as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NETWORK_IDLE",
            value: JsValue::from(HTMLMediaElement::NETWORK_IDLE as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NETWORK_LOADING",
            value: JsValue::from(HTMLMediaElement::NETWORK_LOADING as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NETWORK_NO_SOURCE",
            value: JsValue::from(HTMLMediaElement::NETWORK_NO_SOURCE as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "HAVE_NOTHING",
            value: JsValue::from(HTMLMediaElement::HAVE_NOTHING as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "HAVE_METADATA",
            value: JsValue::from(HTMLMediaElement::HAVE_METADATA as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "HAVE_CURRENT_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_CURRENT_DATA as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "HAVE_FUTURE_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_FUTURE_DATA as i32),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "HAVE_ENOUGH_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_ENOUGH_DATA as i32),
        });

        // network state
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "networkState",
            getter: get_network_state,
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

            id: "readyState",
            getter: get_ready_state,
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

            id: "src",
            getter: get_src,
            setter: Some(set_src),
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

            id: "currentSrc",
            getter: get_current_src,
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

            id: "duration",
            getter: get_duration,
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

            id: "paused",
            getter: get_paused,
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

            id: "seeking",
            getter: get_seeking,
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

            id: "currentTime",
            getter: get_current_time,
            setter: Some(set_current_time),
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

            id: "error",
            getter: get_error,
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

            id: "autoplay",
            getter: get_autoplay,
            setter: Some(set_autoplay),
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

            id: "loop",
            getter: get_loop,
            setter: Some(set_loop),
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

            id: "controls",
            getter: get_controls,
            setter: Some(set_controls),
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

            id: "muted",
            getter: get_muted,
            setter: Some(set_muted),
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

            id: "volume",
            getter: get_volume,
            setter: Some(set_volume),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        // Operations
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "load",
            length: 0,
            method: load_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "play",
            length: 0,
            method: play_method,
            static_: false,
            unforgeable: false,
            promise_type: true,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "pause",
            length: 0,
            method: pause_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "canPlayType",
            length: 1,
            method: can_play_type,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });

        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "preload",
            getter: get_preload,
            setter: Some(set_preload),
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

// ── Attribute getters/setters ──

fn get_network_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let state = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.network_state()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.network_state()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_number(state as f64))
}

fn get_ready_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let state = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.ready_state()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.ready_state()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_number(state as f64))
}

fn get_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let src = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.src()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.src()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let ec_ref = js_engine::boa::context_as_ec(ctx);
    let obj = match this.as_object() {
        Some(o) => o,
        None => return Ok(value_undefined),
    };
    let src = args
        .first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    if let Some(mut media) = obj.downcast_mut::<HTMLMediaElement>() {
        media.set_src(&src, ec_ref);
    } else if let Some(mut video) = obj.downcast_mut::<HTMLVideoElement>() {
        video.media_element.set_src(&src, ec_ref);
    } else {
        return Err(crate::js::native_error_to_js_value(
            JsNativeError::typ().with_message("expected HTMLMediaElement"),
            ctx,
        ));
    }
    Ok(JsValue::undefined())
}

fn get_current_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let src = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.current_src()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.current_src()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn get_duration(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let duration = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.duration()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.duration()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_number(duration))
}

fn get_paused(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let paused = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.paused()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.paused()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(paused))
}

fn get_seeking(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let seeking = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.seeking()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.seeking()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(seeking))
}

fn get_current_time(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let time = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.current_time()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.current_time()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_number(time))
}

fn set_current_time(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    // TODO: Implement using interior mutability.
    Ok(ec.value_undefined())
}

fn get_error(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if obj.downcast_ref::<HTMLMediaElement>().is_some()
        || obj.downcast_ref::<HTMLVideoElement>().is_some()
    {
        Ok(ec.value_null())
    } else {
        Err(ec.new_type_error("expected HTMLMediaElement"))
    }
}

fn get_autoplay(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let val = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.autoplay()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.autoplay()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(val))
}

fn set_autoplay(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.set_autoplay(value);
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.set_autoplay(value);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn get_loop(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let val = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.loop_()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.loop_()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(val))
}

fn set_loop(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.set_loop(value);
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.set_loop(value);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn get_controls(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let val = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.controls()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.controls()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(val))
}

fn set_controls(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.set_controls(value);
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.set_controls(value);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn get_muted(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let val = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.muted()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.muted()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_bool(val))
}

fn set_muted(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.set_muted(value);
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.set_muted(value);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn get_volume(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let vol = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.volume()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.volume()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_number(vol))
}

fn set_volume(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let vol = args
        .first()
        .and_then(|v| crate::js::Types::value_as_number(v))
        .unwrap_or(1.0);
    if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.set_volume(vol);
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.set_volume(vol);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn get_preload(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let preload = if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
        media.preload()
    } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
        video.media_element.preload()
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    };
    Ok(ec.value_from_string(ec.js_string_from_str(&preload)))
}

fn set_preload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let obj = this
            .as_object()
            .ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
        let value = args
            .first()
            .and_then(|v| v.as_string())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        if let Some(media) = obj.downcast_ref::<HTMLMediaElement>() {
            media.set_preload(&value);
        } else if let Some(video) = obj.downcast_ref::<HTMLVideoElement>() {
            video.media_element.set_preload(&value);
        } else {
            return Err(JsNativeError::typ()
                .with_message("expected HTMLMediaElement")
                .into());
        }
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

// ── Operations ──

fn load_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    // Note: load() takes &mut self and requires interior mutability. The HTMLMediaElement
    // is behind a plain &ref in the binding layer. Adding RefCell support is tracked
    // as a separate gap — this binding currently returns undefined.
    Ok(ec.value_undefined())
}

fn play_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if let Some(mut media) = obj.downcast_mut::<HTMLMediaElement>() {
        return media.play(ec);
    } else if let Some(mut video) = obj.downcast_mut::<HTMLVideoElement>() {
        return video.media_element.play(ec);
    }
    Err(ec.new_type_error("expected HTMLMediaElement"))
}

fn pause_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if let Some(mut media) = obj.downcast_mut::<HTMLMediaElement>() {
        media.pause(ec);
    } else if let Some(mut video) = obj.downcast_mut::<HTMLVideoElement>() {
        video.media_element.pause(ec);
    } else {
        return Err(ec.new_type_error("expected HTMLMediaElement"));
    }
    Ok(ec.value_undefined())
}

fn can_play_type(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    // Step 1: Return "probably" if the type is a media type that can be rendered.
    // Initial cut: return empty string (no types supported).
    Ok(ec.value_from_string(ec.js_string_from_str("")))
}
