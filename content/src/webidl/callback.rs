use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, JsValue,
    object::{JsObject, builtins::JsFunction},
};
use boa_gc::{Finalize, Trace};
use js_engine::BoaTypes;

/// <https://webidl.spec.whatwg.org/#idl-callback-function>
// Note: The content process reuses `Callback` for both [callback function](https://webidl.spec.whatwg.org/#idl-callback-function) type values and objects implementing a [callback interface](https://webidl.spec.whatwg.org/#dfn-callback-interface) because both Web IDL representations are a tuple of (object reference, callback context).
// Note: The callback context remains implicit in the current single-realm content process until callback-realm bookkeeping is modeled explicitly.
#[derive(Clone, Trace, Finalize)]
pub(crate) struct Callback {
    object: JsObject,
}

impl Callback {
    pub(crate) fn from_object(object: JsObject) -> Self {
        Self { object }
    }

    pub(crate) fn equals(&self, other: &Self) -> bool {
        JsObject::equals(&self.object, &other.object)
    }

    /// <https://webidl.spec.whatwg.org/#callback-function-to-js>
    // Note: The callback interface type conversion back to JavaScript yields the same referenced object in the implementation, so this helper serves both representations.
    pub(crate) fn to_js_value(&self) -> JsValue {
        JsValue::from(self.object.clone())
    }
}

// Re-export the generic `EcmascriptHost` trait from `js_engine` so that
// content/ code uses a consistent import path regardless of engine backend.
pub(crate) use js_engine::EcmascriptHost;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExceptionBehavior {
    Report,
    Rethrow,
}

/// <https://webidl.spec.whatwg.org/#js-to-callback-interface>
pub(crate) fn callback_interface_type_value(value: &JsValue) -> JsResult<Callback> {
    // Step 1: "If V is not an Object, then throw a TypeError."
    let object = value.as_object().ok_or_else(|| {
        JsError::from(
            JsNativeError::typ().with_message("callback interface value is not an object"),
        )
    })?;

    // Step 2: "Return the IDL callback interface type value that represents a reference to V, with the incumbent settings object as the callback context."
    // Note: The `Callback` stores the referenced [object implementing a callback interface](https://webidl.spec.whatwg.org/#dfn-callback-interface); the callback context remains implicit in the current single-realm implementation.
    Ok(Callback::from_object(object.clone()))
}

/// <https://webidl.spec.whatwg.org/#js-to-callback-function>
pub(crate) fn callback_function_value(value: &JsValue) -> JsResult<Callback> {
    // Step 1: "If the result of calling IsCallable(V) is false and the conversion to an IDL value is not being performed due to V being assigned to an attribute whose type is a nullable callback function that is annotated with [LegacyTreatNonObjectAsNull], then throw a TypeError."
    // Note: No current content call sites use [LegacyTreatNonObjectAsNull].
    let object = match value.as_object() {
        Some(object) if object.is_callable() => object.clone(),
        _ => {
            return Err(JsNativeError::typ()
                .with_message("callback function value is not callable")
                .into());
        }
    };

    // Step 2: "Return the IDL callback function type value that represents a reference to the same object that V represents, with the incumbent settings object as the callback context."
    // Note: The `Callback` stores the referenced [object implementing a callback interface](https://webidl.spec.whatwg.org/#dfn-callback-interface) or callback function; the callback context remains implicit in the current single-realm implementation.
    Ok(Callback::from_object(object.clone()))
}

/// <https://webidl.spec.whatwg.org/#js-to-nullable>
pub(crate) fn nullable_value<T>(
    value: &JsValue,
    convert_inner: impl FnOnce(&JsValue) -> JsResult<T>,
) -> JsResult<Option<T>> {
    // Note: The current content process uses this helper for nullable callback interface and nullable callback function conversions, so the Rust struct models the `null` result as `None` and delegates all non-null inputs to the inner conversion.

    // Step 1: "If V is not an Object, and the conversion to an IDL value is being performed due to V being assigned to an attribute whose type is a nullable callback function that is annotated with [LegacyTreatNonObjectAsNull], then return the IDL nullable type T? value null."
    // Note: No current content call sites use [LegacyTreatNonObjectAsNull].

    // Step 2: "Otherwise, if V is undefined, and T includes undefined, return the unique undefined value."
    // Note: No current content call sites use inner types that include undefined.

    // Step 3: "Otherwise, if V is null or undefined, then return the IDL nullable type T? value null."
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }

    // Step 4: "Otherwise, return the result of converting V using the rules for the inner IDL type T."
    convert_inner(value).map(Some)
}

/// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
pub(crate) fn call_user_objects_operation(
    host: &mut impl EcmascriptHost<BoaTypes>,
    value: &Callback,
    op_name: &str,
    args: &[JsValue],
    this_arg: Option<&JsValue>,
) -> JsResult<JsValue> {
    // Step 1: "Let completion be an uninitialized variable."

    // Step 2: "If thisArg was not given, let thisArg be undefined."
    let mut effective_this_arg = this_arg.cloned().unwrap_or_else(JsValue::undefined);

    // Step 3: "Let O be the JavaScript object corresponding to value."
    let object = value.object.clone();

    // Step 4: "Let realm be O's associated realm."
    // Step 5: "Let relevant settings be realm's settings object."
    // Step 6: "Let stored settings be value's callback context."
    // Step 7: "Prepare to run script with relevant settings."
    // Step 8: "Prepare to run a callback with stored settings."
    // Note: The content process does not yet model callback realms or HTML callback/script preparation stacks explicitly.

    // Step 9: "Let X be O."
    let object_value = JsValue::from(object.clone());
    let mut callable = object.clone();

    // Step 10: "If IsCallable(O) is false, then:"
    if !host.is_callable(&object_value) {
        // Step 10.1: "Let getResult be Completion(Get(O, opName))."
        let operation = host.get(&object, op_name).map_err(into_js_error)?;

        // Step 10.2: "If getResult is an abrupt completion, set completion to getResult and jump to the step labeled return."
        // Note: `?` returns the abrupt completion directly in this Rust implementation.

        // Step 10.3: "Set X to getResult.[[Value]]."
        // Step 10.4: "If IsCallable(X) is false, then set completion to a TypeError and jump to the step labeled return."
        if !host.is_callable(&operation) {
            return Err(JsNativeError::typ()
                .with_message(format!("callback operation `{op_name}` is not callable"))
                .into());
        }

        let operation = operation.as_object().ok_or_else(|| {
            debug_assert!(
                false,
                "IsCallable returned true for a non-object callback operation"
            );
            JsError::from(
                JsNativeError::typ()
                    .with_message(format!("callback operation `{op_name}` is not callable")),
            )
        })?;

        callable = operation;

        // Step 10.5: "Set thisArg to O (overriding the provided value)."
        effective_this_arg = object_value;
    }

    // Step 11: "Let jsArgs be the result of converting args to a JavaScript arguments list."
    // Note: DOM event dispatch already provides ECMAScript values, so there is no additional conversion layer here yet.

    // Step 12: "Let callResult be Completion(Call(X, thisArg, jsArgs))."
    let result = host.call(&callable, &effective_this_arg, args).map_err(into_js_error)?;

    // Step 13: "If callResult is an abrupt completion, set completion to callResult and jump to the step labeled return."
    // Note: `?` returns the abrupt completion directly in this Rust implementation.

    // Step 14: "Set completion to the result of converting callResult.[[Value]] to an IDL value of the same type as the operation's return type."
    // Note: This helper currently returns the raw ECMAScript completion value; the current DOM listener caller ignores that value, which matches `handleEvent`'s `undefined` return type.

    // Return.1: "Clean up after running a callback with stored settings."
    // Return.2: "Clean up after running script with relevant settings."
    // Note: The content process does not yet model callback/script cleanup stacks explicitly.

    // Return.3: "If completion is an IDL value, return completion."
    Ok(result)
}

/// A thin `EcmascriptHost<BoaTypes>` adapter over a `&mut Context`.
///
/// Used at call sites that have a `Context` without a `BoaEngine` wrapper
/// (e.g. NativeFunction callbacks).  Delegates directly to `Context` operations.
pub(crate) struct ContextEcmaHost<'a> {
    pub context: &'a mut Context,
}

