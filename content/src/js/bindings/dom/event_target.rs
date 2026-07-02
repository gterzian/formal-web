use std::marker::PhantomData;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{Context, JsArgs, JsNativeError, JsResult, JsValue, js_string, object::JsObject};

use crate::dom::{AbortSignal, Event, EventDispatchHost, EventTarget, dispatch};
use crate::js::downcast::try_with_event_target_mut;
use crate::js::platform_objects;
use crate::webidl::{callback_interface_type_value_ec, nullable_value_ec};

use js_engine::{Completion, ExecutionContext, JsTypes};

#[derive(Clone)]
pub(crate) struct AddEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
    pub signal: Option<AbortSignal>,
}

// ── WebIDL interface definition (§3) ──

use crate::webidl::bindings::{
    InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};

impl WebIdlInterface<crate::js::Types> for EventTarget {
    const NAME: &'static str = "EventTarget";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        Ok(EventTarget::default())
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "addEventListener",
            length: 3,
            method: add_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "removeEventListener",
            length: 3,
            method: remove_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "dispatchEvent",
            length: 1,
            method: dispatch_event,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object_ec(this, ec);
    let type_ = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    let options = flatten_more_ec(args.get_or_undefined(2), ec)?;
    let callback = nullable_value_ec(
        args.get_or_undefined(1),
        ec,
        callback_interface_type_value_ec,
    )?;
    let receiver = JsValue::from(event_target.clone());

    try_with_event_target_mut(&receiver, ec, |target| {
        target.add_event_listener(
            &event_target,
            type_,
            callback,
            options.capture,
            options.once,
            options.passive,
            options.signal,
        );
    })?;

    Ok(JsValue::undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_target = current_event_target_object_ec(this, ec);
    let type_ = ec.to_rust_string(args.get_or_undefined(0).clone())?;
    let Some(callback) = nullable_value_ec(
        args.get_or_undefined(1),
        ec,
        callback_interface_type_value_ec,
    )?
    else {
        return Ok(JsValue::undefined());
    };
    let capture = flatten_ec(args.get_or_undefined(2), ec)?;
    let receiver = JsValue::from(event_target);

    try_with_event_target_mut(&receiver, ec, |target| {
        target.remove_event_listener_entry(&type_, &callback, capture);
    })?;

    Ok(JsValue::undefined())
}

fn dispatch_event(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_obj = match <crate::js::Types as JsTypes>::value_as_object(args.get_or_undefined(0)) {
        Some(obj) => obj,
        None => return Err(ec.new_type_error("dispatchEvent requires an Event")),
    };
    let target = current_event_target_object_ec(this, ec);
    let mut host = EcDispatchHost::new(ec);
    let canceled = dispatch(&mut host, &target, &event_obj, false)?;
    Ok(JsValue::from(!canceled))
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: Uses `ec_to_ctx` internally for methods that need Boa `Context`, but callers
// never see `Context`.
pub(crate) struct EcDispatchHost<'a, T: JsTypes> {
    ec: &'a mut dyn ExecutionContext<T>,
}

impl<'a, T: JsTypes> EcDispatchHost<'a, T> {
    pub(crate) fn new(ec: &'a mut dyn ExecutionContext<T>) -> Self {
        Self { ec }
    }
}

impl<T: JsTypes + js_engine::JsTypesWithRealm> js_engine::EcmascriptHost<T>
    for EcDispatchHost<'_, T>
{
    fn get(
        &mut self,
        object: &T::JsObject,
        property: &str,
    ) -> js_engine::Completion<T::JsValue, T> {
        let key = self.ec.property_key_from_str(property);
        ExecutionContext::get(self.ec, object.clone(), key)
    }

    fn is_callable(&self, value: &T::JsValue) -> bool {
        self.ec.is_callable(value)
    }

    fn call(
        &mut self,
        callable: &T::JsObject,
        this_arg: &T::JsValue,
        args: &[T::JsValue],
    ) -> js_engine::Completion<T::JsValue, T> {
        self.ec.call(callable, this_arg, args)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> js_engine::Completion<(), T> {
        self.ec.perform_a_microtask_checkpoint()
    }

    fn report_exception(&mut self, error: T::JsValue) {
        self.ec.report_exception(error);
    }

    fn value_undefined(&mut self) -> T::JsValue {
        self.ec.value_undefined()
    }
    fn value_null(&mut self) -> T::JsValue {
        self.ec.value_null()
    }
    fn value_from_bool(&mut self, b: bool) -> T::JsValue {
        self.ec.value_from_bool(b)
    }
    fn value_from_number(&mut self, n: f64) -> T::JsValue {
        self.ec.value_from_number(n)
    }
    fn value_from_string(&mut self, s: T::JsString) -> T::JsValue {
        self.ec.value_from_string(s)
    }
    fn js_string_from_str(&self, s: &str) -> T::JsString {
        self.ec.js_string_from_str(s)
    }
}

impl EventDispatchHost for EcDispatchHost<'_, crate::js::Types> {
    fn ec(&mut self) -> &mut dyn ExecutionContext<crate::js::Types> {
        self.ec
    }

    fn create_event_object(&mut self, event: Event) -> Completion<JsObject, crate::js::Types> {
        create_interface_instance::<crate::js::Types, Event>(event, self.ec)
    }

    fn document_object(&mut self) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::document_object_ec(self.ec)
    }

