use crate::fetch::{BodyInit, BodyMixin};
use crate::js::Types;
use crate::url_standard::URLSearchParams;
use crate::webidl::{get_a_copy_of_the_buffer_source, is_buffer_source};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::file_api::blob_from_value;

type JsValue = <Types as JsTypes>::JsValue;

pub(crate) fn body_init_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<BodyInit, Types> {
    if let Some(blob) = blob_from_value(value, ec) {
        return Ok(BodyInit::Blob(blob));
    }
    if is_buffer_source(value, ec) {
        return Ok(BodyInit::BufferSource(get_a_copy_of_the_buffer_source(
            value, ec,
        )?));
    }
    if let Some(object) = Types::value_as_object(value)
        && let Some(params) = ec
            .with_object_any(&object)
            .and_then(|data| data.downcast_ref::<URLSearchParams>().cloned())
    {
        return Ok(BodyInit::URLSearchParams(params));
    }
    Ok(BodyInit::String(ec.to_rust_string(value.clone())?))
}

pub(crate) fn body_attribute(
    body: &BodyMixin,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(match body.stream(ec)? {
        Some(stream) => Types::value_from_object(stream),
        None => ec.value_null(),
    })
}

/// Generates the Body mixin's members for an interface whose receiver is
/// resolved by `$receiver` and whose body's MIME type is `$mime_type`.
macro_rules! body_mixin_members {
    ($receiver:ident, $mime_type:expr) => {
        fn body(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let object = $receiver(this, ec)?;
            super::body::body_attribute(object.body_mixin(), ec)
        }

        fn body_used(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let used = $receiver(this, ec)?.body_mixin().body_used(ec);
            Ok(ec.value_from_bool(used))
        }

        fn array_buffer(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let promise = $receiver(this, ec)?.body_mixin().array_buffer(ec)?;
            Ok(Types::value_from_object(promise))
        }

        fn blob(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let object = $receiver(this, ec)?;
            let mime_type = $mime_type(&object);
            let promise = object.body_mixin().blob(mime_type, ec)?;
            Ok(Types::value_from_object(promise))
        }

        fn bytes(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let promise = $receiver(this, ec)?.body_mixin().bytes(ec)?;
            Ok(Types::value_from_object(promise))
        }

        fn json(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let promise = $receiver(this, ec)?.body_mixin().json(ec)?;
            Ok(Types::value_from_object(promise))
        }

        fn text(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let promise = $receiver(this, ec)?.body_mixin().text(ec)?;
            Ok(Types::value_from_object(promise))
        }
    };
}
pub(crate) use body_mixin_members;

macro_rules! body_mixin_definitions {
    ($def:expr) => {
        member!($def, attribute "body", body);
        member!($def, attribute "bodyUsed", body_used);
        member!($def, operation "arrayBuffer", 0, array_buffer, promise true);
        member!($def, operation "blob", 0, blob, promise true);
        member!($def, operation "bytes", 0, bytes, promise true);
        member!($def, operation "json", 0, json, promise true);
        member!($def, operation "text", 0, text, promise true);
    };
}
pub(crate) use body_mixin_definitions;
