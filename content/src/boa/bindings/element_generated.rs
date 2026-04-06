// GENERATED FROM: Element.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.accessor(js_string!("id"), Some(NativeFunction::from_fn_ptr(get_id).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("tagName"), Some(NativeFunction::from_fn_ptr(get_tag_name).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("innerHTML"), Some(NativeFunction::from_fn_ptr(get_inner_h_t_m_l).to_js_function(&realm)), Some(NativeFunction::from_fn_ptr(set_inner_h_t_m_l).to_js_function(&realm)), Attribute::all());
    class.method(js_string!("getAttribute"), 1, NativeFunction::from_fn_ptr(get_attribute));
    class.method(js_string!("setAttribute"), 2, NativeFunction::from_fn_ptr(set_attribute));
    Ok(())
}

pub(super) fn with_element_mut<R>(this: &JsValue, f: impl FnOnce(&mut Element) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<Element>() {
        return Ok(f(&mut value));
    }
    Err(JsNativeError::typ().with_message("receiver is not a Element").into())
}
