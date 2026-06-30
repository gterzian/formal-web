use std::marker::PhantomData;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue, js_string,
    object::{JsObject, builtins::JsFunction},
};

use crate::dom::{AbortSignal, Event, EventDispatchHost, EventTarget, dispatch};
use crate::js::downcast::try_with_event_target_mut;
use crate::js::platform_objects::{
    document_object, object_for_existing_node, resolve_element_object,
};
use crate::webidl::{callback_interface_type_value, nullable_value};

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
    // Note: keeps ec_to_ctx — flatten, flatten_more,
    // current_event_target_object all need Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let event_target = current_event_target_object(this, ctx);
    let type_ = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let options = flatten_more(args.get_or_undefined(2), ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let callback =
        nullable_value(args.get_or_undefined(1), callback_interface_type_value)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let receiver = JsValue::from(event_target.clone());

    let ec_ref = js_engine::boa::context_as_ec(ctx);
    try_with_event_target_mut(&receiver, ec_ref, |target| {
        target.add_event_listener(
            &event_target,
            type_,
            callback,
            options.capture,
            options.once,
            options.passive,
            options.signal,
        )
    })?
    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;

    Ok(JsValue::undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — flatten and current_event_target_object
    // need Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let event_target = current_event_target_object(this, ctx);
    let type_ = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let Some(callback) =
        nullable_value(args.get_or_undefined(1), callback_interface_type_value)
            .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
    else {
        return Ok(JsValue::undefined());
    };
    let capture = flatten(args.get_or_undefined(2), ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let receiver = JsValue::from(event_target);

    let ec_ref = js_engine::boa::context_as_ec(ctx);
    try_with_event_target_mut(&receiver, ec_ref, |target| {
        target.remove_event_listener_entry(&type_, &callback, capture);
    })?;

    Ok(JsValue::undefined())
}

fn dispatch_event(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let event_obj = match <crate::js::Types as JsTypes>::value_as_object(&args.get_or_undefined(0)) {
        Some(obj) => obj,
        None => return Err(ec.new_type_error("dispatchEvent requires an Event")),
    };
    // Note: keeps ec_to_ctx — ContextEventDispatchHost needs &mut Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let target = current_event_target_object(this, ctx);
    let mut host = ContextEventDispatchHost::new(ctx);
    let canceled = dispatch(&mut host, &target, &event_obj, false)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    Ok(JsValue::from(!canceled))
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: This helper keeps the DOM-specific event object and target resolution for `dispatch`, while delegating generic ECMAScript callback operations through `EcmascriptHost<crate::js::Types>`.
pub(crate) struct ContextEventDispatchHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEventDispatchHost<'a> {
    pub(crate) fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl js_engine::EcmascriptHost<crate::js::Types> for ContextEventDispatchHost<'_> {
    fn get(
        &mut self,
        object: &JsObject,
        property: &str,
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        object
            .get(js_string!(property), self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn is_callable(&self, value: &JsValue) -> bool {
        value.as_object().is_some_and(|o| o.is_callable())
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> js_engine::Completion<JsValue, crate::js::Types> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsValue::from(
                JsNativeError::typ()
                    .with_message("callback is not callable")
                    .into_opaque(self.context),
            )
        })?;
        function
            .call(this_arg, args, self.context)
            .map_err(|e| e.into_opaque(self.context).unwrap_or(JsValue::undefined()))
    }

    fn perform_a_microtask_checkpoint(&mut self) -> js_engine::Completion<(), crate::js::Types> {
        let _ = self.context.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsValue) {
        log::error!("uncaught event listener error: {error:?}");
    }

    fn value_undefined(&mut self) -> JsValue {
        JsValue::undefined()
    }
    fn value_null(&mut self) -> JsValue {
        JsValue::null()
    }
    fn value_from_bool(&mut self, b: bool) -> JsValue {
        JsValue::from(b)
    }
    fn value_from_number(&mut self, n: f64) -> JsValue {
        JsValue::from(n)
    }
    fn value_from_string(&mut self, s: boa_engine::JsString) -> JsValue {
        JsValue::from(s)
    }
    fn js_string_from_str(&self, s: &str) -> boa_engine::JsString {
        boa_engine::js_string!(s)
    }
}

impl EventDispatchHost for ContextEventDispatchHost<'_> {
    fn ec(&mut self) -> &mut dyn ExecutionContext<crate::js::Types> {
        js_engine::boa::context_as_ec(self.context)
    }

    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        create_interface_instance::<crate::js::Types, Event>(
            event,
            js_engine::boa::context_as_ec(self.context),
        )
        .map_err(JsError::from_opaque)
    }

    fn document_object(&mut self) -> JsResult<JsObject> {
        document_object(self.context)
    }

    fn global_object(&mut self) -> JsObject {
        self.context.global_object()
    }

    fn resolve_element_object(&mut self, node_id: usize) -> JsResult<JsObject> {
        resolve_element_object(node_id, self.context)
    }

    fn resolve_existing_node_object(
        &mut self,
        document: Rc<RefCell<BaseDocument>>,
        node_id: usize,
    ) -> JsResult<JsObject> {
        object_for_existing_node(document, node_id, self.context)
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
