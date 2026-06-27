use std::marker::PhantomData;
use boa_engine::{js_string, Context, JsArgs, JsNativeError, JsResult, JsValue};

use crate::dom::{Event, UIEvent};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use super::event::init_flag;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for UIEvent {
    const NAME: &'static str = "UIEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
        let type_ = args.get_or_undefined(0).to_string(ctx)?.to_std_string_escaped();
        let init = args.get_or_undefined(1);
        let detail = if let Some(object) = init.as_object() {
            object.get(js_string!("detail"), ctx)?.to_i32(ctx)?
        } else { 0 };
        Ok(UIEvent {
            event: Event::new(type_, init_flag(init, js_string!("bubbles"), ctx)?,
                init_flag(init, js_string!("cancelable"), ctx)?,
                init_flag(init, js_string!("composed"), ctx)?, false, 0.0),
            view: None, detail,
        })
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
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

pub(crate) fn with_ui_event_ref<R>(
    this: &boa_engine::object::JsObject,
    f: impl FnOnce(&UIEvent) -> R,
) -> JsResult<R> {
    let Some(ui_event) = this.downcast_ref::<UIEvent>() else {
        return Err(JsNativeError::typ()
            .with_message("receiver is not a UIEvent")
            .into());
    };
    Ok(f(&ui_event))
}

fn get_view(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("UIEvent receiver is not an object"))?;
    with_ui_event_ref(&object, |ui_event| {
        ui_event
            .view_value()
            .clone()
            .map(JsValue::from)
            .unwrap_or_else(JsValue::null)
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_detail(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("UIEvent receiver is not an object"))?;
    with_ui_event_ref(&object, |ui_event| JsValue::from(ui_event.detail_value()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}
