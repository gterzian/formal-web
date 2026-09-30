use crate::encoding::TextDecoder;
use crate::encoding::text_decoder::TextDecoderOptions;
use crate::js::Types;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::get_a_copy_of_the_buffer_source;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{boolean_member, dictionary, string_value, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn decoder(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<TextDecoder, Types> {
    this_as::<TextDecoder>(this, "TextDecoder", ec)
}

impl WebIdlInterface<Types> for TextDecoder {
    const NAME: &'static str = "TextDecoder";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let label = match args.first() {
            Some(value) if !Types::value_is_undefined(value) => ec.to_rust_string(value.clone())?,
            _ => String::from("utf-8"),
        };
        let dict = dictionary(
            args.get(1)
                .filter(|value| !Types::value_is_undefined(value)),
            ec,
        )?;
        let options = TextDecoderOptions {
            fatal: boolean_member(&dict, "fatal", false, ec)?,
            ignore_bom: boolean_member(&dict, "ignoreBOM", false, ec)?,
        };
        TextDecoder::constructor(label, options, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [
            ("encoding", encoding as _),
            ("fatal", fatal as _),
            ("ignoreBOM", ignore_bom as _),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
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
        }
        def.add_operation(OperationDef {
            id: "decode",
            length: 0,
            method: decode,
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
    let encoding = decoder(this, ec)?.encoding();
    Ok(string_value(&encoding, ec))
}

fn fatal(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let fatal = decoder(this, ec)?.fatal();
    Ok(ec.value_from_bool(fatal))
}

fn ignore_bom(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let ignore_bom = decoder(this, ec)?.ignore_bom();
    Ok(ec.value_from_bool(ignore_bom))
}

fn decode(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let decoder = decoder(this, ec)?;
    let input = match args.first() {
        Some(value) if !Types::value_is_undefined(value) => {
            Some(get_a_copy_of_the_buffer_source(value, ec)?)
        }
        _ => None,
    };
    let dict = dictionary(
        args.get(1)
            .filter(|value| !Types::value_is_undefined(value)),
        ec,
    )?;
    let stream = boolean_member(&dict, "stream", false, ec)?;
    let output = decoder.decode(input, stream, ec)?;
    Ok(string_value(&output, ec))
}
