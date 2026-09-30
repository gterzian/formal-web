use crate::encoding::{utf_8_decode, utf_8_encode};
use crate::js::Types;
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

/// <https://infra.spec.whatwg.org/#strip-and-collapse-ascii-whitespace>
pub(crate) fn strip_and_collapse_ascii_whitespace(value: &str) -> String {
    // Step 1: "Replace any sequence of one or more consecutive code points that are ASCII whitespace in the string with a single U+0020 SPACE code point, and then remove any leading and trailing ASCII whitespace from that string."
    let mut normalized = String::with_capacity(value.len());
    let mut pending_space = false;

    for character in value.chars() {
        if matches!(
            character,
            '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' | ' '
        ) {
            pending_space = !normalized.is_empty();
            continue;
        }

        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }

        normalized.push(character);
    }

    normalized
}

/// <https://infra.spec.whatwg.org/#parse-json-bytes-to-a-javascript-value>
pub(crate) fn parse_json_bytes_to_a_javascript_value(
    bytes: &[u8],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Step 1: Let string be the result of running UTF-8 decode on bytes.
    let string = utf_8_decode(bytes);

    // Step 2: Return the result of parsing a JSON string to a JavaScript
    // value given string.
    parse_a_json_string_to_a_javascript_value(&string, ec)
}

/// <https://infra.spec.whatwg.org/#parse-a-json-string-to-a-javascript-value>
pub(crate) fn parse_a_json_string_to_a_javascript_value(
    string: &str,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    // Return ? Call(%JSON.parse%, undefined, « string »).
    let global = ec.realm_global_object();
    let json_key = ec.property_key_from_str("JSON");
    let json = ExecutionContext::get(ec, global, json_key)?;
    let parse_key = ec.property_key_from_str("parse");
    let parse = ec.get_v(json, parse_key)?;
    let Some(parse) = <Types as JsTypes>::value_as_object(&parse) else {
        return Err(ec.new_type_error("JSON.parse is not callable"));
    };
    let undefined = ec.value_undefined();
    let string = ec.js_string_from_str(string);
    let string = ec.value_from_string(string);
    ec.call(&parse, &undefined, &[string])
}

/// <https://infra.spec.whatwg.org/#serialize-a-javascript-value-to-a-json-string>
pub(crate) fn serialize_a_javascript_value_to_a_json_string(
    value: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    // Step 1: Let result be ? Call(%JSON.stringify%, undefined, « value »).
    let global = ec.realm_global_object();
    let json_key = ec.property_key_from_str("JSON");
    let json = ExecutionContext::get(ec, global, json_key)?;
    let stringify_key = ec.property_key_from_str("stringify");
    let stringify = ec.get_v(json, stringify_key)?;
    let Some(stringify) = <Types as JsTypes>::value_as_object(&stringify) else {
        return Err(ec.new_type_error("JSON.stringify is not callable"));
    };
    let undefined = ec.value_undefined();
    let result = ec.call(&stringify, &undefined, &[value])?;

    // Step 2: If result is undefined, then throw a TypeError.
    if <Types as JsTypes>::value_is_undefined(&result) {
        return Err(ec.new_type_error("the value cannot be serialized to JSON"));
    }

    // Step 3: Assert: result is a string.
    // Step 4: Return result.
    ec.to_rust_string(result)
}

/// <https://infra.spec.whatwg.org/#serialize-a-javascript-value-to-json-bytes>
pub(crate) fn serialize_a_javascript_value_to_json_bytes(
    value: JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<u8>, Types> {
    // Step 1: Let string be the result of serializing a JavaScript value to a
    // JSON string given value.
    let string = serialize_a_javascript_value_to_a_json_string(value, ec)?;

    // Step 2: Return the result of running UTF-8 encode on string.
    Ok(utf_8_encode(&string))
}
