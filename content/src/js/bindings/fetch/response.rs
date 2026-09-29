use crate::fetch::{Response, ResponseInit};
use crate::js::Types;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{convert_js_to_byte_string, unsigned_short};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{dictionary, string_value, this_as};
use super::body::{body_init_from_value, body_mixin_definitions, body_mixin_members};
use super::{api_base_url, headers_init_from_value, member};

type JsValue = <Types as JsTypes>::JsValue;

fn response(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Response, Types> {
    this_as::<Response>(this, "Response", ec)
}

fn response_init_from_value(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<ResponseInit, Types> {
    let dict = dictionary(value, ec)?;
    let status = match dict.get_member("status", ec)? {
        Some(status) => Some(unsigned_short(&status, ec)?),
        None => None,
    };
    let headers = match dict.get_member("headers", ec)? {
        Some(headers) => Some(headers_init_from_value(&headers, ec)?),
        None => None,
    };
    let status_text = match dict.get_member("statusText", ec)? {
        Some(status_text) => Some(convert_js_to_byte_string(&status_text, ec)?),
        None => None,
    };
    Ok(ResponseInit {
        status,
        status_text,
        headers,
    })
}

fn optional_body_init(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<crate::fetch::BodyInit>, Types> {
    match value {
        Some(value) if !Types::value_is_undefined(value) && !Types::value_is_null(value) => {
            Ok(Some(body_init_from_value(value, ec)?))
        }
        _ => Ok(None),
    }
}

impl WebIdlInterface<Types> for Response {
    const NAME: &'static str = "Response";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let body = optional_body_init(args.first(), ec)?;
        let init = response_init_from_value(args.get(1), ec)?;
        Response::constructor(body, init, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, static operation "error", 0, error);
        member!(def, static operation "redirect", 1, redirect);
        member!(def, static operation "json", 1, json_static);
        member!(def, attribute "type", type_);
        member!(def, attribute "url", url);
        member!(def, attribute "redirected", redirected);
        member!(def, attribute "status", status);
        member!(def, attribute "ok", ok);
        member!(def, attribute "statusText", status_text);
        member!(def, attribute "headers", headers);
        member!(def, operation "clone", 0, clone_method);
        body_mixin_definitions!(def);
    }
}

fn mime_type(response: &Response) -> Option<String> {
    response
        .headers()
        .header_list()
        .borrow()
        .get("content-type")
}

body_mixin_members!(response, mime_type);

fn response_value(
    response: Response,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let object = create_interface_instance::<Types, Response>(response, ec)?;
    Ok(Types::value_from_object(object))
}

fn error(
    _this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let response = Response::error(ec)?;
    response_value(response, ec)
}

fn redirect(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let url = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let status = match args.get(1) {
        Some(status) if !Types::value_is_undefined(status) => unsigned_short(status, ec)?,
        _ => 302,
    };
    let base_url = api_base_url(ec);
    let response = Response::redirect(url, status, base_url.as_deref(), ec)?;
    response_value(response, ec)
}

fn json_static(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let data = args.first().cloned().unwrap_or(undefined);
    let init = response_init_from_value(args.get(1), ec)?;
    let response = Response::json(data, init, ec)?;
    response_value(response, ec)
}

fn type_(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.type_();
    Ok(string_value(&value, ec))
}

fn url(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.url();
    Ok(string_value(&value, ec))
}

fn redirected(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.redirected();
    Ok(ec.value_from_bool(value))
}

fn status(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.status();
    Ok(ec.value_from_number(f64::from(value)))
}

fn ok(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.ok();
    Ok(ec.value_from_bool(value))
}

fn status_text(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = response(this, ec)?.status_text();
    Ok(string_value(&value, ec))
}

fn headers(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let response = response(this, ec)?;
    let reflector = response
        .headers()
        .reflector
        .clone()
        .ok_or_else(|| ec.new_type_error("Headers has no reflector"))?;
    Ok(Types::value_from_object(reflector))
}

fn clone_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let cloned = response(this, ec)?.clone_method(ec)?;
    response_value(cloned, ec)
}
