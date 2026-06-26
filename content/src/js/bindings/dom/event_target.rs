use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    js_string,
    object::{builtins::JsFunction, JsObject},
    Context, JsArgs, JsNativeError, JsResult, JsValue,
};

use crate::dom::{dispatch, AbortSignal, Event, EventDispatchHost, EventTarget};
use crate::js::platform_objects::{
    document_object, object_for_existing_node, resolve_element_object,
};
use crate::js::with_event_target_mut;
use crate::webidl::{callback_interface_type_value, nullable_value};

#[derive(Clone)]
pub(crate) struct AddEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: Option<bool>,
    pub signal: Option<AbortSignal>,
}

// ── WebIDL interface definition (§3) ──

use crate::webidl::bindings::{
    create_interface_instance, InterfaceDefinition, OperationDef, WebIdlInterface,
};

impl WebIdlInterface for EventTarget {
    const NAME: &'static str = "EventTarget";

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Ok(EventTarget::default())
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_operation(OperationDef {
            id: "addEventListener",
            length: 3,
            method: add_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "removeEventListener",
            length: 3,
            method: remove_event_listener,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
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
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_target = current_event_target_object(this, context);
    let type_ = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let options = flatten_more(args.get_or_undefined(2), context)?;
    let callback = nullable_value(args.get_or_undefined(1), callback_interface_type_value)?;
    let receiver = JsValue::from(event_target.clone());

    with_event_target_mut(&receiver, |target| {
        target.add_event_listener(
            &event_target,
            type_,
            callback,
            options.capture,
            options.once,
            options.passive,
            options.signal,
        )
    })??;

    Ok(JsValue::undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_target = current_event_target_object(this, context);
    let type_ = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let Some(callback) = nullable_value(args.get_or_undefined(1), callback_interface_type_value)?
    else {
        return Ok(JsValue::undefined());
    };
    let capture = flatten(args.get_or_undefined(2), context)?;
    let receiver = JsValue::from(event_target);

    with_event_target_mut(&receiver, |target| {
        target.remove_event_listener_entry(&type_, &callback, capture);
    })?;

    Ok(JsValue::undefined())
}

fn dispatch_event(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let target = current_event_target_object(this, context);
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
    let mut host = ContextEventDispatchHost::new(context);
    dispatch(&mut host, target, event, false)
}

/// <https://dom.spec.whatwg.org/#concept-event-dispatch>
// Note: This helper keeps the DOM-specific event object and target resolution for `dispatch`, while delegating generic ECMAScript callback operations through `EcmascriptHost<BoaTypes>`.
pub(crate) struct ContextEventDispatchHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEventDispatchHost<'a> {
    pub(crate) fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl js_engine::EcmascriptHost<js_engine::boa::BoaTypes> for ContextEventDispatchHost<'_> {
    fn get(
        &mut self,
        object: &JsObject,
        property: &str,
    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
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
    ) -> js_engine::Completion<JsValue, js_engine::boa::BoaTypes> {
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

    fn perform_a_microtask_checkpoint(
        &mut self,
    ) -> js_engine::Completion<(), js_engine::boa::BoaTypes> {
        let _ = self.context.run_jobs();
        Ok(())
    }

    fn report_exception(&mut self, error: JsValue) {
        log::error!("uncaught event listener error: {error:?}");
    }
}

impl EventDispatchHost for ContextEventDispatchHost<'_> {
    fn context(&mut self) -> &mut Context {
        self.context
    }

    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        create_interface_instance::<Event>(event, self.context)
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
