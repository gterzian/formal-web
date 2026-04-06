// GENERATED FROM: UIEvent.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_u_i_event_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.accessor(js_string!("view"), Some(NativeFunction::from_fn_ptr(get_view).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("detail"), Some(NativeFunction::from_fn_ptr(get_detail).to_js_function(&realm)), None, Attribute::all());
    Ok(())
}

pub(super) fn with_u_i_event_mut<R>(this: &JsValue, f: impl FnOnce(&mut UIEvent) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<UIEvent>() {
        return Ok(f(&mut value));
    }
    Err(JsNativeError::typ().with_message("receiver is not a UIEvent").into())
}
