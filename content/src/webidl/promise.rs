use boa_engine::{
    Context, JsError, JsResult, JsValue,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsPromise},
};

/// <https://webidl.spec.whatwg.org/#a-promise-resolved-with>
pub(crate) fn resolved_promise(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    // Step 1: "Return a promise resolved with value."
    Ok(JsPromise::resolve(value, context)?.into())
}

/// <https://webidl.spec.whatwg.org/#a-promise-rejected-with>
pub(crate) fn rejected_promise(reason: JsValue, context: &mut Context) -> JsResult<JsObject> {
    // Step 1: "Return a promise rejected with reason."
    Ok(JsPromise::reject(JsError::from_opaque(reason), context)?.into())
}

/// <https://webidl.spec.whatwg.org/#js-to-promise>
pub(crate) fn promise_from_value(value: JsValue, context: &mut Context) -> JsResult<JsObject> {
    // Step 1: "Convert the JavaScript value to Promise<T>."
    // Note: Streams already passes an ECMAScript value here, so this collapses to `Promise.resolve(value)`.
    Ok(JsPromise::resolve(value, context)?.into())
}

/// <https://webidl.spec.whatwg.org/#dfn-perform-steps-once-promise-is-settled>
pub(crate) fn transform_promise_to_undefined(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "React to promise with a fulfillment step that returns undefined."
    let on_fulfilled = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    Ok(JsPromise::from_object(promise_object.clone())?
        .then(Some(on_fulfilled), None, context)?
        .into())
}

/// Note: Marks a promise handled by attaching an inert rejection reaction through Boa's native
/// promise hooks.
pub(crate) fn mark_promise_as_handled(
    promise_object: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let on_rejected = NativeFunction::from_fn_ptr(return_undefined).to_js_function(context.realm());
    let _ = JsPromise::from_object(promise_object.clone())?.catch(on_rejected, context)?;
    Ok(())
}

fn return_undefined(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}