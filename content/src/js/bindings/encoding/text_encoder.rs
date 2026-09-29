use crate::encoding::TextEncoder;
use crate::js::Types;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{
    array_buffer_view_byte_length, convert_js_to_uint8_array, create_uint8_array,
    write_into_array_buffer_view,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{string_value, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn encoder(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<TextEncoder, Types> {
    this_as::<TextEncoder>(this, "TextEncoder", ec)
}

impl WebIdlInterface<Types> for TextEncoder {
    const NAME: &'static str = "TextEncoder";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        Ok(TextEncoder::constructor())
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "encoding",
            getter: encoding,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "encode",
            length: 0,
            method: encode,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "encodeInto",
            length: 2,
            method: encode_into,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn encoding(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let encoding = encoder(this, ec)?.encoding();
    Ok(string_value(&encoding, ec))
}

fn encode(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let encoder = encoder(this, ec)?;
    let input = match args.first() {
        Some(value) if !Types::value_is_undefined(value) => ec.to_rust_string(value.clone())?,
        _ => String::new(),
    };
    let bytes = encoder.encode(input);
    let array = create_uint8_array(&bytes, ec)?;
    Ok(Types::value_from_object(array))
}

fn encode_into(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let encoder = encoder(this, ec)?;
    if args.len() < 2 {
        return Err(ec.new_type_error("TextEncoder.encodeInto: 2 arguments required"));
    }
    let source = ec.to_rust_string(args[0].clone())?;
    let destination = &args[1];
    convert_js_to_uint8_array(destination, ec)?;
    let destination_byte_length = array_buffer_view_byte_length(destination, ec)?;
    let result = encoder.encode_into(source, destination_byte_length as usize);
    write_into_array_buffer_view(&result.written, destination, 0, ec)?;
    let object = ec.create_plain_object(None);
    let read = ec.value_from_number(result.read as f64);
    let read_key = ec.property_key_from_str("read");
    ec.create_data_property(object.clone(), read_key, read)?;
    let written = ec.value_from_number(result.written.len() as f64);
    let written_key = ec.property_key_from_str("written");
    ec.create_data_property(object.clone(), written_key, written)?;
    Ok(Types::value_from_object(object))
}
