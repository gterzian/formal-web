use boa_engine::{
    Context, JsArgs, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::boa::with_event_mut;
use crate::dom::Event;

impl Class for Event {
    const NAME: &'static str = "Event";
    const LENGTH: usize = 1;

    fn data_constructor(
        _this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let type_ = args
            .get_or_undefined(0)
            .to_string(context)?
            .to_std_string_escaped();
        let init = args.get_or_undefined(1);
        Ok(Event::new(
            type_,
            init_flag(init, js_string!("bubbles"), context)?,
            init_flag(init, js_string!("cancelable"), context)?,
            init_flag(init, js_string!("composed"), context)?,
            false,
            0.0,
        ))
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_methods(class)
    }
}

pub(crate) fn register_event_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("type"),
            Some(NativeFunction::from_fn_ptr(get_type).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("target"),
            Some(NativeFunction::from_fn_ptr(get_target).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("currentTarget"),
            Some(NativeFunction::from_fn_ptr(get_current_target).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("eventPhase"),
            Some(NativeFunction::from_fn_ptr(get_event_phase).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("bubbles"),
            Some(NativeFunction::from_fn_ptr(get_bubbles).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("cancelable"),
            Some(NativeFunction::from_fn_ptr(get_cancelable).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("defaultPrevented"),
            Some(NativeFunction::from_fn_ptr(get_default_prevented).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("cancelBubble"),
            Some(NativeFunction::from_fn_ptr(get_cancel_bubble).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_cancel_bubble).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("isTrusted"),
            Some(NativeFunction::from_fn_ptr(get_is_trusted).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("timeStamp"),
            Some(NativeFunction::from_fn_ptr(get_time_stamp).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .method(
            js_string!("stopPropagation"),
            0,
            NativeFunction::from_fn_ptr(stop_propagation),
        )
        .method(
            js_string!("stopImmediatePropagation"),
            0,
            NativeFunction::from_fn_ptr(stop_immediate_propagation),
        )
        .method(
            js_string!("preventDefault"),
            0,
            NativeFunction::from_fn_ptr(prevent_default),
        );
    Ok(())
}

pub(crate) fn init_flag(init: &JsValue, key: JsString, context: &mut Context) -> JsResult<bool> {
    let Some(object) = init.as_object() else {
        return Ok(false);
    };
    Ok(object.get(key, context)?.to_boolean())
}

fn get_type(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        JsValue::from(JsString::from(event.type_value()))
    })
}

fn get_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
}

fn get_current_target(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event
            .current_target_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
}

fn get_event_phase(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.event_phase_value()))
}

fn get_bubbles(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.bubbles_value()))
}

fn get_cancelable(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancelable_value()))
}

fn get_default_prevented(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.default_prevented()))
}

fn get_cancel_bubble(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.cancel_bubble()))
}

fn set_cancel_bubble(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.set_cancel_bubble(args.first().is_some_and(JsValue::to_boolean));
        JsValue::undefined()
    })
}

fn get_is_trusted(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.is_trusted()))
}

fn get_time_stamp(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| JsValue::from(event.time_stamp_value()))
}

fn stop_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_propagation();
        JsValue::undefined()
    })
}

fn stop_immediate_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.stop_immediate_propagation();
        JsValue::undefined()
    })
}

fn prevent_default(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_event_mut(this, |event| {
        event.prevent_default();
        JsValue::undefined()
    })
}
