use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;

type JsValue = <Types as JsTypes>::JsValue;

/// <https://webidl.spec.whatwg.org/#js-record>
pub(crate) fn convert_js_to_record_of_strings(
    value: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Vec<(String, String)>, Types> {
    // Step 1: If V is not an Object, throw a TypeError.
    let Some(object) = Types::value_as_object(value) else {
        return Err(ec.new_type_error("value is not a record"));
    };

    // Step 2: Let result be a new empty instance of record<K, V>.
    let mut result: Vec<(String, String)> = Vec::new();

    // Step 3: Let keys be ? V.[[OwnPropertyKeys]]().
    let keys = ec.own_property_keys(object.clone())?;

    // Step 4: For each key of keys:
    for key in keys {
        // Step 4.1: Let desc be ? V.[[GetOwnProperty]](key).
        let descriptor = ec.get_own_property(object.clone(), key.clone())?;

        // Step 4.2: If desc is not undefined and desc.[[Enumerable]] is true:
        let Some(descriptor) = descriptor else {
            continue;
        };
        if descriptor.enumerable != Some(true) {
            continue;
        }

        // Step 4.2.1: Let typedKey be key converted to an IDL value of type K.
        // Note: A Symbol key has no string conversion; such keys are left out
        // of the record.
        let key_value = ec.value_from_property_key(key.clone());
        if Types::value_as_symbol(&key_value).is_some() {
            continue;
        }
        let typed_key = ec.to_rust_string(key_value)?;

        // Step 4.2.2: Let value be ? Get(V, key).
        let value = ExecutionContext::get(ec, object.clone(), key)?;

        // Step 4.2.3: Let typedValue be value converted to an IDL value of
        // type V.
        let typed_value = ec.to_rust_string(value)?;

        // Step 4.2.4: Set result[typedKey] to typedValue.
        match result.iter_mut().find(|(name, _)| *name == typed_key) {
            Some(entry) => entry.1 = typed_value,
            None => result.push((typed_key, typed_value)),
        }
    }

    // Step 5: Return result.
    Ok(result)
}
