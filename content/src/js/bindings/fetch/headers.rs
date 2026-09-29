use crate::fetch::{Headers, HeadersInit};
use crate::js::Types;
use crate::webidl::bindings::{InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::{
    DefaultIteratorKind, PairIterable, convert_js_to_byte_string, convert_js_to_record,
    convert_js_to_sequence, create_default_iterator, create_sequence_from_iterable,
    pair_iterable_for_each, strings_to_js_array,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{string_value, this_as};
use super::member;

type JsValue = <Types as JsTypes>::JsValue;

fn headers(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Headers, Types> {
    this_as::<Headers>(this, "Headers", ec)
}

pub(crate) fn headers_init_from_value(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<HeadersInit, Types> {
    if Types::value_as_object(value).is_none() {
        return Err(ec.new_type_error("HeadersInit must be a sequence or a record"));
    }
    let iterator_key = ec.property_key_from_well_known_symbol("iterator");
    if let Some(method) = ec.get_method(value.clone(), iterator_key)? {
        let sequence = create_sequence_from_iterable(
            value,
            method,
            |inner, ec| {
                convert_js_to_sequence(&inner, |item, ec| convert_js_to_byte_string(&item, ec), ec)
            },
            ec,
        )?;
        return Ok(HeadersInit::Sequence(sequence));
    }
    let record = convert_js_to_record(
        value,
        |key, ec| convert_js_to_byte_string(&key, ec),
        |item, ec| convert_js_to_byte_string(&item, ec),
        ec,
    )?;
    Ok(HeadersInit::Record(record))
}

fn byte_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    let undefined = ec.value_undefined();
    convert_js_to_byte_string(args.get(index).unwrap_or(&undefined), ec)
}

impl WebIdlInterface<Types> for Headers {
    const NAME: &'static str = "Headers";

    fn constructor_length() -> usize {
        0
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let init = match args.first() {
            Some(value) if !Types::value_is_undefined(value) => {
                Some(headers_init_from_value(value, ec)?)
            }
            _ => None,
        };
        Headers::constructor(init, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, operation "append", 2, append);
        member!(def, operation "delete", 1, delete);
        member!(def, operation "get", 1, get);
        member!(def, operation "getSetCookie", 0, get_set_cookie);
        member!(def, operation "has", 1, has);
        member!(def, operation "set", 2, set);
        member!(def, operation "entries", 0, entries);
        member!(def, operation "keys", 0, keys);
        member!(def, operation "values", 0, values);
        member!(def, operation "forEach", 1, for_each);
    }
}

impl PairIterable for Headers {
    fn value_pairs_to_iterate_over(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(JsValue, JsValue)> {
        Headers::value_pairs_to_iterate_over(self)
            .into_iter()
            .map(|(name, value)| (string_value(&name, ec), string_value(&value, ec)))
            .collect()
    }
}

fn append(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let name = byte_string_argument(args, 0, ec)?;
    let value = byte_string_argument(args, 1, ec)?;
    headers.append(&name, &value, ec)?;
    Ok(ec.value_undefined())
}

fn delete(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let name = byte_string_argument(args, 0, ec)?;
    headers.delete(&name, ec)?;
    Ok(ec.value_undefined())
}

fn get(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let name = byte_string_argument(args, 0, ec)?;
    Ok(match headers.get(&name, ec)? {
        Some(value) => string_value(&value, ec),
        None => ec.value_null(),
    })
}

fn get_set_cookie(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let values = headers(this, ec)?.get_set_cookie();
    let array = strings_to_js_array(&values, ec)?;
    Ok(Types::value_from_object(array))
}

fn has(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let name = byte_string_argument(args, 0, ec)?;
    let has = headers.has(&name, ec)?;
    Ok(ec.value_from_bool(has))
}

fn set(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let name = byte_string_argument(args, 0, ec)?;
    let value = byte_string_argument(args, 1, ec)?;
    headers.set(&name, &value, ec)?;
    Ok(ec.value_undefined())
}

fn entries(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let iterator = create_default_iterator(headers, DefaultIteratorKind::KeyPlusValue, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn keys(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let iterator = create_default_iterator(headers, DefaultIteratorKind::Key, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn values(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    let iterator = create_default_iterator(headers, DefaultIteratorKind::Value, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn for_each(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let headers = headers(this, ec)?;
    pair_iterable_for_each(&headers, this, args, ec)
}
