// ── HTMLMediaElement JS bindings ──
//
// Note: These bindings define *which members* HTMLMediaElement exposes.
// Domain methods on HTMLMediaElement implement *what those members do*.
//
// Only a subset of the full IDL is exposed for the initial video cut.

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue,
};

use crate::html::HTMLMediaElement;
use crate::webidl::bindings::{
    AttributeDef, ConstantDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

impl WebIdlInterface for HTMLMediaElement {
    const NAME: &'static str = "HTMLMediaElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        // Constants
        def.add_constant(ConstantDef {
            id: "NETWORK_EMPTY",
            value: JsValue::from(HTMLMediaElement::NETWORK_EMPTY as i32),
        });
        def.add_constant(ConstantDef {
            id: "NETWORK_IDLE",
            value: JsValue::from(HTMLMediaElement::NETWORK_IDLE as i32),
        });
        def.add_constant(ConstantDef {
            id: "NETWORK_LOADING",
            value: JsValue::from(HTMLMediaElement::NETWORK_LOADING as i32),
        });
        def.add_constant(ConstantDef {
            id: "NETWORK_NO_SOURCE",
            value: JsValue::from(HTMLMediaElement::NETWORK_NO_SOURCE as i32),
        });
        def.add_constant(ConstantDef {
            id: "HAVE_NOTHING",
            value: JsValue::from(HTMLMediaElement::HAVE_NOTHING as i32),
        });
        def.add_constant(ConstantDef {
            id: "HAVE_METADATA",
            value: JsValue::from(HTMLMediaElement::HAVE_METADATA as i32),
        });
        def.add_constant(ConstantDef {
            id: "HAVE_CURRENT_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_CURRENT_DATA as i32),
        });
        def.add_constant(ConstantDef {
            id: "HAVE_FUTURE_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_FUTURE_DATA as i32),
        });
        def.add_constant(ConstantDef {
            id: "HAVE_ENOUGH_DATA",
            value: JsValue::from(HTMLMediaElement::HAVE_ENOUGH_DATA as i32),
        });

        // network state
        def.add_attribute(AttributeDef {
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
            id: "load",
            length: 0,
            method: load_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "canPlayType",
            length: 1,
            method: can_play_type,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });

        def.add_attribute(AttributeDef {
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

fn get_network_state(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.network_state()))
}

fn get_ready_state(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.ready_state()))
}

fn get_src(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::from(JsString::from(media.src())))
}

fn set_src(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let src = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    media.set_src(&src);
    Ok(JsValue::undefined())
}

fn get_current_src(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::from(JsString::from(media.current_src())))
}

fn get_duration(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.duration()))
}

fn get_paused(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.paused()))
}

fn get_seeking(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.seeking()))
}

fn get_current_time(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.current_time()))
}

fn set_current_time(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let _media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    // TODO: Implement using interior mutability.
    Ok(JsValue::undefined())
}

fn get_error(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    match media.error() {
        Some(_) => Ok(JsValue::null()),
        None => Ok(JsValue::null()),
    }
}

fn get_autoplay(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.autoplay()))
}

fn set_autoplay(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let value = args.first().map(|v| v.to_boolean()).unwrap_or(false);
    media.set_autoplay(value);
    Ok(JsValue::undefined())
}

fn get_loop(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.loop_()))
}

fn set_loop(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let value = args.first().map(|v| v.to_boolean()).unwrap_or(false);
    media.set_loop(value);
    Ok(JsValue::undefined())
}

fn get_controls(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.controls()))
}

fn set_controls(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let value = args.first().map(|v| v.to_boolean()).unwrap_or(false);
    media.set_controls(value);
    Ok(JsValue::undefined())
}

fn get_muted(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.muted()))
}

fn set_muted(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let value = args.first().map(|v| v.to_boolean()).unwrap_or(false);
    media.set_muted(value);
    Ok(JsValue::undefined())
}

fn get_volume(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::new(media.volume()))
}

fn set_volume(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let volume = args.first().and_then(|v| v.as_number()).unwrap_or(1.0);
    media.set_volume(volume);
    Ok(JsValue::undefined())
}

fn get_preload(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    Ok(JsValue::from(JsString::from(media.preload())))
}

fn set_preload(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    let media = obj.downcast_ref::<HTMLMediaElement>().ok_or_else(|| {
        JsNativeError::typ().with_message("expected HTMLMediaElement")
    })?;
    let value = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    media.set_preload(&value);
    Ok(JsValue::undefined())
}

// ── Operations ──

fn load_method(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let _obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    Ok(JsValue::undefined())
}

fn can_play_type(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let _obj = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("expected object"))?;
    // Step 1: Return "probably" if the type is a media type that can be rendered.
    // Initial cut: return empty string (no types supported).
    Ok(JsValue::from(JsString::from("")))
}
