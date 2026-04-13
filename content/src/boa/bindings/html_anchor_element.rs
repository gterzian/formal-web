use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::html::HTMLAnchorElement;

use super::{
    element::register_element_methods, event_target::register_event_target_methods,
    html_element::register_html_element_methods,
    hyperlink_element_utils::{document_creation_url, register_hyperlink_element_utils_methods},
    node::register_node_methods,
};

impl Class for HTMLAnchorElement {
    const NAME: &'static str = "HTMLAnchorElement";

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
        register_hyperlink_element_utils_methods(class)?;
        register_html_anchor_element_methods(class)
    }
}

fn with_html_anchor_element_ref<R>(
    this: &JsValue,
    f: impl FnOnce(&HTMLAnchorElement) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLAnchorElement receiver is not an object")
    })?;
    let html_anchor_element = object
        .downcast_ref::<HTMLAnchorElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an HTMLAnchorElement"))?;
    Ok(f(&html_anchor_element))
}

fn register_html_anchor_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("href"),
            Some(NativeFunction::from_fn_ptr(get_href).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_href).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("target"),
            Some(NativeFunction::from_fn_ptr(get_target).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_target).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("download"),
            Some(NativeFunction::from_fn_ptr(get_download).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_download).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("rel"),
            Some(NativeFunction::from_fn_ptr(get_rel).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_rel).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("referrerPolicy"),
            Some(NativeFunction::from_fn_ptr(get_referrer_policy).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_referrer_policy).to_js_function(&realm)),
            Attribute::all(),
        );
    Ok(())
}

fn get_href(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let base_url = document_creation_url(context)?;
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.href(&base_url)))
    })
}

fn set_href(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let href = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_href(&href))?;
    Ok(JsValue::undefined())
}

fn get_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.target()))
    })
}

fn set_target(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let target = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_target(&target))?;
    Ok(JsValue::undefined())
}

fn get_download(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.download()))
    })
}

fn set_download(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let download = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_download(&download))?;
    Ok(JsValue::undefined())
}

fn get_rel(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.rel()))
    })
}

fn set_rel(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let rel = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| anchor.set_rel(&rel))?;
    Ok(JsValue::undefined())
}

fn get_referrer_policy(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_anchor_element_ref(this, |anchor| {
        JsValue::from(JsString::from(anchor.referrer_policy()))
    })
}

fn set_referrer_policy(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let referrer_policy = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_anchor_element_ref(this, |anchor| {
        anchor.set_referrer_policy(&referrer_policy)
    })?;
    Ok(JsValue::undefined())
}