impl js_engine::EcmascriptHost<js_engine::BoaTypes> for ContextEcmaHost<'_> {
    fn get(&mut self, object: &JsObject, property: &str) -> js_engine::Completion<JsValue, js_engine::BoaTypes> {
        object
            .get(JsString::from(property), self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        value.as_object().is_some_and(|o| o.is_callable())
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> js_engine::Completion<JsValue, js_engine::BoaTypes> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("callback is not callable")
                    .into_opaque(self.context),
            )
        })?;
        function
            .call(this_arg, args, self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn perform_a_microtask_checkpoint(&mut self) -> js_engine::Completion<(), js_engine::BoaTypes> {
        let _ = self.context.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsValue) {
        log::error!("uncaught callback error: {error:?}");
    }
}

/// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
pub(crate) fn invoke_callback_function(
    host: &mut impl EcmascriptHost<BoaTypes>,
    callable: &Callback,
    args: &[JsValue],
    exception_behavior: ExceptionBehavior,
    this_arg: Option<&JsValue>,
) -> JsResult<JsValue> {
    // Step 1: "Let completion be an uninitialized variable."

    // Step 2: "If thisArg was not given, let thisArg be undefined."
    let effective_this_arg = this_arg.cloned().unwrap_or_else(JsValue::undefined);

    // Step 3: "Let F be the JavaScript object corresponding to callable."
    let function = callable.object.clone();
    let function_value = JsValue::from(function.clone());

    // Step 4: "If IsCallable(F) is false:"
    if !host.is_callable(&function_value) {
        // Step 4.1: "Return the result of converting undefined to the callback function's return type."
        // Note: The current content process returns the raw ECMAScript `undefined` value here; current callers either expect `undefined`/`any` directly or immediately perform the surrounding algorithm's return-value conversion.
        return Ok(JsValue::undefined());
    }

    // Step 5: "Let realm be F's associated realm."
    // Step 6: "Let relevant settings be realm's settings object."
    // Step 7: "Let stored settings be callable's callback context."
    // Step 8: "Prepare to run script with relevant settings."
    // Step 9: "Prepare to run a callback with stored settings."
    // Note: The content process does not yet model callback realms or HTML callback/script preparation stacks explicitly.

    // Step 10: "Let jsArgs be the result of converting args to a JavaScript arguments list."
    // Note: Callers already provide ECMAScript values, so there is no additional conversion layer here yet.

    // Step 11: "Let callResult be Completion(Call(F, thisArg, jsArgs))."
    let call_result = host.call(&function, &effective_this_arg, args);

    // Step 12: "If callResult is an abrupt completion, set completion to callResult and jump to the step labeled return."
    // Step 13: "Set completion to the result of converting callResult.[[Value]] to an IDL value of the same type as callable's return type."
    // Note: This helper currently returns the raw ECMAScript completion value; surrounding DOM, HTML, and Streams algorithms perform any required promise wrapping or numeric conversion immediately after this call.
    match call_result {
        Ok(value) => Ok(value),
        Err(error) => {
            // Return.1: "Clean up after running a callback with stored settings."
            // Return.2: "Clean up after running script with relevant settings."
            // Note: The content process does not yet model callback/script cleanup stacks explicitly.

            // Return.5: "If exceptionBehavior is \"rethrow\", throw completion.[[Value]]."
            if exception_behavior == ExceptionBehavior::Rethrow {
                return Err(into_js_error(error));
            }

            // Return.6.2: "Report an exception completion.[[Value]] for realm's global object."
            host.report_exception(error);

            // Return.6.3: "Return the unique undefined IDL value."
            Ok(JsValue::undefined())
        }
    }
}

/// Convert a `JsValue` (from the generic `Completion` model) back into a
/// `JsError` for compatibility with existing callers that expect `JsResult`.
///
/// This is a Boa-specific shim needed because `EcmascriptHost` is generic
/// over `JsTypes` and uses `T::JsValue` as the error type of `Completion`,
/// while the existing domain code still returns `JsResult<JsValue>`.
fn into_js_error(value: JsValue) -> JsError {
    JsError::from_opaque(value)
}
