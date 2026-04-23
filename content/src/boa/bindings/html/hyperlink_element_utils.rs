use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue, class::ClassBuilder, js_string,
    native_function::NativeFunction, property::Attribute,
};
use url::Url;

use crate::boa::platform_objects;
use crate::{
    dom::Document,
    html::{HTMLAnchorElement, HyperlinkElementUtils},
};

pub(crate) fn document_creation_url(context: &Context) -> JsResult<Url> {
    let object = platform_objects::document_object(context)?;
    let document = object
        .downcast_ref::<Document>()
        .ok_or_else(|| JsNativeError::typ().with_message("document object is not a Document"))?;
    Ok(document.creation_url.clone())
}

fn with_hyperlink_element_utils_ref<R>(
    this: &JsValue,
    f: impl FnOnce(&dyn HyperlinkElementUtils) -> R,
) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("hyperlink receiver is not an object"))?;
    if let Some(anchor) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&*anchor));
    }
    Err(JsNativeError::typ()
        .with_message("receiver does not implement HyperlinkElementUtils")
        .into())
}

pub(crate) fn register_hyperlink_element_utils_methods(
    class: &mut ClassBuilder<'_>,
) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("origin"),
            Some(NativeFunction::from_fn_ptr(get_origin).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("protocol"),
            Some(NativeFunction::from_fn_ptr(get_protocol).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_protocol).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("username"),
            Some(NativeFunction::from_fn_ptr(get_username).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_username).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("password"),
            Some(NativeFunction::from_fn_ptr(get_password).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_password).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("host"),
            Some(NativeFunction::from_fn_ptr(get_host).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_host).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("hostname"),
            Some(NativeFunction::from_fn_ptr(get_hostname).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_hostname).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("port"),
            Some(NativeFunction::from_fn_ptr(get_port).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_port).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("pathname"),
            Some(NativeFunction::from_fn_ptr(get_pathname).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_pathname).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("search"),
            Some(NativeFunction::from_fn_ptr(get_search).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_search).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("hash"),
            Some(NativeFunction::from_fn_ptr(get_hash).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_hash).to_js_function(&realm)),
            Attribute::all(),
        );
    Ok(())
}

fn get_origin(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.origin(&document_creation_url)))
    })
}

fn get_protocol(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.protocol(&document_creation_url)))
    })
}

fn set_protocol(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_protocol(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_username(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.username(&document_creation_url)))
    })
}

fn set_username(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_username(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_password(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.password(&document_creation_url)))
    })
}

fn set_password(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_password(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_host(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.host(&document_creation_url)))
    })
}

fn set_host(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_host(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_hostname(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.hostname(&document_creation_url)))
    })
}

fn set_hostname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_hostname(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_port(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.port(&document_creation_url)))
    })
}

fn set_port(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_port(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_pathname(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.pathname(&document_creation_url)))
    })
}

fn set_pathname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_pathname(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_search(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.search(&document_creation_url)))
    })
}

fn set_search(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_search(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}

fn get_hash(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        JsValue::from(JsString::from(hyperlink.hash(&document_creation_url)))
    })
}

fn set_hash(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let document_creation_url = document_creation_url(context)?;
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_hyperlink_element_utils_ref(this, |hyperlink| {
        hyperlink.set_hash(&document_creation_url, &value)
    })?;
    Ok(JsValue::undefined())
}
