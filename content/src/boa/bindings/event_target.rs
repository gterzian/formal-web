use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsFunction},
};

use crate::dom::{Document, Element, EventTarget, Node, Window};

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

pub(crate) fn with_event_target_mut<R>(
    this: &JsValue,
    f: impl FnOnce(&mut EventTarget) -> R,
) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("event target receiver is not an object")
    })?;
    if let Some(mut window) = object.downcast_mut::<Window>() {
        return Ok(f(&mut window.event_target));
    }
    if let Some(mut document) = object.downcast_mut::<Document>() {
        return Ok(f(&mut document.node.event_target));
    }
    if let Some(mut element) = object.downcast_mut::<Element>() {
        return Ok(f(&mut element.node.event_target));
    }
    if let Some(mut node) = object.downcast_mut::<Node>() {
        return Ok(f(&mut node.event_target));
    }
    if let Some(mut target) = object.downcast_mut::<EventTarget>() {
        return Ok(f(&mut target));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not an EventTarget")
        .into())
}

pub(crate) fn with_event_target_ref<R>(
    object: &JsObject,
    f: impl FnOnce(&EventTarget) -> R,
) -> JsResult<R> {
    if let Some(window) = object.downcast_ref::<Window>() {
        return Ok(f(&window.event_target));
    }
    if let Some(document) = object.downcast_ref::<Document>() {
        return Ok(f(&document.node.event_target));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(f(&element.node.event_target));
    }
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(f(&node.event_target));
    }
    if let Some(target) = object.downcast_ref::<EventTarget>() {
        return Ok(f(&target));
    }
    Err(JsNativeError::typ()
        .with_message("object is not an EventTarget")
        .into())
}

fn add_event_listener(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let type_ = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let Some(callback) = callback_from_value(args.get_or_undefined(1))? else {
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
    let type_ = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let Some(callback) = callback_from_value(args.get_or_undefined(1))? else {
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
    let canceled = super::dispatch(&target, &event, context)?;
    Ok(JsValue::from(!canceled))
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

pub(crate) fn callback_from_value(value: &JsValue) -> JsResult<Option<JsFunction>> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let Some(object) = value.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("event listener callback is not callable")
            .into());
    };
    JsFunction::from_object(object.clone())
        .map(Some)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("event listener callback is not callable")
                .into()
        })
}