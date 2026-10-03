use crate::html::HTMLIFrameElement;
use crate::html::windowproxy::create_window_proxy;
use crate::js::try_with_event_target_mut;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use crate::webidl::{callback_function_value, nullable_value};

use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for HTMLIFrameElement {
    const NAME: &'static str = "HTMLIFrameElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
        });
    }
}

fn try_with_html_iframe_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
    f: impl FnOnce(&HTMLIFrameElement) -> R,
) -> Completion<R, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLIFrameElement receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(iframe));
        }
    }
    Err(ec.new_type_error("receiver is not an HTMLIFrameElement"))
}

/// Clone the platform object out of its JS wrapper so an operation can hold
/// the reference while borrowing the execution context mutably. The clone
/// shares the `GcCell` handler slots with the registered platform object, so
/// mutations through the clone are visible through the wrapper.
fn clone_html_iframe_element(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<HTMLIFrameElement, Types> {
    let obj = <Types as JsTypes>::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLIFrameElement receiver is not an object"))?;
    ec.with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<HTMLIFrameElement>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not an HTMLIFrameElement"))
}

fn get_src(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let src = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.src())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn get_onload(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let iframe = clone_html_iframe_element(this, ec)?;
    let onload = iframe.onload_value(ec);
    Ok(onload
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onload(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let callback = nullable_value(
        args.get(0).unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;

    let iframe = clone_html_iframe_element(this, ec)?;
    let previous = iframe.replace_onload(callback.clone(), ec);

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.remove_event_listener_entry("load", &previous, false, ec);
        })?;
    }

    if let Some(callback) = callback {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.add_event_listener(
                target.clone(),
                String::from("load"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
                ec,
            );
        })?;
    }

    Ok(ec.value_undefined())
}

fn get_onerror(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let iframe = clone_html_iframe_element(this, ec)?;
    let onerror = iframe.onerror_value(ec);
    Ok(onerror
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(|| ec.value_null()))
}

fn set_onerror(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let callback = nullable_value(
        args.get(0).unwrap_or(&ec.value_undefined()),
        ec,
        callback_function_value,
    )?;

    let iframe = clone_html_iframe_element(this, ec)?;
    let previous = iframe.replace_onerror(callback.clone(), ec);

    if let Some(previous) = previous {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.remove_event_listener_entry("error", &previous, false, ec);
        })?;
    }

    if let Some(callback) = callback {
        try_with_event_target_mut(this, ec, |target, ec| {
            target.add_event_listener(
                target.clone(),
                String::from("error"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
                ec,
            );
        })?;
    }

    Ok(ec.value_undefined())
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let src = ec.to_rust_string(args.get(0).cloned().unwrap_or(undefined))?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_src(&src))?;
    Ok(ec.value_undefined())
}

fn get_srcdoc(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let srcdoc = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.srcdoc())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&srcdoc)))
}

fn set_srcdoc(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let srcdoc = ec.to_rust_string(args.get(0).cloned().unwrap_or(undefined))?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_srcdoc(&srcdoc))?;
    Ok(ec.value_undefined())
}

fn get_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let name = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.name())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&name)))
}

fn set_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.get(0).cloned().unwrap_or(undefined))?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_name(&name))?;
    Ok(ec.value_undefined())
}

fn get_width(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let width = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.width())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&width)))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let width = ec.to_rust_string(args.get(0).cloned().unwrap_or(undefined))?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_width(&width))?;
    Ok(ec.value_undefined())
}

fn get_height(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let height = try_with_html_iframe_element_ref(this, ec, |iframe| iframe.height())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&height)))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let height = ec.to_rust_string(args.get(0).cloned().unwrap_or(undefined))?;
    try_with_html_iframe_element_ref(this, ec, |iframe| iframe.set_height(&height))?;
    Ok(ec.value_undefined())
}

fn get_content_document(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let _ = try_with_html_iframe_element_ref(this, ec, |_iframe| ())?;
    Ok(ec.value_null())
}

fn get_content_window(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let node_id = try_with_html_iframe_element_ref(this, ec, |iframe| {
        iframe.html_element.element.node.node_id
    })?;
    // <https://html.spec.whatwg.org/#dom-iframe-contentwindow>
    // Resolve the content navigable from the realm's registry and hand out
    // its WindowProxy (created in this realm, cached per navigable).  The
    // cached WindowProxy for the navigable already carries the child
    // document's Window when it lives in this content process, so the
    // WindowProxy is locally backed.
    let navigable_id = crate::js::platform_objects::with_global_scope(ec, |global_scope, ec| {
        Ok(global_scope.content_navigable_for_iframe(node_id, ec))
    })?;
    let Some(navigable_id) = navigable_id else {
        return Ok(ec.value_null());
    };
    create_window_proxy(navigable_id, None, ec)
}
