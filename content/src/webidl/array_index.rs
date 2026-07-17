//! <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>

use js_engine::{ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#legacy-platform-object-abstract-ops>
pub(crate) fn is_array_index_key(key: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> bool {
    // Step 1: "If P is not a String, then return false."

    let s = match <Types as JsTypes>::value_as_string(key) {
        Some(s) => ec.js_string_to_rust_string(&s),
        None => {
            // Also accept numeric JsValues — they coerce to string keys.
            let n = match <Types as JsTypes>::value_as_number(key) {
                Some(n) => n,
                None => return false,
            };
            // For numbers, check the integer range directly.
            // Step 5: "If index is −0, then return false."
            // Step 6: "If index < 0, then return false."
            // Step 7: "If index ≥ 2^32 − 1, then return false."

            if n.fract() != 0.0 || n == -0.0_f64 || n < 0.0 || n >= (1u64 << 32) as f64 - 1.0 {
                return false;
            }
            return true;
        }
    };

    // Step 2: "Let index be CanonicalNumericIndexString(P)."
    // CanonicalNumericIndexString returns undefined for non-numeric strings.
    // Parse as integer and verify round-trip via ToString.
    // Step 3: "If index is undefined, then return false."

    let parsed: u64 = match s.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };

    // Step 6: "If index < 0, then return false." — u64 is always >= 0.
    // Step 7: "If index ≥ 2^32 − 1, then return false."

    if parsed >= (1u64 << 32) - 1 {
        return false;
    }

    // Verify round-trip: CanonicalNumericIndexString requires the string
    // to be the canonical numeric representation (no leading zeros, etc.).
    // Step 4: "If IsInteger(index) is false, then return false."
    // Step 5: "If index is −0, then return false." — u64 excludes −0.
    // Step 8: "Return true."

    parsed.to_string() == s
}
