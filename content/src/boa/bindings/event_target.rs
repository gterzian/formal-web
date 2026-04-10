use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::{JsObject, builtins::JsFunction},
};

use crate::boa::platform_objects::{
    document_object, object_for_existing_node, resolve_element_object,
};
use crate::dom::{
    Event, EventDispatchHost, EventTarget, dispatch, with_event_target_mut,
};
use crate::webidl::{EcmascriptHost, callback_interface_value};

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
    let mut host = ContextEventDispatchHost::new(context);
    dispatch(&mut host, target, event, false)
}

struct ContextEventDispatchHost<'a> {
    context: &'a mut Context,
}

impl<'a> ContextEventDispatchHost<'a> {
    fn new(context: &'a mut Context) -> Self {
        Self { context }
    }
}

impl EcmascriptHost for ContextEventDispatchHost<'_> {
    fn get(&mut self, object: &JsObject, property: &str) -> JsResult<JsValue> {
        object.get(JsString::from(property), self.context)
    }

    fn is_callable(&self, object: &JsObject) -> bool {
        object.is_callable()
    }

    fn call(
        &mut self,
        callable: &JsObject,
        this_arg: &JsValue,
        args: &[JsValue],
    ) -> JsResult<JsValue> {
        let function = JsFunction::from_object(callable.clone()).ok_or_else(|| {
            JsError::from(JsNativeError::typ().with_message("callback is not callable"))
        })?;
        function.call(this_arg, args, self.context)
    }

    fn perform_a_microtask_checkpoint(&mut self) -> JsResult<()> {
        self.context.run_jobs()
    }

    fn report_exception(&mut self, error: JsError, _callback: &JsObject) {
        eprintln!("uncaught event listener error: {error}");
    }
}

impl EventDispatchHost for ContextEventDispatchHost<'_> {
    fn create_event_object(&mut self, event: Event) -> JsResult<JsObject> {
        Event::from_data(event, self.context)
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
