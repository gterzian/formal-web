use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::dom::Element;

use super::{event_target::register_event_target_methods, node::register_node_methods};

impl Class for Element {
    const NAME: &'static str = "Element";

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
        register_element_methods(class)
    }
}

pub(crate) fn register_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("id"),
            Some(NativeFunction::from_fn_ptr(get_id).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("tagName"),
            Some(NativeFunction::from_fn_ptr(get_tag_name).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("innerHTML"),
            Some(NativeFunction::from_fn_ptr(get_inner_html).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_inner_html).to_js_function(&realm)),
            Attribute::all(),
        )
        .method(
            js_string!("setAttribute"),
            2,
            NativeFunction::from_fn_ptr(set_attribute),
        )
        .method(
            js_string!("getAttribute"),
            1,
            NativeFunction::from_fn_ptr(get_attribute),
        );
    Ok(())
}

fn get_id(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    Ok(JsValue::from(JsString::from(element.id())))
}

fn get_tag_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    Ok(JsValue::from(JsString::from(element.tag_name().as_str())))
}

fn get_inner_html(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    Ok(JsValue::from(JsString::from(element.inner_html().as_str())))
}

fn set_inner_html(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let html = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    element.set_inner_html(&html);
    Ok(JsValue::undefined())
}

fn get_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    Ok(match element.get_attribute(&name) {
        Some(value) => JsValue::from(JsString::from(value.as_str())),
        None => JsValue::null(),
    })
}

fn set_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let value = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    let element = object
        .downcast_ref::<Element>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not an Element"))?;
    element.set_attribute(&name, &value);
    Ok(JsValue::undefined())
}
