// GENERATED FROM: Event.webidl -- DO NOT EDIT
// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    class::ClassBuilder,
    js_string,
    native_function::NativeFunction,
    object::JsValue as _,
    property::Attribute,
};

pub(super) fn register_event_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class.accessor(js_string!("type"), Some(NativeFunction::from_fn_ptr(get_type).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("target"), Some(NativeFunction::from_fn_ptr(get_target).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("currentTarget"), Some(NativeFunction::from_fn_ptr(get_current_target).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("eventPhase"), Some(NativeFunction::from_fn_ptr(get_event_phase).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("bubbles"), Some(NativeFunction::from_fn_ptr(get_bubbles).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("cancelable"), Some(NativeFunction::from_fn_ptr(get_cancelable).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("defaultPrevented"), Some(NativeFunction::from_fn_ptr(get_default_prevented).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("cancelBubble"), Some(NativeFunction::from_fn_ptr(get_cancel_bubble).to_js_function(&realm)), Some(NativeFunction::from_fn_ptr(set_cancel_bubble).to_js_function(&realm)), Attribute::all());
    class.accessor(js_string!("isTrusted"), Some(NativeFunction::from_fn_ptr(get_is_trusted).to_js_function(&realm)), None, Attribute::all());
    class.accessor(js_string!("timeStamp"), Some(NativeFunction::from_fn_ptr(get_time_stamp).to_js_function(&realm)), None, Attribute::all());
    class.method(js_string!("stopPropagation"), 0, NativeFunction::from_fn_ptr(stop_propagation));
    class.method(js_string!("stopImmediatePropagation"), 0, NativeFunction::from_fn_ptr(stop_immediate_propagation));
    class.method(js_string!("preventDefault"), 0, NativeFunction::from_fn_ptr(prevent_default));
    Ok(())
}

pub(super) fn with_event_mut<R>(this: &JsValue, f: impl FnOnce(&mut Event) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message("receiver is not an object"))?;
    if let Some(mut value) = object.downcast_mut::<Event>() {
        return Ok(f(&mut value));
    }
    if let Some(mut value) = object.downcast_mut::<UIEvent>() {
        return Ok(f(&mut value.event));
    }
    Err(JsNativeError::typ().with_message("receiver is not a Event").into())
}
