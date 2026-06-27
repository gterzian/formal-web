use std::marker::PhantomData;
use boa_engine::{
    object::builtins::JsArray, Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
};

use crate::dom::DOMException;
use crate::html::{Location, LocationError};

use super::hyperlink_element_utils::document_creation_url;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

/// <https://html.spec.whatwg.org/#entry-settings-object>
struct EntrySettingsObject {
    /// <https://html.spec.whatwg.org/#api-base-url>
    api_base_url: url::Url,
    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    origin: String,
}

use crate::webidl::bindings::{
    create_interface_instance, AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for Location {
    const NAME: &'static str = "Location";

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "href",
            getter: get_href,
            setter: Some(set_href),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "origin",
            getter: get_origin,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "protocol",
            getter: get_protocol,
            setter: Some(set_protocol),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "host",
            getter: get_host,
            setter: Some(set_host),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "hostname",
            getter: get_hostname,
            setter: Some(set_hostname),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "port",
            getter: get_port,
            setter: Some(set_port),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "pathname",
            getter: get_pathname,
            setter: Some(set_pathname),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "search",
            getter: get_search,
            setter: Some(set_search),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "hash",
            getter: get_hash,
            setter: Some(set_hash),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "ancestorOrigins",
            getter: get_ancestor_origins,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "assign",
            length: 1,
            method: assign_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "replace",
            length: 1,
            method: replace_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "reload",
            length: 0,
            method: reload_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "toString",
            length: 0,
            method: to_string_method,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
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

fn map_location_result(
    result: Result<(), LocationError>,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsResult<JsValue> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    result
        .map(|_| JsValue::undefined())
        .map_err(|error| location_error_to_js_error(error, ctx))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn map_location_value<T>(result: Result<T, LocationError>, ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<T, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<T> {
    result.map_err(|error| location_error_to_js_error(error, ctx))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn location_error_to_js_error(error: LocationError, ec: &mut dyn ExecutionContext<BoaTypes>) -> JsError {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    match error {
        LocationError::Security => dom_exception_error(DOMException::security_error(), ctx),
        LocationError::Syntax => dom_exception_error(DOMException::syntax_error(), ctx),
        LocationError::NotSupported(message) => dom_exception_error(
            DOMException::new(message, String::from("NotSupportedError")),
            ctx,
        ),
    }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn dom_exception_error(exception: DOMException, ec: &mut dyn ExecutionContext<BoaTypes>) -> JsError {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    JsError::from_opaque(JsValue::from(
        create_interface_instance::<DOMException>(exception, ctx)
            .expect("DOMException construction should not fail"),
    ))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn entry_settings_object(context: &Context) -> JsResult<EntrySettingsObject> {
    let api_base_url = document_creation_url(context)?;
    let origin = api_base_url.origin().unicode_serialization();
    Ok(EntrySettingsObject {
        api_base_url,
        origin,
    })
}

fn get_href(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let href = with_location_ref(this, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, ctx)?;
    Ok(JsValue::from(JsString::from(href.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_href(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_href_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_origin(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let origin = with_location_ref(this, |location| location.origin(&entry_settings.origin))?;
    let origin = map_location_value(origin, ctx)?;
    Ok(JsValue::from(JsString::from(origin.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_protocol(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let protocol = with_location_ref(this, |location| location.protocol(&entry_settings.origin))?;
    let protocol = map_location_value(protocol, ctx)?;
    Ok(JsValue::from(JsString::from(protocol.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_protocol(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_protocol_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_host(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let host = with_location_ref(this, |location| location.host(&entry_settings.origin))?;
    let host = map_location_value(host, ctx)?;
    Ok(JsValue::from(JsString::from(host.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_host(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_host_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_hostname(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let hostname = with_location_ref(this, |location| location.hostname(&entry_settings.origin))?;
    let hostname = map_location_value(hostname, ctx)?;
    Ok(JsValue::from(JsString::from(hostname.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_hostname(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_hostname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_port(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let port = with_location_ref(this, |location| location.port(&entry_settings.origin))?;
    let port = map_location_value(port, ctx)?;
    Ok(JsValue::from(JsString::from(port.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_port(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_port_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_pathname(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let pathname = with_location_ref(this, |location| location.pathname(&entry_settings.origin))?;
    let pathname = map_location_value(pathname, ctx)?;
    Ok(JsValue::from(JsString::from(pathname.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_pathname(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_pathname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_search(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let search = with_location_ref(this, |location| location.search(&entry_settings.origin))?;
    let search = map_location_value(search, ctx)?;
    Ok(JsValue::from(JsString::from(search.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_search(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_search_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_hash(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let hash = with_location_ref(this, |location| location.hash(&entry_settings.origin))?;
    let hash = map_location_value(hash, ctx)?;
    Ok(JsValue::from(JsString::from(hash.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_hash(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.set_hash_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn assign_method(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.assign_with_origin(&value, &entry_settings.api_base_url, &entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn replace_method(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.replace_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn reload_method(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let result = with_location_ref(this, |location| {
        location.reload_with_origin(&entry_settings.origin)
    })?;
    map_location_result(result, ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_ancestor_origins(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let values = with_location_ref(this, |location| {
        location.ancestor_origins_with_origin(&entry_settings.origin)
    })?;
    let values = map_location_value(values, ctx)?
        .into_iter()
        .map(|value| JsValue::from(JsString::from(value.as_str())))
        .collect::<Vec<_>>();
    Ok(JsValue::from(JsArray::from_iter(values, ctx)))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn to_string_method(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let entry_settings = entry_settings_object(ctx)?;
    let href = with_location_ref(this, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, ctx)?;
    Ok(JsValue::from(JsString::from(href.as_str())))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
