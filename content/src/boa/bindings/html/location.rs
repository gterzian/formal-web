use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::builtins::JsArray,
    property::Attribute,
};

use crate::dom::DOMException;
use crate::html::{Location, LocationError};

use super::hyperlink_element_utils::document_creation_url;

/// <https://html.spec.whatwg.org/#entry-settings-object>
struct EntrySettingsObject {
    /// <https://html.spec.whatwg.org/#api-base-url>
    api_base_url: url::Url,
    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    origin: String,
}

impl Class for Location {
    const NAME: &'static str = "Location";

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
        register_location_methods(class)
    }
}

pub(crate) fn register_location_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("href"),
            Some(NativeFunction::from_fn_ptr(get_href).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_href).to_js_function(&realm)),
            Attribute::all(),
        )
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
        )
        .accessor(
            js_string!("ancestorOrigins"),
            Some(NativeFunction::from_fn_ptr(get_ancestor_origins).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(
            js_string!("assign"),
            1,
            NativeFunction::from_fn_ptr(assign_method),
        )
        .method(
            js_string!("replace"),
            1,
            NativeFunction::from_fn_ptr(replace_method),
        )
        .method(
            js_string!("reload"),
            0,
            NativeFunction::from_fn_ptr(reload_method),
        )
        .method(
            js_string!("toString"),
            0,
            NativeFunction::from_fn_ptr(to_string_method),
        );
    Ok(())
}

fn with_location_ref<R>(this: &JsValue, f: impl FnOnce(&Location) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("Location receiver is not an object"))?;
    let location = object
        .downcast_ref::<Location>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not a Location"))?;
    Ok(f(&location))
}

fn map_location_result(result: Result<(), LocationError>, context: &mut Context) -> JsResult<JsValue> {
    result
        .map(|_| JsValue::undefined())
        .map_err(|error| location_error_to_js_error(error, context))
}

fn map_location_value<T>(result: Result<T, LocationError>, context: &mut Context) -> JsResult<T> {
    result.map_err(|error| location_error_to_js_error(error, context))
}

fn location_error_to_js_error(error: LocationError, context: &mut Context) -> JsError {
    match error {
        LocationError::Security => dom_exception_error(DOMException::security_error(), context),
        LocationError::Syntax => dom_exception_error(DOMException::syntax_error(), context),
        LocationError::NotSupported(message) => {
            dom_exception_error(DOMException::new(message, String::from("NotSupportedError")), context)
        }
    }
}

fn dom_exception_error(exception: DOMException, context: &mut Context) -> JsError {
    JsError::from_opaque(JsValue::from(
        DOMException::from_data(exception, context)
            .expect("DOMException construction should not fail"),
    ))
}

fn entry_settings_object(context: &Context) -> JsResult<EntrySettingsObject> {
    let api_base_url = document_creation_url(context)?;
    let origin = api_base_url.origin().unicode_serialization();
    Ok(EntrySettingsObject {
        api_base_url,
        origin,
    })
}

fn get_href(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let href = with_location_ref(this, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, context)?;
    Ok(JsValue::from(JsString::from(href.as_str())))
}

fn set_href(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_href_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, context)
}

fn get_origin(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let origin = with_location_ref(this, |location| location.origin(&entry_settings.origin))?;
    let origin = map_location_value(origin, context)?;
    Ok(JsValue::from(JsString::from(origin.as_str())))
}

fn get_protocol(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let protocol = with_location_ref(this, |location| location.protocol(&entry_settings.origin))?;
    let protocol = map_location_value(protocol, context)?;
    Ok(JsValue::from(JsString::from(protocol.as_str())))
}

fn set_protocol(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_protocol_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_host(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let host = with_location_ref(this, |location| location.host(&entry_settings.origin))?;
    let host = map_location_value(host, context)?;
    Ok(JsValue::from(JsString::from(host.as_str())))
}

fn set_host(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_host_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_hostname(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let hostname = with_location_ref(this, |location| location.hostname(&entry_settings.origin))?;
    let hostname = map_location_value(hostname, context)?;
    Ok(JsValue::from(JsString::from(hostname.as_str())))
}

fn set_hostname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_hostname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_port(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let port = with_location_ref(this, |location| location.port(&entry_settings.origin))?;
    let port = map_location_value(port, context)?;
    Ok(JsValue::from(JsString::from(port.as_str())))
}

fn set_port(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_port_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_pathname(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let pathname = with_location_ref(this, |location| location.pathname(&entry_settings.origin))?;
    let pathname = map_location_value(pathname, context)?;
    Ok(JsValue::from(JsString::from(pathname.as_str())))
}

fn set_pathname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_pathname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_search(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let search = with_location_ref(this, |location| location.search(&entry_settings.origin))?;
    let search = map_location_value(search, context)?;
    Ok(JsValue::from(JsString::from(search.as_str())))
}

fn set_search(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_search_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn get_hash(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let hash = with_location_ref(this, |location| location.hash(&entry_settings.origin))?;
    let hash = map_location_value(hash, context)?;
    Ok(JsValue::from(JsString::from(hash.as_str())))
}

fn set_hash(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.set_hash_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn assign_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.assign_with_origin(&value, &entry_settings.api_base_url, &entry_settings.origin)
    })?;
    map_location_result(result, context)
}

fn replace_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| {
        location.replace_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, context)
}

fn reload_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let result = with_location_ref(this, |location| location.reload_with_origin(&entry_settings.origin))?;
    map_location_result(result, context)
}

fn get_ancestor_origins(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let values = with_location_ref(this, |location| {
        location.ancestor_origins_with_origin(&entry_settings.origin)
    })?;
    let values = map_location_value(values, context)?
        .into_iter()
        .map(|value| JsValue::from(JsString::from(value.as_str())))
        .collect::<Vec<_>>();
    Ok(JsValue::from(JsArray::from_iter(values, context)))
}

fn to_string_method(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(context)?;
    let href = with_location_ref(this, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, context)?;
    Ok(JsValue::from(JsString::from(href.as_str())))
}
