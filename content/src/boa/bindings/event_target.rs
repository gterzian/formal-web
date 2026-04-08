use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::JsObject,
};

use crate::dom::{EventTarget, dispatch, with_event_target_mut};
use crate::webidl::callback_interface_value;

#[derive(Clone, Copy)]
pub(crate) struct AddEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
}

impl Class for EventTarget {
    const NAME: &'static str = "EventTarget";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Ok(EventTarget::default())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)
    }
}

pub(crate) fn register_event_target_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    class
        .method(
            js_string!("addEventListener"),
            3,
            NativeFunction::from_fn_ptr(add_event_listener),
        )
        .method(
            js_string!("removeEventListener"),
            3,
            NativeFunction::from_fn_ptr(remove_event_listener),
        )
        .method(
            js_string!("dispatchEvent"),
            1,
            NativeFunction::from_fn_ptr(dispatch_event),
        );
    Ok(())
}

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let type_ = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let Some(callback) = callback_interface_value(args.get_or_undefined(1))? else {
        return Ok(JsValue::undefined());
    };
    let options = flatten_more(args.get_or_undefined(2), context)?;

    with_event_target_mut(this, |target| {
        target.add_event_listener(
            type_,
            callback,
            options.capture,
            options.once,
            options.passive,
        );
    })?;

    Ok(JsValue::undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let type_ = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let Some(callback) = callback_interface_value(args.get_or_undefined(1))? else {
        return Ok(JsValue::undefined());
    };
    let capture = flatten(args.get_or_undefined(2), context)?;

    with_event_target_mut(this, |target| {
        target.remove_event_listener_entry(&type_, &callback, capture);
    })?;

    Ok(JsValue::undefined())
}

fn dispatch_event(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let target = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("dispatchEvent receiver is not an object")
    })?;
    let event = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("dispatchEvent requires an Event"))?;
    let canceled = dispatch_event_with_context(&target, &event, context)?;
    Ok(JsValue::from(!canceled))
}

fn dispatch_event_with_context(
    target: &JsObject,
    event: &JsObject,
    context: &mut Context,
) -> JsResult<bool> {
    use crate::boa::execution_context::ContextEventDispatchHost;

    let mut host = ContextEventDispatchHost::new(context);
    dispatch(&mut host, target, event, false)
}

pub(crate) fn flatten(options: &JsValue, context: &mut Context) -> JsResult<bool> {
    if let Some(boolean) = options.as_boolean() {
        return Ok(boolean);
    }
    let Some(object) = options.as_object() else {
        return Ok(false);
    };
    Ok(object.get(js_string!("capture"), context)?.to_boolean())
}

pub(crate) fn flatten_more(
    options: &JsValue,
    context: &mut Context,
) -> JsResult<AddEventListenerOptions> {
    let capture = flatten(options, context)?;
    let Some(object) = options.as_object() else {
        return Ok(AddEventListenerOptions {
            capture,
            once: false,
            passive: None,
        });
    };
    let once = object.get(js_string!("once"), context)?.to_boolean();
    let passive = {
        let value = object.get(js_string!("passive"), context)?;
        if value.is_undefined() {
            None
        } else {
            Some(value.to_boolean())
        }
    };
    Ok(AddEventListenerOptions {
        capture,
        once,
        passive,
    })
}
