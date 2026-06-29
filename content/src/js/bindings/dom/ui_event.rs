use boa_engine::{JsArgs, JsResult, JsValue, js_string};
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
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
            let type_ = args
                .get_or_undefined(0)
                .to_string(ctx)?
                .to_std_string_escaped();
            let init = args.get_or_undefined(1);
            let detail = if let Some(object) = init.as_object() {
                object.get(js_string!("detail"), ctx)?.to_i32(ctx)?
            } else {
                0
            };
            Ok(UIEvent {
                event: Event::new(
                    type_,
                    init_flag(init, js_string!("bubbles"), ctx)?,
                    init_flag(init, js_string!("cancelable"), ctx)?,
                    init_flag(init, js_string!("composed"), ctx)?,
                    false,
                    0.0,
                ),
                view: None,
                detail,
            })
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
        .map(JsValue::from)
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
    Ok(JsValue::from(ui_event.detail_value()))
}
