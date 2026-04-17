use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::dom::DOMException;

impl Class for DOMException {
    const NAME: &'static str = "DOMException";
    const LENGTH: usize = 2;

    fn data_constructor(
        _this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let message = args
            .get(0)
            .map(|value| value.to_string(context).map(|value| value.to_std_string_escaped()))
            .transpose()?
            .unwrap_or_default();
        let name = args
            .get(1)
            .map(|value| value.to_string(context).map(|value| value.to_std_string_escaped()))
            .transpose()?
            .unwrap_or_else(|| String::from("Error"));

        Ok(DOMException::new(message, name))
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_dom_exception_methods(class)
    }
}

pub(crate) fn register_dom_exception_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("name"),
            Some(NativeFunction::from_fn_ptr(get_name).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("message"),
            Some(NativeFunction::from_fn_ptr(get_message).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("code"),
            Some(NativeFunction::from_fn_ptr(get_code).to_js_function(&realm)),
            None,
            Attribute::all(),
        );
    Ok(())
}

fn get_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| {
        JsValue::from(JsString::from(exception.name_value()))
    })
}

fn get_message(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| {
        JsValue::from(JsString::from(exception.message_value()))
    })
}

fn get_code(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_dom_exception_ref(this, |exception| JsValue::from(exception.code_value()))
}

fn with_dom_exception_ref<R>(
    this: &JsValue,
    f: impl FnOnce(&DOMException) -> R,
) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("DOMException receiver is not an object"))?;
    let exception = object.downcast_ref::<DOMException>().ok_or_else(|| {
        JsNativeError::typ().with_message("receiver is not a DOMException")
    })?;
    Ok(f(&exception))
}