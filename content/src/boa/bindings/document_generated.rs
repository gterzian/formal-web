// GENERATED FROM: Document.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_document_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.method(js_string!("getElementById"), 1, NativeFunction::from_fn_ptr(get_element_by_id));
    class.method(js_string!("querySelector"), 1, NativeFunction::from_fn_ptr(query_selector));
    class.method(js_string!("querySelectorAll"), 1, NativeFunction::from_fn_ptr(query_selector_all));
    class.method(js_string!("createElement"), 1, NativeFunction::from_fn_ptr(create_element));
    class.method(js_string!("createTextNode"), 1, NativeFunction::from_fn_ptr(create_text_node));
    class.accessor(js_string!("body"), Some(NativeFunction::from_fn_ptr(get_body).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("title"), Some(NativeFunction::from_fn_ptr(get_title).to_js_function(&realm)), Some(NativeFunction::from_fn_ptr(set_title).to_js_function(&realm)), Attribute::all());
    Ok(())
}

pub(super) fn with_document_mut<R>(this: &JsValue, f: impl FnOnce(&mut Document) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<Document>() {
        return Ok(f(&mut value));
    }
    Err(JsNativeError::typ().with_message("receiver is not a Document").into())
}
