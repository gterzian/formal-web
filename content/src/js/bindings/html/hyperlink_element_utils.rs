use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, js_string,
    native_function::NativeFunction, object::JsObject, property::PropertyDescriptor,
};
use url::Url;

use crate::js::platform_objects;
use crate::{
    dom::Document,
    html::{HTMLAnchorElement, HyperlinkElementUtils},
};

use js_engine::{Completion, ExecutionContext, JsTypes};

pub(crate) fn document_creation_url(context: &Context) -> JsResult<Url> {
    let object = platform_objects::document_object(context)?;
    let document = object
        .downcast_ref::<Document>()
        .ok_or_else(|| JsNativeError::typ().with_message("document object is not a Document"))?;
    Ok(document.creation_url.clone())
}

pub(crate) fn document_creation_url_ec(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Url, crate::js::Types> {
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    document_creation_url(ctx).map_err(|e| e.into_opaque(ctx).unwrap_or(JsValue::undefined()))
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

fn try_with_hyperlink_element_utils_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&dyn HyperlinkElementUtils) -> R,
) -> Completion<R, crate::js::Types> {
    let object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("hyperlink receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&object) {
        if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&*anchor));
        }
    }
    Err(ec.new_type_error("receiver does not implement HyperlinkElementUtils"))
}

/// Register HTMLHyperlinkElementUtils members directly on an interface prototype.
///
/// This is the prototype-based equivalent of `register_hyperlink_element_utils_methods`,
/// for use with the new `register_interface_spec` binding layer that creates prototypes
/// via `JsObject::from_proto_and_data` instead of via `ClassBuilder`.
pub(crate) fn register_hyperlink_element_utils_on_prototype(
    proto: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // Note: keeps ec_to_ctx — NativeFunction::from_closure needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let realm = ctx.realm().clone();
    link_property(proto, ec, &realm, "origin", get_origin, None)?;
    link_property(
        proto,
        ec,
        &realm,
        "protocol",
        get_protocol,
        Some(set_protocol),
    )?;
    link_property(
        proto,
        ec,
        &realm,
        "username",
        get_username,
        Some(set_username),
    )?;
    link_property(
        proto,
        ec,
        &realm,
        "password",
        get_password,
        Some(set_password),
    )?;
    link_property(proto, ec, &realm, "host", get_host, Some(set_host))?;
    link_property(
        proto,
        ec,
        &realm,
        "hostname",
        get_hostname,
        Some(set_hostname),
    )?;
    link_property(proto, ec, &realm, "port", get_port, Some(set_port))?;
    link_property(
        proto,
        ec,
        &realm,
        "pathname",
        get_pathname,
        Some(set_pathname),
    )?;
    link_property(proto, ec, &realm, "search", get_search, Some(set_search))?;
    link_property(proto, ec, &realm, "hash", get_hash, Some(set_hash))?;
    Ok(())
}

fn link_property(
    proto: &JsObject,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    realm: &boa_engine::realm::Realm,
    name: &str,
    getter: fn(
        &JsValue,
        &[JsValue],
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types>,
    setter: Option<
        fn(
            &JsValue,
            &[JsValue],
            &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types>,
    >,
) -> Completion<(), crate::js::Types> {
    // Note: keeps ec_to_ctx — NativeFunction::from_closure needs Context.
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<()> {
        let get = ec_fn_ptr_to_js_function(getter, realm);
        let mut desc = PropertyDescriptor::builder()
            .get(get)
            .enumerable(true)
            .configurable(true);
        if let Some(setter_fn) = setter {
            let set = ec_fn_ptr_to_js_function(setter_fn, realm);
            desc = desc.set(set);
        }
        proto.define_property_or_throw(js_string!(name), desc.build(), ctx)?;
        Ok(())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

/// Bridge a function pointer taking `&mut dyn ExecutionContext<crate::js::Types>`
/// into a `NativeFunction` → `JsObject` callable, using an `unsafe`
/// `repr(transparent)` cast to recover the Boa `Context`.
fn ec_fn_ptr_to_js_function(
    f: fn(
        &JsValue,
        &[JsValue],
        &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<JsValue, crate::js::Types>,
    realm: &boa_engine::realm::Realm,
) -> JsObject {
    // Note: keeps ec_to_ctx — NativeFunction::from_closure needs Context.
    // SAFETY: BoaContext is `#[repr(transparent)]` over `Context`, and
    // `ExecutionContext<crate::js::Types>` is implemented by `BoaContext`.
    // Casting `&mut Context` → `&mut dyn ExecutionContext<crate::js::Types>` via
    // the `repr(transparent)` guarantee is sound.
    unsafe {
        let native = NativeFunction::from_closure(Box::new(
            move |this: &JsValue, args: &[JsValue], context: &mut Context| -> JsResult<JsValue> {
                let engine: &mut dyn ExecutionContext<crate::js::Types> =
                    &mut *(context as *mut Context as *mut js_engine::boa::BoaContext);
                f(this, args, engine).map_err(|e| JsError::from_opaque(e))
            },
        ));
        native.to_js_function(realm).into()
    }
}

fn get_origin(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let origin = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.origin(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(origin.as_str())))
}

fn get_protocol(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let protocol = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.protocol(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(protocol.as_str())))
}

fn set_protocol(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_protocol(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_username(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let username = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.username(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(username.as_str())))
}

fn set_username(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_username(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_password(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let password = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.password(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(password.as_str())))
}

fn set_password(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_password(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_host(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let host = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.host(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(host.as_str())))
}

fn set_host(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_host(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_hostname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let hostname = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.hostname(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(hostname.as_str())))
}

fn set_hostname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_hostname(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_port(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let port = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.port(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(port.as_str())))
}

fn set_port(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_port(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_pathname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let pathname = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.pathname(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(pathname.as_str())))
}

fn set_pathname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_pathname(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_search(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let search = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.search(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(search.as_str())))
}

fn set_search(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_search(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}

fn get_hash(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let creation_url = document_creation_url_ec(ec)?;
    let hash = try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.hash(&creation_url)
    })?;
    Ok(ec.value_from_string(ec.js_string_from_str(hash.as_str())))
}

fn set_hash(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let creation_url = document_creation_url_ec(ec)?;
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_hyperlink_element_utils_ref(this, ec, |hyperlink| {
        hyperlink.set_hash(&creation_url, &value)
    })?;
    Ok(ec.value_undefined())
}
