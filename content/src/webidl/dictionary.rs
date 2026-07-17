/// <https://webidl.spec.whatwg.org/#js-dictionary>
///
/// Infrastructure for converting JavaScript values to Web IDL dictionary types.
/// Each dictionary type implements its own member-by-member extraction by
/// calling `convert_js_to_dictionary` then `DictionaryAccess::get_member` for
/// each member.

use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

/// The result of opening a JS value for dictionary conversion.
pub(crate) enum DictionaryAccess<T: JsTypes> {
    /// The value was a JS object — members can be extracted from it.
    Object(T::JsObject),
    /// The value was null or undefined — the dictionary is empty (all defaults).
    Empty,
}

/// <https://webidl.spec.whatwg.org/#js-dictionary>
///
/// Converts a JavaScript value to an IDL dictionary type.
///
/// Step 1: If jsDict is not an Object and jsDict is neither undefined nor
///         null, then throw a TypeError.
/// Step 2: Let idlDict be an empty ordered map.
/// Step 3: Let dictionaries be a list consisting of D and all of D's inherited
///         dictionaries, in order from least to most derived.
///
/// Returns a `DictionaryAccess` that the caller uses to extract each member
/// via `get_member`, which implements Steps 4.1.2-4.1.4 for individual members.
/// The caller is responsible for applying defaults (Step 4.1.5), checking
/// required members (Step 4.1.6), and converting member values to their
/// declared IDL types (Step 4.1.4.1).
pub(crate) fn convert_js_to_dictionary<T: JsTypes + JsTypesWithRealm>(
    js_value: &T::JsValue,
    ec: &mut dyn ExecutionContext<T>,
) -> Completion<DictionaryAccess<T>, T> {
    // Step 1: If jsDict is not an Object and jsDict is neither undefined nor
    //         null, then throw a TypeError.
    if T::value_is_undefined(js_value) || T::value_is_null(js_value) {
        return Ok(DictionaryAccess::Empty);
    }
    if let Some(object) = T::value_as_object(js_value) {
        return Ok(DictionaryAccess::Object(object.clone()));
    }
    Err(ec.new_type_error("value is not an object, undefined, or null"))
    // Steps 2-3 are implicit: the caller creates the dictionary struct and
    // iterates over members.
}

impl<T: JsTypes + JsTypesWithRealm> DictionaryAccess<T> {
    /// <https://webidl.spec.whatwg.org/#js-dictionary>
    ///
    /// Steps 4.1.2-4.1.4: Get a dictionary member's JS value by key.
    /// Returns `None` if the property is absent (Get returned undefined).
    /// The caller applies the member's default or skips it.
    pub(crate) fn get_member(
        &self,
        key: &str,
        ec: &mut dyn ExecutionContext<T>,
    ) -> Completion<Option<T::JsValue>, T> {
        let Self::Object(object) = self else {
            // Step 4.1.2: If jsDict is either undefined or null, let
            //             jsMemberValue be undefined.
            return Ok(None);
        };
        // Step 4.1.3.1: Let jsMemberValue be ? Get(jsDict, key).
        let js_member_value =
            ExecutionContext::get(ec, object.clone(), ec.property_key_from_str(key))?;
        // Step 4.1.4: If jsMemberValue is not undefined, then …
        if T::value_is_undefined(&js_member_value) {
            return Ok(None);
        }
        Ok(Some(js_member_value))
        // Step 4.1.4.1-4.1.6: handled by the caller (conversion, defaults,
        // required-member check).
    }
}
