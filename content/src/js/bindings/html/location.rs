use std::marker::PhantomData;

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::dom::DOMException;
use crate::html::{Location, LocationError};

use super::hyperlink_element_utils::document_creation_url;

use js_engine::{Completion, ExecutionContext, JsTypes};

/// <https://html.spec.whatwg.org/#entry-settings-object>
struct EntrySettingsObject {
    /// <https://html.spec.whatwg.org/#api-base-url>
    api_base_url: url::Url,
    /// <https://html.spec.whatwg.org/#concept-settings-object-origin>
    origin: String,
}

use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for Location {
    const NAME: &'static str = "Location";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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

// ── Generic helpers ──

fn try_with_location_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Location) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("Location receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(location) = data.downcast_ref::<Location>() {
            return Ok(f(location));
        }
    }
    Err(ec.new_type_error("receiver is not a Location"))
}

fn location_error_to_js_value(
    error: LocationError,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsValue {
    let exception = match error {
        LocationError::Security => DOMException::security_error(),
        LocationError::Syntax => DOMException::syntax_error(),
        LocationError::NotSupported(message) => {
            DOMException::new(message, String::from("NotSupportedError"))
        }
    };
    create_interface_instance::<crate::js::Types, DOMException>(exception, ec)
        .map(|obj| crate::js::Types::value_from_object(obj))
        .unwrap_or_else(|err| err)
}

fn map_location_result(
    result: Result<(), LocationError>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match result {
        Ok(()) => Ok(ec.value_undefined()),
        Err(error) => Err(location_error_to_js_value(error, ec)),
    }
}

fn map_location_value<T>(
    result: Result<T, LocationError>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<T, crate::js::Types> {
    result.map_err(|error| location_error_to_js_value(error, ec))
}

fn entry_settings_object(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<EntrySettingsObject, crate::js::Types> {
    let api_base_url = document_creation_url(ec)?;
    let origin = api_base_url.origin().unicode_serialization();
    Ok(EntrySettingsObject {
        api_base_url,
        origin,
    })
}

// ── Getters ──

fn get_href(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let href = try_with_location_ref(this, ec, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(href.as_str())))
}

fn get_origin(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let origin =
        try_with_location_ref(this, ec, |location| location.origin(&entry_settings.origin))?;
    let origin = map_location_value(origin, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(origin.as_str())))
}

fn get_protocol(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let protocol = try_with_location_ref(this, ec, |location| {
        location.protocol(&entry_settings.origin)
    })?;
    let protocol = map_location_value(protocol, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(protocol.as_str())))
}

fn get_host(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let host = try_with_location_ref(this, ec, |location| location.host(&entry_settings.origin))?;
    let host = map_location_value(host, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(host.as_str())))
}

fn get_hostname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let hostname = try_with_location_ref(this, ec, |location| {
        location.hostname(&entry_settings.origin)
    })?;
    let hostname = map_location_value(hostname, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(hostname.as_str())))
}

fn get_port(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let port = try_with_location_ref(this, ec, |location| location.port(&entry_settings.origin))?;
    let port = map_location_value(port, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(port.as_str())))
}

fn get_pathname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let pathname = try_with_location_ref(this, ec, |location| {
        location.pathname(&entry_settings.origin)
    })?;
    let pathname = map_location_value(pathname, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(pathname.as_str())))
}

fn get_search(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let search =
        try_with_location_ref(this, ec, |location| location.search(&entry_settings.origin))?;
    let search = map_location_value(search, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(search.as_str())))
}

fn get_hash(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let hash = try_with_location_ref(this, ec, |location| location.hash(&entry_settings.origin))?;
    let hash = map_location_value(hash, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(hash.as_str())))
}

// ── Setters ──

fn set_href(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_href_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, ec)
}

fn set_protocol(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_protocol_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_host(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_host_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_hostname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_hostname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_port(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_port_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_pathname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_pathname_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_search(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_search_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn set_hash(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.set_hash_with_origin(&value, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

// ── Methods ──

fn assign_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.assign_with_origin(&value, &entry_settings.api_base_url, &entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn replace_method(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.replace_with_origin(&value, &entry_settings.api_base_url)
    })?;
    map_location_result(result, ec)
}

fn reload_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let result = try_with_location_ref(this, ec, |location| {
        location.reload_with_origin(&entry_settings.origin)
    })?;
    map_location_result(result, ec)
}

fn get_ancestor_origins(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let values = try_with_location_ref(this, ec, |location| {
        location.ancestor_origins_with_origin(&entry_settings.origin)
    })?;
    let values = map_location_value(values, ec)?;
    let array = ec.create_empty_array();
    for value in values {
        let js_val = ec.value_from_string(ec.js_string_from_str(value.as_str()));
        ec.array_push(&array, js_val)?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn to_string_method(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let entry_settings = entry_settings_object(ec)?;
    let href = try_with_location_ref(this, ec, |location| location.href(&entry_settings.origin))?;
    let href = map_location_value(href, ec)?;
    Ok(ec.value_from_string(ec.js_string_from_str(href.as_str())))
}
