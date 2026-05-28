use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::builtins::JsArray,
    property::Attribute,
};

use crate::html::Location;

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

fn map_location_result(result: Result<(), String>) -> JsResult<JsValue> {
    result
        .map(|_| JsValue::undefined())
        .map_err(|error| JsNativeError::typ().with_message(error).into())
}

fn get_href(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.href().as_str()))
    })
}

fn set_href(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_href(&value))?;
    map_location_result(result)
}

fn get_origin(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.origin().as_str()))
    })
}

fn get_protocol(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.protocol().as_str()))
    })
}

fn set_protocol(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_protocol(&value))?;
    map_location_result(result)
}

fn get_host(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.host().as_str()))
    })
}

fn set_host(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_host(&value))?;
    map_location_result(result)
}

fn get_hostname(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.hostname().as_str()))
    })
}

fn set_hostname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_hostname(&value))?;
    map_location_result(result)
}

fn get_port(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.port().as_str()))
    })
}

fn set_port(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_port(&value))?;
    map_location_result(result)
}

fn get_pathname(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.pathname().as_str()))
    })
}

fn set_pathname(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_pathname(&value))?;
    map_location_result(result)
}

fn get_search(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.search().as_str()))
    })
}

fn set_search(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_search(&value))?;
    map_location_result(result)
}

fn get_hash(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.hash().as_str()))
    })
}

fn set_hash(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.set_hash(&value))?;
    map_location_result(result)
}

fn assign_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.assign(&value))?;
    map_location_result(result)
}

fn replace_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let result = with_location_ref(this, |location| location.replace(&value))?;
    map_location_result(result)
}

fn reload_method(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let result = with_location_ref(this, Location::reload)?;
    map_location_result(result)
}

fn get_ancestor_origins(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let values = with_location_ref(this, Location::ancestor_origins)?
        .into_iter()
        .map(|value| JsValue::from(JsString::from(value.as_str())))
        .collect::<Vec<_>>();
    Ok(JsValue::from(JsArray::from_iter(values, context)))
}

fn to_string_method(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_location_ref(this, |location| {
        JsValue::from(JsString::from(location.href().as_str()))
    })
}
