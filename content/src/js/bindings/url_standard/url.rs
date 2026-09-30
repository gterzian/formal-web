use crate::file_api::{
    create_object_url as create_object_url_for, revoke_object_url as revoke_object_url_for,
};
use crate::js::Types;
use crate::js::bindings::file_api::blob_from_value;
use crate::url_standard::URL;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface, create_interface_instance};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{member, optional_usv_string_argument, string_value, this_as, usv_string_argument};

type JsValue = <Types as JsTypes>::JsValue;

fn url(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<URL, Types> {
    this_as::<URL>(this, "URL", ec)
}

impl WebIdlInterface<Types> for URL {
    const NAME: &'static str = "URL";

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        if args.is_empty() {
            return Err(
                ec.new_type_error("URL constructor: 1 argument required, but only 0 present")
            );
        }
        let url = usv_string_argument(args, 0, ec)?;
        let base = optional_usv_string_argument(args, 1, ec)?;
        URL::constructor(url, base, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, static operation "parse", 1, parse);
        member!(def, static operation "canParse", 1, can_parse);
        member!(def, static operation "createObjectURL", 1, create_object_url);
        member!(def, static operation "revokeObjectURL", 1, revoke_object_url);
        member!(def, attribute "href", href, set_href);
        member!(def, attribute "origin", origin);
        member!(def, attribute "protocol", protocol, set_protocol);
        member!(def, attribute "username", username, set_username);
        member!(def, attribute "password", password, set_password);
        member!(def, attribute "host", host, set_host);
        member!(def, attribute "hostname", hostname, set_hostname);
        member!(def, attribute "port", port, set_port);
        member!(def, attribute "pathname", pathname, set_pathname);
        member!(def, attribute "search", search, set_search);
        member!(def, attribute "searchParams", search_params);
        member!(def, attribute "hash", hash, set_hash);
        member!(def, operation "toJSON", 0, to_json);
        member!(def, operation "toString", 0, href);
    }
}

fn parse(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URL.parse: 1 argument required, but only 0 present"));
    }
    let url = usv_string_argument(args, 0, ec)?;
    let base = optional_usv_string_argument(args, 1, ec)?;
    match URL::parse(url, base, ec)? {
        Some(url) => {
            let object = create_interface_instance::<Types, URL>(url, ec)?;
            Ok(Types::value_from_object(object))
        }
        None => Ok(ec.value_null()),
    }
}

fn can_parse(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URL.canParse: 1 argument required, but only 0 present"));
    }
    let url = usv_string_argument(args, 0, ec)?;
    let base = optional_usv_string_argument(args, 1, ec)?;
    Ok(ec.value_from_bool(URL::can_parse(url, base)))
}

macro_rules! string_getter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let value = url(this, ec)?.$method();
            Ok(string_value(&value, ec))
        }
    };
}

macro_rules! string_setter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let url = url(this, ec)?;
            let value = usv_string_argument(args, 0, ec)?;
            url.$method(value);
            Ok(ec.value_undefined())
        }
    };
}

string_getter!(href, href);
string_getter!(to_json, to_json);
string_getter!(origin, origin);
string_getter!(protocol, protocol);
string_getter!(username, username);
string_getter!(password, password);
string_getter!(host, host);
string_getter!(hostname, hostname);
string_getter!(port, port);
string_getter!(pathname, pathname);
string_getter!(search, search);
string_getter!(hash, hash);
string_setter!(set_protocol, set_protocol);
string_setter!(set_username, set_username);
string_setter!(set_password, set_password);
string_setter!(set_host, set_host);
string_setter!(set_hostname, set_hostname);
string_setter!(set_port, set_port);
string_setter!(set_pathname, set_pathname);
string_setter!(set_search, set_search);
string_setter!(set_hash, set_hash);

fn set_href(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let url = url(this, ec)?;
    let value = usv_string_argument(args, 0, ec)?;
    url.set_href(value, ec)?;
    Ok(ec.value_undefined())
}

fn search_params(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let search_params = url(this, ec)?.search_params();
    let reflector = search_params
        .reflector
        .clone()
        .ok_or_else(|| ec.new_type_error("URLSearchParams has no reflector"))?;
    Ok(Types::value_from_object(reflector))
}

fn create_object_url(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let object = args.first().unwrap_or(&undefined);
    let Some(blob) = blob_from_value(object, ec) else {
        return Err(ec.new_type_error("URL.createObjectURL: the argument is not a Blob"));
    };
    let url = create_object_url_for(blob, ec)?;
    Ok(string_value(&url, ec))
}

fn revoke_object_url(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let url = usv_string_argument(args, 0, ec)?;
    revoke_object_url_for(url, ec)?;
    Ok(ec.value_undefined())
}
