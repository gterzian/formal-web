use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::html::HTMLIFrameElement;

use crate::boa::bindings::dom::{
    register_element_methods, register_event_target_methods, register_node_methods,
};

use super::html_element::register_html_element_methods;

impl Class for HTMLIFrameElement {
    const NAME: &'static str = "HTMLIFrameElement";

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
        register_node_methods(class)?;
        register_element_methods(class)?;
        register_html_element_methods(class)?;
        register_html_iframe_element_methods(class)
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

fn register_html_iframe_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("src"),
            Some(NativeFunction::from_fn_ptr(get_src).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_src).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("srcdoc"),
            Some(NativeFunction::from_fn_ptr(get_srcdoc).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_srcdoc).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("name"),
            Some(NativeFunction::from_fn_ptr(get_name).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_name).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("width"),
            Some(NativeFunction::from_fn_ptr(get_width).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_width).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("height"),
            Some(NativeFunction::from_fn_ptr(get_height).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_height).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("contentDocument"),
            Some(NativeFunction::from_fn_ptr(get_content_document).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("contentWindow"),
            Some(NativeFunction::from_fn_ptr(get_content_window).to_js_function(&realm)),
            None,
            Attribute::all(),
        );
    Ok(())
}

fn get_src(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_iframe_element_ref(this, |iframe| {
        JsValue::from(JsString::from(iframe.src()))
    })
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
    with_html_iframe_element_ref(this, |iframe| {
        JsValue::from(JsString::from(iframe.name()))
    })
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
    with_html_iframe_element_ref(this, |iframe| {
        JsValue::from(JsString::from(iframe.width()))
    })
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
