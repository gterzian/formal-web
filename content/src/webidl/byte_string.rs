use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#js-ByteString>
pub(crate) fn convert_js_to_byte_string(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    // Step 1: Let x be ? ToString(V).
    let string = ec.to_rust_string(value.clone())?;

    // Step 2: If the value of any element of x is greater than 255, then
    // throw a TypeError.
    if string.chars().any(|character| u32::from(character) > 255) {
        return Err(ec.new_type_error(
            "the string cannot be converted to a ByteString: it has a code unit greater than 255",
        ));
    }

    // Step 3: Return an IDL ByteString value whose length is the length of x,
    // and where the value of each element is the value of the corresponding
    // element of x.
    Ok(string)
}
