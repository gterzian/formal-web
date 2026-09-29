use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#js-to-unsigned-long-long>
pub(crate) fn enforce_range_unsigned_long_long(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u64, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 64, \"unsigned\")."
    // Note: only the [EnforceRange] branch of ConvertToInt is implemented: a
    // 64-bit type is bounded by 2^53 − 1 so the value is an unambiguous
    // integer in JavaScript's Number type (ConvertToInt step 1.1), the value
    // is rounded toward zero (step 6.2), and NaN, ±∞, or an out-of-range value
    // throws a TypeError instead of wrapping (steps 6.1 and 6.3).
    let number = ec.to_number(value.clone())?;
    let rounded = number.trunc();
    if !(0.0..=9_007_199_254_740_991.0).contains(&rounded) {
        return Err(ec.new_type_error("value is outside the unsigned long long range"));
    }
    // Step 2: "Return the IDL unsigned long long value that represents the same numeric value as x."
    Ok(rounded as u64)
}

/// <https://webidl.spec.whatwg.org/#js-unsigned-short>
/// The [EnforceRange] form.
pub(crate) fn enforce_range_unsigned_short(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u16, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 16, \"unsigned\")."
    // Note: The [EnforceRange] branch of ConvertToInt: NaN, ±∞, or a value
    // outside 0 to 2^16 − 1 after rounding toward zero throws a TypeError
    // (ConvertToInt steps 6.1 to 6.3).
    let number = ec.to_number(value.clone())?;
    if !number.is_finite() {
        return Err(ec.new_type_error("value is not a finite number"));
    }
    let rounded = number.trunc();
    if !(0.0..=65_535.0).contains(&rounded) {
        return Err(ec.new_type_error("value is outside the unsigned short range"));
    }
    // Step 2: "Return the IDL unsigned short value that represents the same numeric value as x."
    Ok(rounded as u16)
}

/// <https://webidl.spec.whatwg.org/#js-unsigned-short>
/// The form without [EnforceRange] or [Clamp].
pub(crate) fn unsigned_short(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u16, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 16, \"unsigned\")."
    // Note: ConvertToInt steps 8-11: NaN, +0, −0, +∞ and −∞ become 0; the
    // value is rounded toward zero and taken modulo 2^16.
    let number = ec.to_number(value.clone())?;
    if !number.is_finite() {
        return Ok(0);
    }
    // Step 2: "Return the IDL unsigned short value that represents the same numeric value as x."
    Ok((number.trunc().rem_euclid(65_536.0)) as u16)
}
