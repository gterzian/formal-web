use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue, js_string,
    native_function::NativeFunction, object::JsObject, property::PropertyDescriptor,
};
use url::Url;

use crate::js::platform_objects;
use crate::{
    dom::Document,
    html::{HTMLAnchorElement, HyperlinkElementUtils},
};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

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

/// Register HTMLHyperlinkElementUtils members directly on an interface prototype.
///
/// This is the prototype-based equivalent of `register_hyperlink_element_utils_methods`,
/// for use with the new `register_interface_spec` binding layer that creates prototypes
/// via `JsObject::from_proto_and_data` instead of via `ClassBuilder`.
pub(crate) fn register_hyperlink_element_utils_on_prototype(
    proto: &JsObject,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<(), BoaTypes> {
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
    realm: &boa_engine::realm::Realm,
    name: &str,
    getter: fn(
        &JsValue,
        &[JsValue],
        &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsValue, BoaTypes>,
    setter: Option<
        fn(
            &JsValue,
            &[JsValue],
            &mut dyn ExecutionContext<BoaTypes>,
        ) -> Completion<JsValue, BoaTypes>,
    >,
) -> Completion<(), BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
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

/// Bridge a function pointer taking `&mut dyn ExecutionContext<BoaTypes>`
/// into a `NativeFunction` → `JsObject` callable, using an `unsafe`
/// `repr(transparent)` cast to recover the Boa `Context`.
fn ec_fn_ptr_to_js_function(
    f: fn(
        &JsValue,
        &[JsValue],
        &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<JsValue, BoaTypes>,
    realm: &boa_engine::realm::Realm,
) -> JsObject {
    // SAFETY: BoaEngine is `#[repr(transparent)]` over `Context`, and
    // `ExecutionContext<BoaTypes>` is implemented by `BoaEngine`.
    // Casting `&mut Context` → `&mut dyn ExecutionContext<BoaTypes>` via
    // the `repr(transparent)` guarantee is sound.
    unsafe {
        let native = NativeFunction::from_closure(Box::new(
            move |this: &JsValue, args: &[JsValue], context: &mut Context| -> JsResult<JsValue> {
                let engine: &mut dyn ExecutionContext<BoaTypes> =
                    &mut *(context as *mut Context as *mut js_engine::boa::BoaEngine);
                f(this, args, engine).map_err(|e| JsError::from_opaque(e))
            },
        ));
        native.to_js_function(realm).into()
    }
}

fn get_origin(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.origin(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_protocol(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.protocol(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_protocol(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_protocol(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_username(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.username(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_username(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_username(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_password(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.password(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_password(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_password(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_host(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.host(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_host(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_host(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_hostname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.hostname(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_hostname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_hostname(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_port(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.port(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_port(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_port(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_pathname(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.pathname(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_pathname(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_pathname(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_search(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.search(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_search(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_search(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_hash(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            JsValue::from(JsString::from(hyperlink.hash(&document_creation_url)))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_hash(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let document_creation_url = document_creation_url(ctx)?;
        let value = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_hyperlink_element_utils_ref(this, |hyperlink| {
            hyperlink.set_hash(&document_creation_url, &value)
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
