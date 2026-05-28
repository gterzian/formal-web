use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::{JsObject, ObjectInitializer},
    property::Attribute,
};

use crate::boa::with_event_target_mut;
use crate::html::{Window, WindowOrWorkerGlobalScope};
use crate::webidl::{callback_function_value, nullable_value};

use crate::boa::bindings::dom::{register_event_target_methods, with_element_ref};

use super::{computed_style_object_for_element, hyperlink_element_utils::document_creation_url};

impl Class for Window {
    const NAME: &'static str = "Window";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)?;
        register_window_methods(class)
    }
}

pub(crate) fn register_window_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("onload"),
            Some(NativeFunction::from_fn_ptr(get_onload).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_onload).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("parent"),
            Some(NativeFunction::from_fn_ptr(get_parent).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("top"),
            Some(NativeFunction::from_fn_ptr(get_top).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("location"),
            Some(NativeFunction::from_fn_ptr(get_location).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(
            js_string!("requestAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(request_animation_frame_method),
        )
        .method(
            js_string!("cancelAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(cancel_animation_frame_method),
        )
        .method(
            js_string!("setTimeout"),
            1,
            NativeFunction::from_fn_ptr(set_timeout_method),
        )
        .method(
            js_string!("clearTimeout"),
            1,
            NativeFunction::from_fn_ptr(clear_timeout_method),
        )
        .method(
            js_string!("setInterval"),
            1,
            NativeFunction::from_fn_ptr(set_interval_method),
        )
        .method(
            js_string!("clearInterval"),
            1,
            NativeFunction::from_fn_ptr(clear_interval_method),
        )
        .method(
            js_string!("getComputedStyle"),
            1,
            NativeFunction::from_fn_ptr(get_computed_style_method),
        );
    Ok(())
}

fn request_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let callback = callback_function_value(args.get_or_undefined(0))?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(
        window.global_scope.request_animation_frame(callback),
    ))
}

fn get_onload(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(window
        .onload_value()
        .map(|callback| callback.to_js_value())
        .unwrap_or_else(JsValue::null))
}

fn set_onload(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let callback = nullable_value(args.get_or_undefined(0), callback_function_value)?;
    let previous = with_window_mut(&window_object, |window| window.replace_onload(callback.clone()))?;

    if let Some(previous) = previous {
        let receiver = JsValue::from(window_object.clone());
        with_event_target_mut(&receiver, |target| {
            target.remove_event_listener_entry("load", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        let receiver = JsValue::from(window_object.clone());
        with_event_target_mut(&receiver, |target| {
            target.add_event_listener(
                &window_object,
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

fn get_parent(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(current_window_object(this, context)))
}

fn get_top(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(current_window_object(this, context)))
}

fn get_location(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(location_object(context)?))
}

fn cancel_animation_frame_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let handle = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.global_scope.cancel_animation_frame(handle);
    Ok(JsValue::undefined())
}

fn set_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_timeout(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_timeout_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_timeout(timer_id);
    Ok(JsValue::undefined())
}

fn set_interval_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    Ok(JsValue::from(window.set_interval(
        args.get_or_undefined(0),
        args.get_or_undefined(1),
        args.iter().skip(2).cloned().collect(),
        context,
    )?))
}

fn clear_interval_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let timer_id = args.get_or_undefined(0).to_u32(context)?;
    let window_object = current_window_object(this, context);
    let window = downcast_window(&window_object)?;
    window.clear_interval(timer_id);
    Ok(JsValue::undefined())
}

fn get_computed_style_method(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    with_element_ref(args.get_or_undefined(0), |element| {
        computed_style_object_for_element(element, context).map(JsValue::from)
    })?
}

fn location_object(context: &mut Context) -> JsResult<JsObject> {
    let url = document_creation_url(context)?;
    let href = url.as_str().to_owned();
    let protocol = format!("{}:", url.scheme());
    let hostname = url.host_str().unwrap_or_default().to_owned();
    let host = match url.port() {
        Some(port) if !hostname.is_empty() => format!("{hostname}:{port}"),
        _ => hostname.clone(),
    };
    let port = url.port().map(|value| value.to_string()).unwrap_or_default();
    let pathname = url.path().to_owned();
    let search = url
        .query()
        .map(|value| format!("?{value}"))
        .unwrap_or_default();
    let hash = url
        .fragment()
        .map(|value| format!("#{value}"))
        .unwrap_or_default();

    let mut initializer = ObjectInitializer::new(context);
    initializer.property(js_string!("href"), JsString::from(href.as_str()), Attribute::all());
    initializer.property(
        js_string!("origin"),
        JsString::from(url.origin().unicode_serialization()),
        Attribute::all(),
    );
    initializer.property(
        js_string!("protocol"),
        JsString::from(protocol.as_str()),
        Attribute::all(),
    );
    initializer.property(
        js_string!("host"),
        JsString::from(host.as_str()),
        Attribute::all(),
    );
    initializer.property(
        js_string!("hostname"),
        JsString::from(hostname.as_str()),
        Attribute::all(),
    );
    initializer.property(js_string!("port"), JsString::from(port.as_str()), Attribute::all());
    initializer.property(
        js_string!("pathname"),
        JsString::from(pathname.as_str()),
        Attribute::all(),
    );
    initializer.property(
        js_string!("search"),
        JsString::from(search.as_str()),
        Attribute::all(),
    );
    initializer.property(js_string!("hash"), JsString::from(hash.as_str()), Attribute::all());
    initializer.function(
        NativeFunction::from_fn_ptr(location_to_string_method),
        js_string!("toString"),
        0,
    );
    Ok(initializer.build())
}

fn location_to_string_method(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let Some(object) = this.as_object() else {
        return Ok(JsValue::from(JsString::from("")));
    };
    let href = object.get(js_string!("href"), context)?;
    if href.is_undefined() {
        return Ok(JsValue::from(JsString::from("")));
    }
    Ok(href)
}

fn current_window_object(this: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = this.as_object() {
        return object.clone();
    }

    context.global_object()
}

fn downcast_window(object: &JsObject) -> JsResult<boa_gc::GcRef<'_, Window>> {
    object.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("receiver is not a Window")
            .into()
    })
}

fn with_window_mut<R>(
    object: &JsObject,
    f: impl FnOnce(&mut Window) -> R,
) -> JsResult<R> {
    let Some(mut window) = object.downcast_mut::<Window>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not a Window")
            .into());
    };
    Ok(f(&mut window))
}
