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
#[cfg(feature = "webrtc")]
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

/// <https://webidl.spec.whatwg.org/#js-to-long-long>
pub(crate) fn clamp_long_long(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<i64, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 64, \"signed\")."
    // Note: only the [Clamp] branch of ConvertToInt is implemented: a 64-bit
    // type is bounded by ±(2^53 − 1) so the value is an unambiguous integer
    // in JavaScript's Number type (ConvertToInt step 1.1); NaN becomes +0
    // (step 8), and the value is clamped to the bounds and rounded to the
    // nearest integer, ties to even (steps 7.1 and 7.2).
    let number = ec.to_number(value.clone())?;
    if number.is_nan() {
        return Ok(0);
    }
    let bound = 9_007_199_254_740_991.0;
    let clamped = number.clamp(-bound, bound);
    let rounded = {
        let floor = clamped.floor();
        let fraction = clamped - floor;
        if fraction > 0.5 || (fraction == 0.5 && floor % 2.0 != 0.0) {
            floor + 1.0
        } else {
            floor
        }
    };

    // Step 2: "Return the IDL long long value that represents the same numeric value as x."
    Ok(rounded as i64)
}

/// <https://webidl.spec.whatwg.org/#js-to-long-long>
pub(crate) fn long_long(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<i64, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 64, \"signed\")."
    // Note: the plain (no [EnforceRange], no [Clamp]) branch of ConvertToInt:
    // NaN, ±0 and ±∞ become +0 (step 8), the value is rounded toward zero
    // (step 9) and reduced modulo 2^64 into the signed range (steps 10 and
    // 11).
    let number = ec.to_number(value.clone())?;
    if !number.is_finite() {
        return Ok(0);
    }
    let truncated = number.trunc();
    let modulus = 18_446_744_073_709_551_616.0_f64;
    let reduced = truncated.rem_euclid(modulus);
    let signed = if reduced >= 9_223_372_036_854_775_808.0 {
        reduced - modulus
    } else {
        reduced
    };

    // Step 2: "Return the IDL long long value that represents the same numeric value as x."
    Ok(signed as i64)
}

/// <https://webidl.spec.whatwg.org/#js-unsigned-short>
pub(crate) fn clamp_unsigned_short(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<u16, Types> {
    // Step 1: "Let x be ? ConvertToInt(V, 16, \"unsigned\")."
    // Note: only the [Clamp] branch of ConvertToInt is implemented: NaN
    // becomes +0 (step 8), and the value is clamped to [0, 2^16 − 1] and
    // rounded to the nearest integer, ties to even (steps 7.1 and 7.2).
    let number = ec.to_number(value.clone())?;
    if number.is_nan() {
        return Ok(0);
    }
    let clamped = number.clamp(0.0, 65_535.0);
    let rounded = {
        let floor = clamped.floor();
        let fraction = clamped - floor;
        if fraction > 0.5 || (fraction == 0.5 && floor % 2.0 != 0.0) {
            floor + 1.0
        } else {
            floor
        }
    };

    // Step 2: "Return the IDL unsigned short value that represents the same numeric value as x."
    Ok(rounded as u16)
}
