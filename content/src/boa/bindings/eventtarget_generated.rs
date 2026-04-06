// GENERATED FROM: EventTarget.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_event_target_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.method(js_string!("addEventListener"), 3, NativeFunction::from_fn_ptr(add_event_listener));
    class.method(js_string!("removeEventListener"), 3, NativeFunction::from_fn_ptr(remove_event_listener));
    class.method(js_string!("dispatchEvent"), 1, NativeFunction::from_fn_ptr(dispatch_event));
    Ok(())
}

pub(super) fn with_event_target_mut<R>(this: &JsValue, f: impl FnOnce(&mut EventTarget) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<EventTarget>() {
        return Ok(f(&mut value));
    }
    if let Some(mut value) = object.downcast_mut::<Node>() {
        return Ok(f(&mut value.event_target));
    }
    if let Some(mut value) = object.downcast_mut::<Window>() {
        return Ok(f(&mut value.event_target));
    }
    Err(JsNativeError::typ().with_message("receiver is not a EventTarget").into())
}
