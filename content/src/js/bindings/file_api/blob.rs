use crate::file_api::{Blob, BlobPart, BlobPropertyBag, EndingType, File};
use crate::js::Types;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use crate::webidl::{
    clamp_long_long, convert_js_to_sequence, get_a_copy_of_the_buffer_source, is_buffer_source,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{dictionary, optional_usv_string_argument, string_member, string_value};

type JsValue = <Types as JsTypes>::JsValue;

fn blob(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Blob, Types> {
    blob_from_value(this, ec).ok_or_else(|| ec.new_type_error("receiver is not a Blob"))
}

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
}

impl WebIdlInterface<Types> for Blob {
    const NAME: &'static str = "Blob";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let blob_parts = match args.first() {
            Some(value) if !Types::value_is_undefined(value) => {
                Some(convert_blob_parts(value, ec)?)
            }
            _ => None,
        };
        let options = convert_blob_property_bag(args.get(1), ec)?;
        Ok(Blob::constructor(blob_parts, options))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "size", size);
        member!(def, attribute "type", type_);
        member!(def, operation "slice", 0, slice, promise false);
        member!(def, operation "stream", 0, stream, promise false);
        member!(def, operation "text", 0, text, promise true);
        member!(def, operation "arrayBuffer", 0, array_buffer, promise true);
        member!(def, operation "bytes", 0, bytes, promise true);
    }
}

pub(crate) fn blob_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Option<Blob> {
    let object = Types::value_as_object(value)?;
    ec.with_object_any(&object).and_then(|data| {
        data.downcast_ref::<Blob>()
            .cloned()
            .or_else(|| data.downcast_ref::<File>().map(|file| file.blob.clone()))
    })
}

pub(super) fn convert_blob_parts(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<BlobPart>, Types> {
    convert_js_to_sequence(
        value,
        |item, ec| {
            if is_buffer_source(&item, ec) {
                Ok(BlobPart::BufferSource(get_a_copy_of_the_buffer_source(
                    &item, ec,
                )?))
            } else if let Some(blob) = blob_from_value(&item, ec) {
                Ok(BlobPart::Blob(blob))
            } else {
                Ok(BlobPart::String(ec.to_rust_string(item)?))
            }
        },
        ec,
    )
}

pub(super) fn convert_blob_property_bag(
    value: Option<&JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<BlobPropertyBag, Types> {
    let dict = dictionary(value.filter(|value| !Types::value_is_undefined(value)), ec)?;
    let mut bag = BlobPropertyBag::default();
    if let Some(endings) = string_member(&dict, "endings", ec)? {
        bag.endings = EndingType::from_idl(&endings).ok_or_else(|| {
            ec.new_type_error(&format!("'{endings}' is not a valid value for EndingType"))
        })?;
    }
    if let Some(type_) = string_member(&dict, "type", ec)? {
        bag.type_ = type_;
    }
    Ok(bag)
}

fn size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let size = blob(this, ec)?.size();
    Ok(ec.value_from_number(size as f64))
}

fn type_(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let type_ = blob(this, ec)?.type_();
    Ok(string_value(&type_, ec))
}

fn slice(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let blob = blob(this, ec)?;
    let start = match args.first() {
        Some(value) if !Types::value_is_undefined(value) => Some(clamp_long_long(value, ec)?),
        _ => None,
    };
    let end = match args.get(1) {
        Some(value) if !Types::value_is_undefined(value) => Some(clamp_long_long(value, ec)?),
        _ => None,
    };
    let content_type = optional_usv_string_argument(args, 2, ec)?;
    let sliced = blob.slice(start, end, content_type);
    let object = create_interface_instance::<Types, Blob>(sliced, ec)?;
    Ok(Types::value_from_object(object))
}

fn stream(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let (_stream, stream_object) = blob(this, ec)?.stream(ec)?;
    Ok(Types::value_from_object(stream_object))
}

fn text(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = blob(this, ec)?.text(ec)?;
    Ok(Types::value_from_object(promise))
}

fn array_buffer(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = blob(this, ec)?.array_buffer(ec)?;
    Ok(Types::value_from_object(promise))
}

fn bytes(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let promise = blob(this, ec)?.bytes_method(ec)?;
    Ok(Types::value_from_object(promise))
}
