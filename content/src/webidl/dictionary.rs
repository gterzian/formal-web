use js_engine::{Completion, ExecutionContext, JsTypes, JsTypesWithRealm};

use crate::dom::AbortSignal;

/// <https://webidl.spec.whatwg.org/#js-dictionary>
pub(crate) enum DictionaryAccess<T: JsTypes> {
    Object(T::JsObject),
    Empty,
}

/// <https://webidl.spec.whatwg.org/#js-dictionary>
// Note: Steps 2-3 (creating the empty dict and iterating inherited dictionaries)
// are implicit — the caller creates the dictionary struct and iterates members.
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
}

impl<T: JsTypes + JsTypesWithRealm> DictionaryAccess<T> {
    /// <https://webidl.spec.whatwg.org/#js-dictionary>
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
    }
}

/// <https://webidl.spec.whatwg.org/#js-union>
///
/// Convert a JS value to the union type `(boolean or AddEventListenerOptions)`,
/// used by the DOM's addEventListener options parameter.
pub(crate) fn convert_boolean_or_add_event_listener_options<T: JsTypes + JsTypesWithRealm>(
    value: &T::JsValue,
    ec: &mut dyn ExecutionContext<T>,
) -> Completion<crate::dom::BooleanOrAddEventListenerOptions, T> {
    // Step 12: If V is a Boolean, then: if types includes boolean, convert.
    if let Some(b) = T::value_as_bool(value) {
        return Ok(crate::dom::BooleanOrAddEventListenerOptions::Boolean(b));
    }

    // Step 4.1: If V is null or undefined and types includes dictionary, convert.
    // Step 11.4: If V is an Object and types includes dictionary, convert.
    let access = convert_js_to_dictionary::<T>(value, ec)?;

    let mut dict = crate::dom::AddEventListenerOptions::default();

    // Member: capture (boolean, default false) — inherited from EventListenerOptions
    if let Some(val) = access.get_member("capture", ec)? {
        dict.capture = ec.to_boolean(&val);
    }

    // Member: once (boolean, default false)
    if let Some(val) = access.get_member("once", ec)? {
        dict.once = ec.to_boolean(&val);
    }

    // Member: passive (boolean, no default — stays None if absent)
    if let Some(val) = access.get_member("passive", ec)? {
        dict.passive = Some(ec.to_boolean(&val));
    }

    // Member: signal (AbortSignal, no default — stays None if absent)
    if let Some(val) = access.get_member("signal", ec)? {
        let signal_obj = T::value_as_object(&val)
            .ok_or_else(|| ec.new_type_error("addEventListener signal must be an AbortSignal"))?;
        dict.signal = Some(
            ec.with_object_any(&signal_obj)
                .and_then(|d| d.downcast_ref::<AbortSignal>().cloned())
                .ok_or_else(|| {
                    ec.new_type_error("addEventListener signal must be an AbortSignal")
                })?,
        );
    }

    Ok(crate::dom::BooleanOrAddEventListenerOptions::Dict(dict))
}
