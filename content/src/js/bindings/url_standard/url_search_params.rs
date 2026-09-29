use crate::js::Types;
use crate::url_standard::URLSearchParams;
use crate::webidl::bindings::{InterfaceDefinition, WebIdlInterface};
use crate::webidl::{
    DefaultIteratorKind, PairIterable, SequenceOrRecordOrString,
    convert_js_to_sequence_or_record_or_string, create_default_iterator, pair_iterable_for_each,
    strings_to_js_array,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::{member, optional_usv_string_argument, string_value, this_as, usv_string_argument};

type JsValue = <Types as JsTypes>::JsValue;

fn params(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<URLSearchParams, Types> {
    this_as::<URLSearchParams>(this, "URLSearchParams", ec)
}

impl WebIdlInterface<Types> for URLSearchParams {
    const NAME: &'static str = "URLSearchParams";

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
                convert_js_to_sequence_or_record_or_string(value, ec)?
            }
            _ => SequenceOrRecordOrString::String(String::new()),
        };
        URLSearchParams::constructor(init, ec)
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        member!(def, attribute "size", size);
        member!(def, operation "append", 2, append);
        member!(def, operation "delete", 1, delete);
        member!(def, operation "get", 1, get);
        member!(def, operation "getAll", 1, get_all);
        member!(def, operation "has", 1, has);
        member!(def, operation "set", 2, set);
        member!(def, operation "sort", 0, sort);
        member!(def, operation "toString", 0, to_string);
        member!(def, operation "entries", 0, entries);
        member!(def, operation "keys", 0, keys);
        member!(def, operation "values", 0, values);
        member!(def, operation "forEach", 1, for_each);
    }
}

impl PairIterable for URLSearchParams {
    fn value_pairs_to_iterate_over(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<(JsValue, JsValue)> {
        URLSearchParams::value_pairs_to_iterate_over(self)
            .into_iter()
            .map(|(name, value)| (string_value(&name, ec), string_value(&value, ec)))
            .collect()
    }
}

fn size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let size = params(this, ec)?.size();
    Ok(ec.value_from_number(f64::from(size)))
}

fn append(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.len() < 2 {
        return Err(ec.new_type_error("URLSearchParams.append: 2 arguments required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    let value = usv_string_argument(args, 1, ec)?;
    params.append(name, value);
    Ok(ec.value_undefined())
}

fn delete(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URLSearchParams.delete: 1 argument required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    let value = optional_usv_string_argument(args, 1, ec)?;
    params.delete(name, value);
    Ok(ec.value_undefined())
}

fn get(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URLSearchParams.get: 1 argument required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    Ok(match params.get(name) {
        Some(value) => string_value(&value, ec),
        None => ec.value_null(),
    })
}

fn get_all(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URLSearchParams.getAll: 1 argument required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    let values = params.get_all(name);
    let array = strings_to_js_array(&values, ec)?;
    Ok(Types::value_from_object(array))
}

fn has(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.is_empty() {
        return Err(ec.new_type_error("URLSearchParams.has: 1 argument required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    let value = optional_usv_string_argument(args, 1, ec)?;
    Ok(ec.value_from_bool(params.has(name, value)))
}

fn set(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    if args.len() < 2 {
        return Err(ec.new_type_error("URLSearchParams.set: 2 arguments required"));
    }
    let params = params(this, ec)?;
    let name = usv_string_argument(args, 0, ec)?;
    let value = usv_string_argument(args, 1, ec)?;
    params.set(name, value);
    Ok(ec.value_undefined())
}

fn sort(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    params(this, ec)?.sort();
    Ok(ec.value_undefined())
}

fn to_string(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let serialization = params(this, ec)?.stringification_behavior();
    Ok(string_value(&serialization, ec))
}

fn entries(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let params = params(this, ec)?;
    let iterator = create_default_iterator(params, DefaultIteratorKind::KeyPlusValue, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn keys(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let params = params(this, ec)?;
    let iterator = create_default_iterator(params, DefaultIteratorKind::Key, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn values(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let params = params(this, ec)?;
    let iterator = create_default_iterator(params, DefaultIteratorKind::Value, ec)?;
    Ok(Types::value_from_object(iterator))
}

fn for_each(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let params = params(this, ec)?;
    pair_iterable_for_each(&params, this, args, ec)
}
