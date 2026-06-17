//! <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>

use boa_engine::JsValue;

/// <https://webidl.spec.whatwg.org/#dfn-array-index-property-name>
pub(crate) fn is_array_index_key(key: &JsValue) -> bool {
    if let Some(s) = key.as_string() {
        let s = s.to_std_string_escaped();
        if s.is_empty() {
            return false;
        }
        let parsed: u64 = match s.parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if parsed >= u32::MAX as u64 {
            return false;
        }
        parsed.to_string() == s
    } else if key.is_number() {
        if let Some(n) = key.as_number() {
            n.fract() == 0.0 && n >= 0.0 && n < u32::MAX as f64
        } else {
            false
        }
    } else {
        false
    }
}
