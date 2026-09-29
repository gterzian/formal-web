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

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// <https://infra.spec.whatwg.org/#forgiving-base64-decode>
pub(crate) fn forgiving_base64_decode(data: &str) -> Option<Vec<u8>> {
    // Step 1: "Remove all ASCII whitespace from data."
    let mut data: Vec<u8> = data
        .bytes()
        .filter(|byte| !matches!(byte, 0x09 | 0x0A | 0x0C | 0x0D | 0x20))
        .collect();

    // Step 2: "If data’s code point length divides by 4 leaving no remainder, then:"
    if data.len().is_multiple_of(4) {
        // Step 2.1: "If data ends with one or two U+003D (=) code points, then remove them from data."
        if data.ends_with(b"==") {
            data.truncate(data.len() - 2);
        } else if data.ends_with(b"=") {
            data.truncate(data.len() - 1);
        }
    }

    // Step 3: "If data’s code point length divides by 4 leaving a remainder of 1, then return failure."
    if data.len() % 4 == 1 {
        return None;
    }

    // Step 4: "If data contains a code point that is not one of U+002B (+), U+002F (/), ASCII alphanumeric, then return failure."
    if !data
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        return None;
    }

    // Step 5: "Let output be an empty byte sequence."
    let mut output = Vec::with_capacity(data.len() / 4 * 3);

    // Step 6: "Let buffer be an empty buffer that can have bits appended to it."
    let mut buffer: u32 = 0;
    let mut buffered_bits = 0;

    // Step 7: "Let position be a position variable for data, initially pointing at the start of data."
    // Step 8: "While position does not point past the end of data:"
    for byte in data {
        // Step 8.1: "Find the code point pointed to by position in the second column of Table 1: The Base 64 Alphabet of RFC 4648. Let n be the number given in the first cell of the same row."
        let n = BASE64_ALPHABET
            .iter()
            .position(|alphabet_byte| *alphabet_byte == byte)? as u32;

        // Step 8.2: "Append the six bits corresponding to n, most significant bit first, to buffer."
        buffer = (buffer << 6) | n;
        buffered_bits += 6;

        // Step 8.3: "If buffer has accumulated 24 bits, interpret them as three 8-bit big-endian numbers. Append three bytes with values equal to those numbers to output, in the same order, and then empty buffer."
        if buffered_bits == 24 {
            output.extend_from_slice(&[(buffer >> 16) as u8, (buffer >> 8) as u8, buffer as u8]);
            buffer = 0;
            buffered_bits = 0;
        }

        // Step 8.4: "Advance position by 1."
    }

    // Step 9: "If buffer is not empty, it contains either 12 or 18 bits. If it contains 12 bits, then discard the last four and interpret the remaining eight as an 8-bit big-endian number. If it contains 18 bits, then discard the last two and interpret the remaining 16 as two 8-bit big-endian numbers. Append the one or two bytes with values equal to those one or two numbers to output, in the same order."
    match buffered_bits {
        12 => output.push((buffer >> 4) as u8),
        18 => output.extend_from_slice(&[(buffer >> 10) as u8, (buffer >> 2) as u8]),
        _ => {}
    }

    // Step 10: "Return output."
    Some(output)
}

/// <https://infra.spec.whatwg.org/#forgiving-base64-encode>
pub(crate) fn forgiving_base64_encode(data: &[u8]) -> String {
    // "To forgiving-base64 encode given a byte sequence data, apply the base64 algorithm defined in section 4 of RFC 4648 to data and return the result."
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (index, byte)| {
            group | (u32::from(*byte) << (16 - 8 * index))
        });
        let symbols = chunk.len() + 1;
        for index in 0..4 {
            if index < symbols {
                let sextet = ((group >> (18 - 6 * index)) & 0x3F) as usize;
                output.push(BASE64_ALPHABET[sextet] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
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
