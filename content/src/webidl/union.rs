use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

use super::record::convert_js_to_record_of_strings;
use super::sequence::{convert_js_to_sequence, create_sequence_from_iterable, usv_string_value};

type JsValue = <Types as JsTypes>::JsValue;

/// The IDL union type (sequence<sequence<USVString>> or record<USVString,
/// USVString> or USVString).
pub(crate) enum SequenceOrRecordOrString {
    Sequence(Vec<Vec<String>>),
    Record(Vec<(String, String)>),
    String(String),
}

/// <https://webidl.spec.whatwg.org/#js-union>
pub(crate) fn convert_js_to_sequence_or_record_or_string(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<SequenceOrRecordOrString, Types> {
    // Step 1: If the union type includes undefined and V is undefined, then
    // return the unique undefined IDL value.
    // Step 2: If the union type includes a nullable type and V is null or
    // undefined, then return the IDL value null.
    // Step 3: Let types be the flattened member types of the union type.
    // Step 4: If V is null or undefined, then:
    // Step 4.1: If types includes a dictionary type, then return the result
    // of converting V to that dictionary type.
    // Step 5: If V is a platform object, then:
    // Step 5.1: If types includes an interface type that V implements, then
    // return the IDL value that is a reference to the object V.
    // Step 5.2: If types includes object, then return the IDL value that is
    // a reference to the object V.
    // Step 6: If V is an Object and V has an [[ArrayBufferData]] internal
    // slot, then:
    // Step 7: If V is an Object and V has a [[DataView]] internal slot,
    // then:
    // Step 8: If V is an Object and V has a [[TypedArrayName]] internal slot,
    // then:
    // Step 9: If IsCallable(V) is true, then:
    // Step 9.1: If types includes a callback function type, then return the
    // result of converting V to that callback function type.
    // Step 9.2: If types includes object, then return the IDL value that is
    // a reference to the object V.
    // Note: None of the types of steps 1 to 9 are members of this union.
    // Step 10: If V is an Object, then:
    if Types::value_as_object(value).is_some() {
        // Step 10.1: If types includes a sequence type, then:
        // Step 10.1.1: Let method be ? GetMethod(V, @@iterator).
        let iterator_key = ec.property_key_from_well_known_symbol("iterator");
        let method = ec.get_method(value.clone(), iterator_key)?;

        // Step 10.1.2: If method is not undefined, return the result of
        // creating a sequence of that type from V and method.
        if let Some(method) = method {
            let sequences = create_sequence_from_iterable(
                value,
                method,
                |inner, ec| convert_js_to_sequence(&inner, usv_string_value, ec),
                ec,
            )?;
            return Ok(SequenceOrRecordOrString::Sequence(sequences));
        }

        // Step 10.2: If types includes a frozen array type, then:
        // Step 10.3: If types includes a dictionary type, then return the
        // result of converting V to that dictionary type.
        // Step 10.4: If types includes a record type, then return the result
        // of converting V to that record type.
        return Ok(SequenceOrRecordOrString::Record(
            convert_js_to_record_of_strings(value, ec)?,
        ));
    }

    // Step 11: If V is a Boolean value, then:
    // Step 12: If V is a Number value, then:
    // Step 13: If V is a BigInt value, then:
    // Step 14: If types includes a string type, then return the result of
    // converting V to that type.
    Ok(SequenceOrRecordOrString::String(
        ec.to_rust_string(value.clone())?,
    ))
}
