use boa_engine::{JsArgs, JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::html::HTMLIFrameElement;
use crate::js::downcast::try_with_event_target_mut;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use crate::webidl::{callback_function_value_ec, nullable_value_ec};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for HTMLIFrameElement {
    const NAME: &'static str = "HTMLIFrameElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            _phantom: PhantomData,

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
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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

fn try_with_html_iframe_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLIFrameElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLIFrameElement receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(iframe));
        }
    }
    Err(ec.new_type_error("receiver is not an HTMLIFrameElement"))
}

fn get_src(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let src = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.src())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn get_onload(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let onload = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.onload_value())?;
    Ok(onload
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let iframe_object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLIFrameElement receiver is not an object"))?;
    let callback = nullable_value_ec(
        args.get_or_undefined(0),
        ec,
        callback_function_value_ec,
    )?;

    // Note: uses JsObject::downcast_mut — with_object_any_mut borrows ec.
    let previous = if let Some(mut iframe) = iframe_object.downcast_mut::<HTMLIFrameElement>() {
        iframe.replace_onload(callback.clone())
    } else {
        return Err(ec.new_type_error("receiver is not an HTMLIFrameElement"));
    };

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target| {
            target.remove_event_listener_entry("load", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        // Note: keeps ec_to_ctx — add_event_listener returns JsResult.
        let add_result = try_with_event_target_mut(this, ec, |target| {
            target.add_event_listener(
                &iframe_object,
                String::from("load"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            )
        })?;
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let undefined = JsValue::undefined();
        add_result.map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?;
    }

    Ok(ec.value_undefined())
}

fn get_onerror(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let onerror = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.onerror_value())?;
    Ok(onerror
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onerror(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let iframe_object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLIFrameElement receiver is not an object"))?;
    let callback = nullable_value_ec(
        args.get_or_undefined(0),
        ec,
        callback_function_value_ec,
    )?;

    // Note: uses JsObject::downcast_mut — with_object_any_mut borrows ec.
    let previous = if let Some(mut iframe) = iframe_object.downcast_mut::<HTMLIFrameElement>() {
        iframe.replace_onerror(callback.clone())
    } else {
        return Err(ec.new_type_error("receiver is not an HTMLIFrameElement"));
    };

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target| {
            target.remove_event_listener_entry("error", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        // Note: keeps ec_to_ctx — add_event_listener returns JsResult.
        let add_result = try_with_event_target_mut(this, ec, |target| {
            target.add_event_listener(
                &iframe_object,
                String::from("error"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            )
        })?;
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        let undefined = JsValue::undefined();
        add_result.map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?;
    }

    Ok(ec.value_undefined())
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let src = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_src(&src))?;
    Ok(ec.value_undefined())
}

fn get_srcdoc(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let srcdoc = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.srcdoc())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&srcdoc)))
}

fn set_srcdoc(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let srcdoc = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_srcdoc(&srcdoc))?;
    Ok(ec.value_undefined())
}

fn get_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let name = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.name())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&name)))
}

fn set_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let name = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_name(&name))?;
    Ok(ec.value_undefined())
}

fn get_width(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let width = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.width())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&width)))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let width = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_width(&width))?;
    Ok(ec.value_undefined())
}

fn get_height(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let height = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.height())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&height)))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let height = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_height(&height))?;
    Ok(ec.value_undefined())
}

fn get_content_document(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _ = try_with_html_iframe_element_ref(this, ec, |_iframe| ())?;
    Ok(ec.value_null())
}

fn get_content_window(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let _ = try_with_html_iframe_element_ref(this, ec, |_iframe| ())?;
    Ok(ec.value_null())
}
