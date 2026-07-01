use boa_engine::JsValue;
use std::marker::PhantomData;

use crate::dom::{Event, UIEvent};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use super::event::init_flag;

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for UIEvent {
    const NAME: &'static str = "UIEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        let undefined = ec.value_undefined();
        let type_ = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
        let init = args.get(1).cloned().unwrap_or(ec.value_undefined());
        let detail = if let Some(object) = crate::js::Types::value_as_object(&init) {
            let property_key = ec.property_key_from_str("detail");
            let detail_value = ExecutionContext::get(ec, object, property_key)?;
            ec.to_number(detail_value)? as i32
        } else {
            0
        };
        Ok(UIEvent {
            event: Event::new(
                type_,
                init_flag(&init, "bubbles", ec)?,
                init_flag(&init, "cancelable", ec)?,
                init_flag(&init, "composed", ec)?,
                false,
                0.0,
            ),
            view: None,
            detail,
        })
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "view",
            getter: get_view,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "detail",
            getter: get_detail,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

// ── Member getters/setters/methods ──

fn get_view(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("UIEvent receiver is not an object"))?;
    let ui_event = obj
        .downcast_ref::<UIEvent>()
        .ok_or_else(|| ec.new_type_error("receiver is not a UIEvent"))?;
    Ok(ui_event
        .view_value()
        .clone()
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null()))
}

fn get_detail(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("UIEvent receiver is not an object"))?;
    let ui_event = obj
        .downcast_ref::<UIEvent>()
        .ok_or_else(|| ec.new_type_error("receiver is not a UIEvent"))?;
    Ok(ec.value_from_number(ui_event.detail_value() as f64))
}
