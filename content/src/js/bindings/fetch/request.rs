use crate::dom::AbortSignal;
use crate::fetch::{FetchApiRequest as Request, RequestInit};
use crate::js::Types;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::convert_js_to_byte_string;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{boolean_member, dictionary, string_member, string_value, this_as};
use super::body::{body_init_from_value, body_mixin_definitions, body_mixin_members};
use super::{api_base_url, headers_init_from_value, member, request_info_from_value};

type JsValue = <Types as JsTypes>::JsValue;

fn request(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Request, Types> {
    this_as::<Request>(this, "Request", ec)
}

pub(crate) fn request_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<Request> {
    let object = Types::value_as_object(value)?;
    ec.with_object_any(&object)
        .and_then(|data| data.downcast_ref::<Request>().cloned())
}

fn enumeration_member(
    dict: &crate::webidl::dictionary::DictionaryAccess<Types>,
    key: &str,
    allowed: &[&str],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    let Some(value) = string_member(dict, key, ec)? else {
        return Ok(None);
    };
    if !allowed.contains(&value.as_str()) {
        return Err(ec.new_type_error(&format!(
            "'{value}' is not a valid value for enumeration {key}"
        )));
    }
    Ok(Some(value))
}

pub(crate) fn request_init_from_value(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RequestInit, Types> {
    let dict = dictionary(value, ec)?;
    let method = match dict.get_member("method", ec)? {
        Some(method) => Some(convert_js_to_byte_string(&method, ec)?),
        None => None,
    };
    let mut init = RequestInit {
        method,
        referrer: string_member(&dict, "referrer", ec)?,
        referrer_policy: enumeration_member(
            &dict,
            "referrerPolicy",
            &[
                "",
                "no-referrer",
                "no-referrer-when-downgrade",
                "same-origin",
                "origin",
                "strict-origin",
                "origin-when-cross-origin",
                "strict-origin-when-cross-origin",
                "unsafe-url",
            ],
            ec,
        )?,
        mode: enumeration_member(
            &dict,
            "mode",
            &["same-origin", "no-cors", "cors", "navigate"],
            ec,
        )?,
        credentials: enumeration_member(
            &dict,
            "credentials",
            &["omit", "same-origin", "include"],
            ec,
        )?,
        cache: enumeration_member(
            &dict,
            "cache",
            &[
                "default",
                "no-store",
                "reload",
                "no-cache",
                "force-cache",
                "only-if-cached",
            ],
            ec,
        )?,
        redirect: enumeration_member(&dict, "redirect", &["follow", "error", "manual"], ec)?,
        integrity: string_member(&dict, "integrity", ec)?,
        keepalive: match dict.get_member("keepalive", ec)? {
            Some(_) => Some(boolean_member(&dict, "keepalive", false, ec)?),
            None => None,
        },
        duplex: enumeration_member(&dict, "duplex", &["half"], ec)?,
        headers: None,
        body: None,
        signal: None,
        window: None,
        is_empty: true,
    };
    if let Some(headers) = dict.get_member("headers", ec)? {
        init.headers = Some(headers_init_from_value(&headers, ec)?);
    }
    if let Some(body) = dict.get_member("body", ec)? {
        init.body = Some(if Types::value_is_null(&body) {
            None
        } else {
            Some(body_init_from_value(&body, ec)?)
        });
    }
    if let Some(signal) = dict.get_member("signal", ec)? {
        init.signal = Some(if Types::value_is_null(&signal) {
            None
        } else {
            let signal = Types::value_as_object(&signal).and_then(|object| {
                ec.with_object_any(&object)
                    .and_then(|data| data.downcast_ref::<AbortSignal>().cloned())
            });
            match signal {
                Some(signal) => Some(signal),
                None => {
                    return Err(
                        ec.new_type_error("RequestInit: member signal is not of type AbortSignal")
                    );
                }
            }
        });
    }
    if let Some(window) = dict.get_member("window", ec)? {
        if !Types::value_is_null(&window) {
            return Err(ec.new_type_error("RequestInit: member window is not null"));
        }
        init.window = Some(());
    }
    init.is_empty = init.method.is_none()
        && init.headers.is_none()
        && init.body.is_none()
        && init.referrer.is_none()
        && init.referrer_policy.is_none()
        && init.mode.is_none()
        && init.credentials.is_none()
        && init.cache.is_none()
        && init.redirect.is_none()
        && init.integrity.is_none()
        && init.keepalive.is_none()
        && init.signal.is_none()
        && init.duplex.is_none()
        && init.window.is_none();
    Ok(init)
}

impl WebIdlInterface<Types> for Request {
    const NAME: &'static str = "Request";

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let undefined = ec.value_undefined();
        let input = request_info_from_value(args.first().unwrap_or(&undefined), ec)?;
        let init = request_init_from_value(args.get(1), ec)?;
        let base_url = api_base_url(ec);
        Request::constructor(input, init, base_url.as_deref(), ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "method", method);
        member!(def, attribute "url", url);
        member!(def, attribute "headers", headers);
        member!(def, attribute "destination", destination);
        member!(def, attribute "referrer", referrer);
        member!(def, attribute "referrerPolicy", referrer_policy);
        member!(def, attribute "mode", mode);
        member!(def, attribute "credentials", credentials);
        member!(def, attribute "cache", cache);
        member!(def, attribute "redirect", redirect);
        member!(def, attribute "integrity", integrity);
        member!(def, attribute "keepalive", keepalive);
        member!(def, attribute "isReloadNavigation", is_reload_navigation);
        member!(def, attribute "isHistoryNavigation", is_history_navigation);
        member!(def, attribute "signal", signal);
        member!(def, attribute "duplex", duplex);
        member!(def, operation "clone", 0, clone_method);
        body_mixin_definitions!(def);
    }
}

fn mime_type(request: &Request) -> Option<String> {
    request.headers().header_list().borrow().get("content-type")
}

body_mixin_members!(request, mime_type);

macro_rules! string_getter {
    ($($name:ident),* $(,)?) => {
        $(
            fn $name(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                let value = request(this, ec)?.$name();
                Ok(string_value(&value, ec))
            }
        )*
    };
}

string_getter!(
    method,
    url,
    destination,
    referrer,
    referrer_policy,
    mode,
    credentials,
    cache,
    redirect,
    integrity,
    duplex,
);

macro_rules! boolean_getter {
    ($($name:ident),* $(,)?) => {
        $(
            fn $name(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<Types>,
            ) -> Completion<JsValue, Types> {
                let value = request(this, ec)?.$name();
                Ok(ec.value_from_bool(value))
            }
        )*
    };
}

boolean_getter!(keepalive, is_reload_navigation, is_history_navigation);

fn headers(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let request = request(this, ec)?;
    let reflector = request
        .headers()
        .reflector
        .clone()
        .ok_or_else(|| ec.new_type_error("Headers has no reflector"))?;
    Ok(Types::value_from_object(reflector))
}

fn signal(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let request = request(this, ec)?;
    let signal_object = request
        .signal()
        .object(ec)
        .ok_or_else(|| ec.new_type_error("AbortSignal is missing its JavaScript object"))?;
    Ok(Types::value_from_object(signal_object))
}

fn clone_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let cloned = request(this, ec)?.clone_method(ec)?;
    let object = create_interface_instance::<Types, Request>(cloned, ec)?;
    Ok(Types::value_from_object(object))
}
