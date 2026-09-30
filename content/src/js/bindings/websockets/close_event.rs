use crate::js::Types;
use crate::js::bindings::initialization::init_flag;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use crate::webidl::unsigned_short;
use crate::websockets::{CloseEvent, CloseEventInit};
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{boolean_member, dictionary, string_member, string_value, this_as};

type JsValue = <Types as JsTypes>::JsValue;

fn close_event(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<CloseEvent, Types> {
    this_as::<CloseEvent>(this, "CloseEvent", ec)
}

impl WebIdlInterface<Types> for CloseEvent {
    const NAME: &'static str = "CloseEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        if args.is_empty() {
            return Err(ec.new_type_error(
                "CloseEvent constructor: 1 argument required, but only 0 present",
            ));
        }
        let type_ = ec.to_rust_string(args[0].clone())?;
        let undefined = ec.value_undefined();
        let init = args.get(1).cloned().unwrap_or(undefined);
        let dict = dictionary(
            Some(&init).filter(|value| !Types::value_is_undefined(value)),
            ec,
        )?;
        let code = match dict.get_member("code", ec)? {
            Some(value) => unsigned_short(&value, ec)?,
            None => 0,
        };
        Ok(CloseEvent::new(
            type_,
            CloseEventInit {
                bubbles: init_flag(&init, "bubbles", ec)?,
                cancelable: init_flag(&init, "cancelable", ec)?,
                composed: init_flag(&init, "composed", ec)?,
                was_clean: boolean_member(&dict, "wasClean", false, ec)?,
                code,
                reason: string_member(&dict, "reason", ec)?.unwrap_or_default(),
            },
            ec,
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [
            ("wasClean", was_clean as _),
            ("code", code as _),
            ("reason", reason as _),
        ] {
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

fn was_clean(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let was_clean = close_event(this, ec)?.was_clean();
    Ok(ec.value_from_bool(was_clean))
}

fn code(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let code = close_event(this, ec)?.code();
    Ok(ec.value_from_number(f64::from(code)))
}

fn reason(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let reason = close_event(this, ec)?.reason();
    Ok(string_value(&reason, ec))
}
