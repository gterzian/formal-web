use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
};

use crate::js::with_event_target_mut;
use crate::html::HTMLIFrameElement;
use crate::webidl::{callback_function_value, nullable_value};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, WebIdlInterface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for HTMLIFrameElement {
    const NAME: &'static str = "HTMLIFrameElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition) {
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
            id: "srcdoc",
            getter: get_srcdoc,
            setter: Some(set_srcdoc),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "name",
            getter: get_name,
            setter: Some(set_name),
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
        def.add_attribute(AttributeDef {
            id: "contentDocument",
            getter: get_content_document,
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
            id: "contentWindow",
            getter: get_content_window,
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
            id: "onload",
            getter: get_onload,
            setter: Some(set_onload),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "onerror",
            getter: get_onerror,
            setter: Some(set_onerror),
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

fn with_html_iframe_element_ref<R>(
    this: &JsValue,
    f: impl FnOnce(&HTMLIFrameElement) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLIFrameElement receiver is not an object")
    })?;
    let html_iframe_element = object
        .downcast_ref::<HTMLIFrameElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an HTMLIFrameElement"))?;
    Ok(f(&html_iframe_element))
}

fn with_html_iframe_element_mut<R>(
    this: &JsValue,
    f: impl FnOnce(&mut HTMLIFrameElement) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLIFrameElement receiver is not an object")
    })?;
    let mut html_iframe_element = object
        .downcast_mut::<HTMLIFrameElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an HTMLIFrameElement"))?;
    Ok(f(&mut html_iframe_element))
}
fn get_src(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| JsValue::from(JsString::from(iframe.src())))
}

fn get_onload(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| {
        iframe
            .onload_value()
            .map(|callback| callback.to_js_value())
            .unwrap_or_else(JsValue::null)
    })
}

fn set_onload(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let iframe_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLIFrameElement receiver is not an object")
    })?;
    let callback = nullable_value(args.get_or_undefined(0), callback_function_value)?;
    let previous =
        with_html_iframe_element_mut(this, |iframe| iframe.replace_onload(callback.clone()))?;

    if let Some(previous) = previous {
        with_event_target_mut(this, |target| {
            target.remove_event_listener_entry("load", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        with_event_target_mut(this, |target| {
            target.add_event_listener(
                &iframe_object,
                String::from("load"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            )
        })??;
    }

    Ok(JsValue::undefined())
}

fn get_onerror(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| {
        iframe
            .onerror_value()
            .map(|callback| callback.to_js_value())
            .unwrap_or_else(JsValue::null)
    })
}

fn set_onerror(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let iframe_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLIFrameElement receiver is not an object")
    })?;
    let callback = nullable_value(args.get_or_undefined(0), callback_function_value)?;
    let previous =
        with_html_iframe_element_mut(this, |iframe| iframe.replace_onerror(callback.clone()))?;

    if let Some(previous) = previous {
        with_event_target_mut(this, |target| {
            target.remove_event_listener_entry("error", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        with_event_target_mut(this, |target| {
            target.add_event_listener(
                &iframe_object,
                String::from("error"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            )
        })??;
    }

    Ok(JsValue::undefined())
}

fn set_src(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let src = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_iframe_element_ref(this, |iframe| iframe.set_src(&src))?;
    Ok(JsValue::undefined())
}

fn get_srcdoc(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| {
        JsValue::from(JsString::from(iframe.srcdoc()))
    })
}

fn set_srcdoc(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let srcdoc = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_iframe_element_ref(this, |iframe| iframe.set_srcdoc(&srcdoc))?;
    Ok(JsValue::undefined())
}

fn get_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| JsValue::from(JsString::from(iframe.name())))
}

fn set_name(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_iframe_element_ref(this, |iframe| iframe.set_name(&name))?;
    Ok(JsValue::undefined())
}

fn get_width(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| JsValue::from(JsString::from(iframe.width())))
}

fn set_width(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let width = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_iframe_element_ref(this, |iframe| iframe.set_width(&width))?;
    Ok(JsValue::undefined())
}

fn get_height(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| {
        JsValue::from(JsString::from(iframe.height()))
    })
}

fn set_height(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let height = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_iframe_element_ref(this, |iframe| iframe.set_height(&height))?;
    Ok(JsValue::undefined())
}

fn get_content_document(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |_iframe| JsValue::null())
}

fn get_content_window(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |_iframe| JsValue::null())
}
