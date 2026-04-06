// GENERATED FROM: Node.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_node_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.accessor(js_string!("textContent"), Some(NativeFunction::from_fn_ptr(get_text_content).to_js_function(&realm)), Some(NativeFunction::from_fn_ptr(set_text_content).to_js_function(&realm)), Attribute::all());
    class.method(js_string!("appendChild"), 1, NativeFunction::from_fn_ptr(append_child));
    Ok(())
}

pub(super) fn with_node_mut<R>(this: &JsValue, f: impl FnOnce(&mut Node) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<Node>() {
        return Ok(f(&mut value));
    }
    if let Some(mut value) = object.downcast_mut::<Document>() {
        return Ok(f(&mut value.node));
    }
    if let Some(mut value) = object.downcast_mut::<Element>() {
        return Ok(f(&mut value.node));
    }
    Err(JsNativeError::typ().with_message("receiver is not a Node").into())
}
