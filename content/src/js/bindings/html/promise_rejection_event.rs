use crate::html::{PromiseRejectionEvent, PromiseRejectionEventInit};
use crate::js::Types;
use crate::js::bindings::initialization::init_flag;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{dictionary, this_as};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for PromiseRejectionEvent {
    const NAME: &'static str = "PromiseRejectionEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn constructor_length() -> usize {
        2
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined.clone()))?;
        let init = args.get(1).cloned().unwrap_or(undefined.clone());
        let dict = dictionary(Some(&init), ec)?;
        let promise = dict
            .get_member("promise", ec)?
            .and_then(|value| {
                let object = Types::value_as_object(&value)?;
                Types::object_as_promise(&object).map(|_| object)
            })
            .ok_or_else(|| {
                ec.new_type_error(
                    "PromiseRejectionEventInit: member promise is required and must be a Promise",
                )
            })?;
        let reason = dict.get_member("reason", ec)?.unwrap_or(undefined);
        Ok(PromiseRejectionEvent::new(
            type_,
            PromiseRejectionEventInit {
                bubbles: init_flag(&init, "bubbles", ec)?,
                cancelable: init_flag(&init, "cancelable", ec)?,
                composed: init_flag(&init, "composed", ec)?,
                promise,
                reason,
            },
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [("promise", promise as _), ("reason", reason as _)] {
            def.add_attribute(AttributeDef {
                id,
                getter,
                setter: None,
                static_: false,
                unforgeable: false,
                promise_type: false,
                legacy_lenient_this: false,
                replaceable: false,
                put_forwards: None,
                legacy_lenient_setter: false,
                exposed: None,
            });
        }
    }
}

fn promise(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<PromiseRejectionEvent>(this, "PromiseRejectionEvent", ec)?;
    Ok(Types::value_from_object(event.promise.clone()))
}

fn reason(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let event = this_as::<PromiseRejectionEvent>(this, "PromiseRejectionEvent", ec)?;
    Ok(event.reason.borrow(ec).clone())
}