    fn global_object(&mut self) -> JsObject {
        self.ec.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::resolve_element_object_ec(node_id, self.ec)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> Completion<JsObject, crate::js::Types> {
        crate::js::platform_objects::object_for_existing_node_ec(document, node_id, self.ec)
    }

    fn current_time_millis(&self) -> f64 {
        0.0
    }
}

fn current_event_target_object(this: &JsValue, context: &Context) -> JsObject {
    if let Some(object) = this.as_object() {
        return object.clone();
    }

    context.global_object()
}

fn current_event_target_object_ec(
    this: &JsValue,
    ec: &dyn ExecutionContext<crate::js::Types>,
) -> JsObject {
    if let Some(object) = <crate::js::Types as JsTypes>::value_as_object(this) {
        return object.clone();
    }

    ec.global_object()
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

pub(crate) fn flatten_ec(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<bool, crate::js::Types> {
    if let Some(boolean) = crate::js::Types::value_as_bool(options) {
        return Ok(boolean);
    }
    let Some(object) = crate::js::Types::value_as_object(options) else {
        return Ok(false);
    };
    let capture_key = ec.property_key_from_str("capture");
    let capture_val = ExecutionContext::get(ec, object, capture_key)?;
    Ok(ec.to_boolean(&capture_val))
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
            signal: None,
        });
    };
    let once = object.get(js_string!("once"), context)?.to_boolean();
    let passive = {
        if !object.has_property(js_string!("passive"), context)? {
            None
        } else {
            let value = object.get(js_string!("passive"), context)?;
            Some(value.to_boolean())
        }
    };
    let signal = if !object.has_property(js_string!("signal"), context)? {
        None
    } else {
        let value = object.get(js_string!("signal"), context)?;
        let signal = value.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("addEventListener signal must be an AbortSignal")
        })?;
        Some(
            signal
                .downcast_ref::<AbortSignal>()
                .map(|signal| signal.clone())
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("addEventListener signal must be an AbortSignal")
                })?,
        )
    };
    Ok(AddEventListenerOptions {
        capture,
        once,
        passive,
        signal,
    })
}

pub(crate) fn flatten_more_ec(
    options: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<AddEventListenerOptions, crate::js::Types> {
    let capture = flatten_ec(options, ec)?;
    let Some(object) = <crate::js::Types as JsTypes>::value_as_object(options) else {
        return Ok(AddEventListenerOptions {
            capture,
            once: false,
            passive: None,
            signal: None,
        });
    };
    let once_key = ec.property_key_from_str("once");
    let once_val = ExecutionContext::get(ec, object.clone(), once_key)?;
    let once = ec.to_boolean(&once_val);
    let passive = {
        let passive_key = ec.property_key_from_str("passive");
        let has_passive = ExecutionContext::has_property(ec, object.clone(), passive_key.clone())?;
        if !has_passive {
            None
        } else {
            let value = ExecutionContext::get(ec, object.clone(), passive_key)?;
            Some(ec.to_boolean(&value))
        }
    };
    let signal = {
        let signal_key = ec.property_key_from_str("signal");
        let has_signal = ExecutionContext::has_property(ec, object.clone(), signal_key.clone())?;
        if !has_signal {
            None
        } else {
            let value = ExecutionContext::get(ec, object.clone(), signal_key)?;
            let signal_obj =
                <crate::js::Types as JsTypes>::value_as_object(&value).ok_or_else(|| {
                    ec.new_type_error("addEventListener signal must be an AbortSignal")
                })?;
            Some(
                signal_obj
                    .downcast_ref::<AbortSignal>()
                    .map(|signal| signal.clone())
                    .ok_or_else(|| {
                        ec.new_type_error("addEventListener signal must be an AbortSignal")
                    })?,
            )
        }
    };
    Ok(AddEventListenerOptions {
        capture,
        once,
        passive,
        signal,
    })
}
