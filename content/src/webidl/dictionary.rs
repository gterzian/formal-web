/// <https://webidl.spec.whatwg.org/#js-dictionary>
///
/// Infrastructure for converting JavaScript values to Web IDL dictionary types.
/// Each dictionary type implements its own member-by-member extraction using
/// the helpers provided here.

use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

/// Result of opening a JS value for dictionary conversion: either actual JS
/// object, or the value was null/undefined (empty dictionary).
pub(crate) enum DictionarySource<T: JsTypes> {
    Object(T::JsObject),
    Empty,
}

/// <https://webidl.spec.whatwg.org/#js-dictionary>
///
/// Step 1: If jsDict is not an Object and jsDict is neither undefined nor
/// null, then throw a TypeError.
/// Otherwise, returns `Object` if jsDict is an object, or `Empty` if
/// undefined/null.
pub(crate) fn open_dictionary<T: JsTypes + JsTypesWithRealm>(
    js_value: &T::JsValue,
    ec: &mut dyn ExecutionContext<T>,
) -> Completion<DictionarySource<T>, T> {
    // Step 1: If jsDict is not an Object and jsDict is neither undefined nor
    //         null, then throw a TypeError.
    if T::value_is_undefined(js_value) || T::value_is_null(js_value) {
        return Ok(DictionarySource::Empty);
    }
    if let Some(object) = T::value_as_object(js_value) {
        return Ok(DictionarySource::Object(object.clone()));
    }
    Err(ec.new_type_error("value is not an object, undefined, or null"))
}

/// <https://webidl.spec.whatwg.org/#js-dictionary>
///
/// Steps 4.1.2-4.1.4: Get the JS value for a dictionary member by key.
/// Returns `None` if the property is absent (Get returns undefined) or the
/// value is undefined — caller should apply the member's default or skip.
pub(crate) fn get_dictionary_member<T: JsTypes + JsTypesWithRealm>(
    source: &DictionarySource<T>,
    key: &str,
    ec: &mut dyn ExecutionContext<T>,
) -> Completion<Option<T::JsValue>, T> {
    let DictionarySource::Object(object) = source else {
        // Step 4.1.2: If jsDict is either undefined or null, let jsMemberValue be undefined.
        return Ok(None);
    };
    // Step 4.1.3.1: Let jsMemberValue be ? Get(jsDict, key).
    let js_member_value = ExecutionContext::get(ec, object.clone(), ec.property_key_from_str(key))?;
    // Step 4.1.4: If jsMemberValue is not undefined, then …
    if T::value_is_undefined(&js_member_value) {
        return Ok(None);
    }
    Ok(Some(js_member_value))
}
