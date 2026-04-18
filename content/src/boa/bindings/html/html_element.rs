use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::html::HTMLElement;

use crate::boa::bindings::dom::{
    register_element_methods, register_event_target_methods, register_node_methods,
};

impl Class for HTMLElement {
    const NAME: &'static str = "HTMLElement";

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
        register_html_element_methods(class)
    }
}

fn with_html_element_ref<R>(this: &JsValue, f: impl FnOnce(&HTMLElement) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLElement receiver is not an object")
    })?;
    let html_element = object
        .downcast_ref::<HTMLElement>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an HTMLElement"))?;
    Ok(f(&html_element))
}

pub(crate) fn register_html_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("title"),
            Some(NativeFunction::from_fn_ptr(get_title).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_title).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("lang"),
            Some(NativeFunction::from_fn_ptr(get_lang).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_lang).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("dir"),
            Some(NativeFunction::from_fn_ptr(get_dir).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_dir).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("hidden"),
            Some(NativeFunction::from_fn_ptr(get_hidden).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_hidden).to_js_function(&realm)),
            Attribute::all(),
        );
    Ok(())
}

fn get_title(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.title()))
    })
}

fn set_title(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let title = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_title(&title))?;
    Ok(JsValue::undefined())
}

fn get_lang(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.lang()))
    })
}

fn set_lang(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let lang = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_lang(&lang))?;
    Ok(JsValue::undefined())
}

fn get_dir(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.dir()))
    })
}

fn set_dir(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let dir = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_dir(&dir))?;
    Ok(JsValue::undefined())
}

fn get_hidden(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| JsValue::from(html_element.hidden()))
}

fn set_hidden(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let hidden = args.get_or_undefined(0).to_boolean();
    with_html_element_ref(this, |html_element| html_element.set_hidden(hidden))?;
    Ok(JsValue::undefined())
}
