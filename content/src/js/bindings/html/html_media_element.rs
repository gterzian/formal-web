//
// Note: These bindings define *which members* HTMLMediaElement exposes.
// Domain methods on HTMLMediaElement implement *what those members do*.
//
// Only a subset of the full IDL is exposed for the initial video cut.

#![cfg_attr(not(feature = "media"), allow(dead_code, unused_imports))]

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLMediaElement;
use crate::html::HTMLVideoElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

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
        #[cfg(feature = "media")]
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // Note: Members are always defined (even without the `media` feature)
        // so that the prototype has entries for e.g. `play`, `src`. When media
        // is disabled the method bodies are no-ops returning sensible defaults.

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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
        });
        // Operations
        def.add_operation(OperationDef {
            id: "load",
            length: 0,
            method: load_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "play",
            length: 0,
            method: play_method,
            static_: false,
            unforgeable: false,
            promise_type: true,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "pause",
            length: 0,
            method: pause_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "canPlayType",
            length: 1,
            method: can_play_type,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
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
            exposed: None,
        });
    }
}

// Tries HTMLMediaElement first, then HTMLVideoElement → .media_element.

fn try_with_media_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLMediaElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
            return Ok(f(media));
        }
        if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            return Ok(f(&video.media_element));
        }
    }
    Err(ec.new_type_error("expected HTMLMediaElement"))
}

fn get_network_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let state = try_with_media_ref(this, ec, |media| media.network_state())?;
    Ok(ec.value_from_number(state as f64))
}

fn get_ready_state(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let state = try_with_media_ref(this, ec, |media| media.ready_state())?;
    Ok(ec.value_from_number(state as f64))
}

fn get_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let src = try_with_media_ref(this, ec, |media| media.src())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let src = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined))?;
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    ec.with_object_any_mut_with(
        &obj,
        Box::new(|data, ec2| {
            if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                media.set_src(&src, ec2);
            } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                video.media_element.set_src(&src, ec2);
            }
        }),
    );
    Ok(ec.value_undefined())
}

fn get_current_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let src = try_with_media_ref(this, ec, |media| media.current_src())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn get_duration(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let duration = try_with_media_ref(this, ec, |media| media.duration())?;
    Ok(ec.value_from_number(duration))
}

fn get_paused(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let paused = try_with_media_ref(this, ec, |media| media.paused())?;
    Ok(ec.value_from_bool(paused))
}

fn get_seeking(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let seeking = try_with_media_ref(this, ec, |media| media.seeking())?;
    Ok(ec.value_from_bool(seeking))
}

fn get_current_time(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let time = try_with_media_ref(this, ec, |media| media.current_time())?;
    Ok(ec.value_from_number(time))
}

fn set_current_time(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _ = try_with_media_ref(this, ec, |_media| ())?;
    // TODO: Implement using interior mutability.
    Ok(ec.value_undefined())
}

fn get_error(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    use crate::js::Types;
    let media_error = try_with_media_ref(this, ec, |media| media.error())?;
    match media_error {
        Some(err) => {
            // Note: Returns a plain JS object rather than a MediaError platform
            // object.  MediaError needs its own WebIdlInterface impl for full
            // spec compliance.
            let obj = ec.create_plain_object(None);
            let code_key = ec.property_key_from_str("code");
            let code_desc = js_engine::PropertyDescriptor {
                value: Some(ec.value_from_number(err.code as f64)),
                writable: Some(false),
                enumerable: Some(true),
                configurable: Some(true),
                get: None,
                set: None,
            };
            ec.define_property_or_throw(obj.clone(), code_key, code_desc)?;
            let message_key = ec.property_key_from_str("message");
            let message_desc = js_engine::PropertyDescriptor {
                value: Some(ec.value_from_string(ec.js_string_from_str(&err.message))),
                writable: Some(false),
                enumerable: Some(true),
                configurable: Some(true),
                get: None,
                set: None,
            };
            ec.define_property_or_throw(obj.clone(), message_key, message_desc)?;
            Ok(Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_autoplay(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = try_with_media_ref(this, ec, |media| media.autoplay())?;
    Ok(ec.value_from_bool(val))
}

fn set_autoplay(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_media_ref(this, ec, |media| media.set_autoplay(value))?;
    Ok(ec.value_undefined())
}

fn get_loop(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = try_with_media_ref(this, ec, |media| media.loop_())?;
    Ok(ec.value_from_bool(val))
}

fn set_loop(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_media_ref(this, ec, |media| media.set_loop(value))?;
    Ok(ec.value_undefined())
}

fn get_controls(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = try_with_media_ref(this, ec, |media| media.controls())?;
    Ok(ec.value_from_bool(val))
}

fn set_controls(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_media_ref(this, ec, |media| media.set_controls(value))?;
    Ok(ec.value_undefined())
}

fn get_muted(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let val = try_with_media_ref(this, ec, |media| media.muted())?;
    Ok(ec.value_from_bool(val))
}

fn set_muted(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_media_ref(this, ec, |media| media.set_muted(value))?;
    Ok(ec.value_undefined())
}

fn get_volume(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let vol = try_with_media_ref(this, ec, |media| media.volume())?;
    Ok(ec.value_from_number(vol))
}

fn set_volume(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let vol = args
        .first()
        .and_then(|v| crate::js::Types::value_as_number(v))
        .unwrap_or(1.0);
    try_with_media_ref(this, ec, |media| media.set_volume(vol))?;
    Ok(ec.value_undefined())
}

fn get_preload(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let preload = try_with_media_ref(this, ec, |media| media.preload())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&preload)))
}

fn set_preload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or_else(|| undefined))?;
    try_with_media_ref(this, ec, |media| media.set_preload(&value))?;
    Ok(ec.value_undefined())
}

fn load_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _ = try_with_media_ref(this, ec, |_media| ())?;
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
    let mut result = Err(ec.new_type_error("expected HTMLMediaElement"));
    ec.with_object_any_mut_with(
        &obj,
        Box::new(|data, ec2| {
            if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                result = media.play(ec2);
            } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                result = video.media_element.play(ec2);
            }
        }),
    );
    result
}

fn pause_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    ec.with_object_any_mut_with(
        &obj,
        Box::new(|data, ec2| {
            if let Some(media) = data.downcast_mut::<HTMLMediaElement>() {
                media.pause(ec2);
            } else if let Some(video) = data.downcast_mut::<HTMLVideoElement>() {
                video.media_element.pause(ec2);
            }
        }),
    );
    Ok(ec.value_undefined())
}

fn can_play_type(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _ = try_with_media_ref(this, ec, |_media| ())?;
    // Step 1: Return "probably" if the type is a media type that can be rendered.
    // Initial cut: return empty string (no types supported).

    Ok(ec.value_from_string(ec.js_string_from_str("")))
}
