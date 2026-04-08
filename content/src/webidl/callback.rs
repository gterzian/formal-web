use boa_engine::{JsError, JsNativeError, JsResult, JsValue, object::JsObject};

pub(crate) trait EcmascriptHost {
    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue>;

    fn is_callable(&self, object: &JsObject) -> bool;

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue>;

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()>;

    fn report_exception(&mut self, error: JsError, callback: &JsObject);
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExceptionBehavior {
    Report,
    Rethrow,
}

pub(crate) fn callback_interface_value(value: &JsValue) -> JsResult<Option<JsObject>> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }

    value
        .as_object()
        .ok_or_else(|| {
            JsError::from(
                JsNativeError::typ().with_message("event listener callback is not an object"),
            )
        })
        .map(|object| Some(object.clone()))
}

/// <https://webidl.spec.whatwg.org/#js-to-callback-function>
pub(crate) fn callback_function_value(value: &JsValue) -> JsResult<JsObject> {
    let object = value.as_object().ok_or_else(|| {
        JsError::from(
            JsNativeError::typ().with_message("animation frame callback is not an object"),
        )
    })?;

    if !object.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("animation frame callback is not callable")
            .into());
    }

    Ok(object.clone())
}

/// <https://webidl.spec.whatwg.org/#call-a-user-objects-operation>
pub(crate) fn call_user_objects_operation(
    host: &mut impl EcmascriptHost,
    value: &JsObject,
    op_name: &str,
    args: &[JsValue],
    this_arg: Option<&JsValue>,
) -> JsResult<JsValue> {
    // Step 1: "Let completion be an uninitialized variable."

    // Step 2: "If thisArg was not given, let thisArg be undefined."
    let mut effective_this_arg = this_arg.cloned().unwrap_or_else(JsValue::undefined);

    // Step 3: "Let O be the JavaScript object corresponding to value."
    let object = value.clone();

    // Step 4: "Let realm be O's associated realm."
    // Step 5: "Let relevant settings be realm's settings object."
    // Step 6: "Let stored settings be value's callback context."
    // Step 7: "Prepare to run script with relevant settings."
    // Step 8: "Prepare to run a callback with stored settings."
    // Note: The content runtime does not yet model callback realms or HTML callback/script preparation stacks explicitly.

    // Step 9: "Let X be O."
    let mut callable = object.clone();

    // Step 10: "If IsCallable(O) is false, then:"
    if !host.is_callable(&object) {
        // Step 10.1: "Let getResult be Completion(Get(O, opName))."
        let operation = host.get(&object, op_name)?;

        // Step 10.2: "If getResult is an abrupt completion, set completion to getResult and jump to the step labeled return."
        // Note: `?` returns the abrupt completion directly in this Rust implementation.

        // Step 10.3: "Set X to getResult.[[Value]]."
        let operation = operation.as_object().ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message(format!(
                "event listener callback does not define `{op_name}`"
            )))
        })?;

        // Step 10.4: "If IsCallable(X) is false, then set completion to a TypeError and jump to the step labeled return."
        if !host.is_callable(&operation) {
            return Err(JsNativeError::typ()
                .with_message(format!(
                    "event listener callback `{op_name}` is not callable"
                ))
                .into());
        }

        callable = operation.clone();

        // Step 10.5: "Set thisArg to O (overriding the provided value)."
        effective_this_arg = JsValue::from(object);
    }

    // Step 11: "Let jsArgs be the result of converting args to a JavaScript arguments list."
    // Note: DOM event dispatch already provides ECMAScript values, so there is no additional conversion layer here yet.

    // Step 12: "Let callResult be Completion(Call(X, thisArg, jsArgs))."
    let result = host.call(&callable, &effective_this_arg, args);

    // Note: The content runtime performs <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint> immediately after each Rust-to-JavaScript callback entry returns.
    host.perform_a_microtask_checkpoint()?;

    let result = result?;

    // Step 13: "If callResult is an abrupt completion, set completion to callResult and jump to the step labeled return."
    // Note: `?` returns the abrupt completion directly in this Rust implementation.

    // Step 14: "Set completion to the result of converting callResult.[[Value]] to an IDL value of the same type as the operation's return type."
    // Note: Event listener callbacks are treated as returning ECMAScript values directly because the runtime does not model typed callback return conversions yet.

    // Return.1: "Clean up after running a callback with stored settings."
    // Return.2: "Clean up after running script with relevant settings."
    // Note: The content runtime does not yet model callback/script cleanup stacks explicitly.

    // Return.3: "If completion is an IDL value, return completion."
    Ok(result)
}

/// <https://webidl.spec.whatwg.org/#invoke-a-callback-function>
pub(crate) fn invoke_callback_function(
    host: &mut impl EcmascriptHost,
    callable: &JsObject,
    args: &[JsValue],
    exception_behavior: ExceptionBehavior,
    this_arg: Option<&JsValue>,
) -> JsResult<JsValue> {
    // Step 1: "Let completion be an uninitialized variable."

    // Step 2: "If thisArg was not given, let thisArg be undefined."
    let effective_this_arg = this_arg.cloned().unwrap_or_else(JsValue::undefined);

    // Step 3: "Let F be the JavaScript object corresponding to callable."
    let function = callable.clone();

    // Step 4: "If IsCallable(F) is false:"
    if !host.is_callable(&function) {
        // Step 4.1: "Return the result of converting undefined to the callback function's return type."
        // Note: The content runtime converts callback results directly as ECMAScript values and currently uses this helper only for `undefined`-returning callbacks.
        return Ok(JsValue::undefined());
    }

    // Step 5: "Let realm be F's associated realm."
    // Step 6: "Let relevant settings be realm's settings object."
    // Step 7: "Let stored settings be callable's callback context."
    // Step 8: "Prepare to run script with relevant settings."
    // Step 9: "Prepare to run a callback with stored settings."
    // Note: The content runtime does not yet model callback realms or HTML callback/script preparation stacks explicitly.

    // Step 10: "Let jsArgs be the result of converting args to a JavaScript arguments list."
    // Note: Callers already provide ECMAScript values, so there is no additional conversion layer here yet.

    // Step 11: "Let callResult be Completion(Call(F, thisArg, jsArgs))."
    let call_result = host.call(&function, &effective_this_arg, args);

    // Note: The content runtime performs <https://html.spec.whatwg.org/#perform-a-microtask-checkpoint> immediately after each Rust-to-JavaScript callback entry returns.
    host.perform_a_microtask_checkpoint()?;

    // Step 12: "If callResult is an abrupt completion, set completion to callResult and jump to the step labeled return."
    // Step 13: "Set completion to the result of converting callResult.[[Value]] to an IDL value of the same type as callable's return type."
    // Note: The content runtime converts callback results directly as ECMAScript values and currently uses this helper only for `undefined`-returning callbacks.
    match call_result {
        Ok(value) => Ok(value),
        Err(error) => {
            // Return.1: "Clean up after running a callback with stored settings."
            // Return.2: "Clean up after running script with relevant settings."
            // Note: The content runtime does not yet model callback/script cleanup stacks explicitly.

            // Return.5: "If exceptionBehavior is \"rethrow\", throw completion.[[Value]]."
            if exception_behavior == ExceptionBehavior::Rethrow {
                return Err(error);
            }

            // Return.6.2: "Report an exception completion.[[Value]] for realm's global object."
            host.report_exception(error, &function);

            // Return.6.3: "Return the unique undefined IDL value."
            Ok(JsValue::undefined())
        }
    }
}
