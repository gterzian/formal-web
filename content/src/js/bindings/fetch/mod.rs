mod body;
mod headers;
mod request;
mod response;

macro_rules! member {
    ($def:expr, attribute $id:literal, $getter:expr) => {
        $def.add_attribute(AttributeDef {
            id: $id,
            getter: $getter,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        })
    };
    ($def:expr, operation $id:literal, $length:literal, $method:expr) => {
        member!($def, operation $id, $length, $method, promise false)
    };
    ($def:expr, operation $id:literal, $length:literal, $method:expr, promise $promise:literal) => {
        $def.add_operation(OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: false,
            unforgeable: false,
            promise_type: $promise,
            exposed: None,
        })
    };
    ($def:expr, static operation $id:literal, $length:literal, $method:expr) => {
        $def.add_operation(OperationDef {
            id: $id,
            length: $length,
            method: $method,
            static_: true,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        })
    };
}
pub(crate) use member;

pub(crate) use headers::headers_init_from_value;
pub(crate) use request::{request_from_value, request_init_from_value};

use crate::fetch::{RequestInfo, fetch};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

/// The API base URL of the current global: the URL the global was created
/// with.
pub(crate) fn api_base_url(ec: &mut dyn ExecutionContext<Types>) -> Option<String> {
    with_global_scope(ec, |global_scope, _ec| Ok(global_scope.creation_url()))
        .ok()
        .flatten()
        .map(|url| url.to_string())
}

pub(crate) fn request_info_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<RequestInfo, Types> {
    if let Some(request) = request_from_value(value, ec) {
        return Ok(RequestInfo::Request(request));
    }
    Ok(RequestInfo::USVString(ec.to_rust_string(value.clone())?))
}

pub(crate) fn fetch_operation(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let input = request_info_from_value(args.first().unwrap_or(&undefined), ec)?;
    let init = request_init_from_value(args.get(1), ec)?;
    let base_url = api_base_url(ec);
    let promise = fetch(input, init, base_url.as_deref(), ec)?;
    Ok(Types::value_from_object(promise))
}
