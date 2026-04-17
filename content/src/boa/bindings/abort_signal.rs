use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::JsObject,
    property::Attribute,
};

use crate::dom::{
    AbortSignal, DOMException, initialize_dependent_abort_signal, signal_abort,
    with_abort_signal_mut, with_abort_signal_ref,
};
use crate::html::{Window, WindowOrWorkerGlobalScope};

use super::event_target::{ContextEventDispatchHost, register_event_target_methods};

impl Class for AbortSignal {
    const NAME: &'static str = "AbortSignal";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)?;
        register_abort_signal_methods(class)
    }
}

pub(crate) fn register_abort_signal_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .static_method(
            js_string!("abort"),
            1,
            NativeFunction::from_fn_ptr(abort_static),
        )
        .static_method(
            js_string!("timeout"),
            1,
            NativeFunction::from_fn_ptr(timeout_static),
        )
        .static_method(
            js_string!("any"),
            1,
            NativeFunction::from_fn_ptr(any_static),
        )
        .accessor(
            js_string!("aborted"),
            Some(NativeFunction::from_fn_ptr(get_aborted).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("reason"),
            Some(NativeFunction::from_fn_ptr(get_reason).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("onabort"),
            Some(NativeFunction::from_fn_ptr(get_onabort).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_onabort).to_js_function(&realm)),
            Attribute::all(),
        )
        .method(
            js_string!("throwIfAborted"),
            0,
            NativeFunction::from_fn_ptr(throw_if_aborted),
        );
    Ok(())
}

pub(crate) fn abort_reason_from_argument(
    argument: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let Some(argument) = argument else {
        return abort_error_value(context);
    };

    if argument.is_undefined() {
        return abort_error_value(context);
    }

    Ok(argument.clone())
}

pub(crate) fn timeout_reason(context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(DOMException::from_data(
        DOMException::timeout_error(),
        context,
    )?))
}

pub(crate) fn signal_abort_with_context(
    signal: &JsObject,
    reason: JsValue,
    context: &mut Context,
) -> JsResult<()> {
    let mut host = ContextEventDispatchHost::new(context);
    signal_abort(&mut host, signal, reason)
}

fn abort_static(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let reason = abort_reason_from_argument(args.get(0), context)?;
    let signal = AbortSignal::from_data(AbortSignal::aborted_with_reason(reason), context)?;
    Ok(JsValue::from(signal))
}

fn timeout_static(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let milliseconds = args.get_or_undefined(0).to_length(context)?;
    let signal = AbortSignal::from_data(AbortSignal::new(), context)?;
    let callback = NativeFunction::from_copy_closure_with_captures(
        |_, _, signal: &JsObject, context| {
            let reason = timeout_reason(context)?;
            signal_abort_with_context(&signal, reason, context)?;
            Ok(JsValue::undefined())
        },
        signal.clone(),
    )
    .to_js_function(context.realm());

    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsError::from(
            JsNativeError::typ().with_message("AbortSignal.timeout() requires a Window global"),
        )
    })?;
    let _ = window.set_timeout(
        &JsValue::from(callback),
        &JsValue::from(milliseconds as f64),
        Vec::new(),
        context,
    )?;

    Ok(JsValue::from(signal))
}

fn any_static(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let signals = sequence_abort_signals(args.get_or_undefined(0), context)?;
    let result_signal = AbortSignal::from_data(AbortSignal::new(), context)?;
    initialize_dependent_abort_signal(&result_signal, &signals)?;
    Ok(JsValue::from(result_signal))
}

fn get_aborted(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let signal = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal receiver is not an object")
    })?;
    with_abort_signal_ref(&signal, |signal| JsValue::from(signal.aborted_value()))
}

fn get_reason(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let signal = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal receiver is not an object")
    })?;
    with_abort_signal_ref(&signal, |signal| signal.reason_value())
}

fn throw_if_aborted(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let signal = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal receiver is not an object")
    })?;
    let aborted = with_abort_signal_ref(&signal, |signal| signal.aborted_value())?;
    if !aborted {
        return Ok(JsValue::undefined());
    }

    let reason = with_abort_signal_ref(&signal, |signal| signal.reason_value())?;
    Err(JsError::from_opaque(reason))
}

fn get_onabort(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let signal = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal receiver is not an object")
    })?;
    with_abort_signal_ref(&signal, |signal| {
        signal
            .onabort_value()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
}

fn set_onabort(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let signal_object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal receiver is not an object")
    })?;
    let callback = event_handler_value(args.get_or_undefined(0))?;
    let previous = with_abort_signal_mut(this, |signal| signal.replace_onabort(callback.clone()))?;

    if let Some(previous) = previous {
        crate::dom::with_event_target_mut(this, |target| {
            target.remove_event_listener_entry("abort", &previous, false);
        })?;
    }

    if let Some(callback) = callback {
        crate::dom::with_event_target_mut(this, |target| {
            target.add_event_listener(
                &signal_object,
                String::from("abort"),
                Some(callback),
                false,
                false,
                Some(false),
                None,
            )
        })??;
    }

    Ok(JsValue::undefined())
}

fn event_handler_value(value: &JsValue) -> JsResult<Option<JsObject>> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }

    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("event handler is not an object")
    })?;
    if !object.is_callable() {
        return Err(JsNativeError::typ()
            .with_message("event handler is not callable")
            .into());
    }

    Ok(Some(object.clone()))
}

fn sequence_abort_signals(value: &JsValue, context: &mut Context) -> JsResult<Vec<JsObject>> {
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("AbortSignal.any() requires a sequence of AbortSignal objects")
    })?;
    let length = object.get(js_string!("length"), context)?.to_length(context)?;
    let mut signals = Vec::with_capacity(length as usize);

    for index in 0..length {
        let signal_value = object.get(index, context)?;
        let signal_object = signal_value.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("AbortSignal.any() requires AbortSignal objects")
        })?;
        if signal_object.downcast_ref::<AbortSignal>().is_none() {
            return Err(JsNativeError::typ()
                .with_message("AbortSignal.any() requires AbortSignal objects")
                .into());
        }
        signals.push(signal_object.clone());
    }

    Ok(signals)
}

fn abort_error_value(context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(DOMException::from_data(
        DOMException::abort_error(),
        context,
    )?))
}