use js_engine::{Completion, ExecutionContext, IntegrityLevel, IteratorKind, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;
type Function = <Types as JsTypes>::Function;

/// <https://webidl.spec.whatwg.org/#js-sequence>
pub(crate) fn convert_js_to_sequence<T>(
    value: &JsValue,
    convert: impl FnMut(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<T, Types>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<T>, Types> {
    // Step 1: If V is not an Object, throw a TypeError.
    if Types::value_as_object(value).is_none() {
        return Err(ec.new_type_error("value is not a sequence"));
    }

    // Step 2: Let method be ? GetMethod(V, @@iterator).
    let iterator_key = ec.property_key_from_well_known_symbol("iterator");
    let method = ec.get_method(value.clone(), iterator_key)?;

    // Step 3: If method is undefined, throw a TypeError.
    let Some(method) = method else {
        return Err(ec.new_type_error("value is not iterable"));
    };

    // Step 4: Return the result of creating a sequence from V and method.
    create_sequence_from_iterable(value, method, convert, ec)
}

/// <https://webidl.spec.whatwg.org/#create-sequence-from-iterable>
pub(crate) fn create_sequence_from_iterable<T>(
    iterable: &JsValue,
    method: Function,
    mut convert: impl FnMut(JsValue, &mut dyn ExecutionContext<Types>) -> Completion<T, Types>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<T>, Types> {
    // Step 1: Let iter be ? GetIteratorFromMethod(iterable, method).
    let mut iterator = ec.get_iterator(iterable.clone(), IteratorKind::Sync, Some(method))?;

    // Step 2: Let i be 0.
    let mut items = Vec::new();

    // Step 3: Repeat
    loop {
        // Step 3.1: Let next be ? IteratorStepValue(iter).
        let next = ec.iterator_step_value(&mut iterator)?;

        // Step 3.2: If next is done, then return an IDL sequence value of type
        // sequence<T> of length i where the value of the element at index j
        // is Sj.
        let Some(next) = next else {
            return Ok(items);
        };

        // Step 3.3: Initialize Si to the result of converting next to an IDL
        // value of type T.
        // Step 3.4: Set i to i + 1.
        items.push(convert(next, ec)?);
    }
}

/// The identity conversion: a sequence<any>.
#[cfg(feature = "webrtc")]
pub(crate) fn any_value(
    value: JsValue,
    _ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    Ok(value)
}

/// The USVString conversion of a sequence element.
pub(crate) fn usv_string_value(
    value: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    ec.to_rust_string(value)
}

/// <https://webidl.spec.whatwg.org/#js-sequence>
fn sequence_to_js_array(
    values: Vec<JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let n be the length of S.
    // Step 2: Let A be ! ArrayCreate(n).
    let array = ec.create_empty_array();

    // Step 3: Initialize i to be 0.
    // Step 4: While i < n:
    for (index, value) in values.into_iter().enumerate() {
        // Step 4.1: Let V be the value in S at index i.
        // Step 4.2: Let E be the result of converting V to a JavaScript value.
        // Step 4.3: Let P be the result of calling ToString(i).
        let key = ec.property_key_from_index(index as u32);

        // Step 4.4: Perform ! CreateDataProperty(A, P, E).
        ec.create_data_property(array.clone(), key, value)?;

        // Step 4.5: Set i to i + 1.
    }

    // Step 5: Return A.
    Ok(array)
}

/// <https://webidl.spec.whatwg.org/#js-sequence>
pub(crate) fn strings_to_js_array(
    values: &[String],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let values = values
        .iter()
        .map(|value| {
            let string = ec.js_string_from_str(value);
            ec.value_from_string(string)
        })
        .collect();
    sequence_to_js_array(values, ec)
}

/// <https://webidl.spec.whatwg.org/#dfn-create-a-frozen-array>
fn create_a_frozen_array(
    values: Vec<JsValue>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let array be the result of converting the sequence of values
    // of type T to a JavaScript value.
    let array = sequence_to_js_array(values, ec)?;

    // Step 2: Perform SetIntegrityLevel(array, "frozen").
    ec.set_integrity_level(array.clone(), IntegrityLevel::Frozen)?;

    // Step 3: Return array.
    Ok(array)
}

/// <https://webidl.spec.whatwg.org/#dfn-create-a-frozen-array>
pub(crate) fn create_a_frozen_array_of_strings(
    values: &[String],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    let values = values
        .iter()
        .map(|value| {
            let string = ec.js_string_from_str(value);
            ec.value_from_string(string)
        })
        .collect();
    create_a_frozen_array(values, ec)
}
