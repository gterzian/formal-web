use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    js_string,
};

use crate::dom::{Event, UIEvent};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, WebIdlInterface,
};

use super::event::init_flag;

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for UIEvent {
    const NAME: &'static str = "UIEvent";

    fn parent_name() -> Option<&'static str> {
        Some("Event")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<Self> {
        let type_ = args
            .get_or_undefined(0)
            .to_string(context)?
            .to_std_string_escaped();
        let init = args.get_or_undefined(1);
        let detail = if let Some(object) = init.as_object() {
            object.get(js_string!("detail"), context)?.to_i32(context)?
        } else {
            0
        };
        Ok(UIEvent {
            event: Event::new(
                type_,
                init_flag(init, js_string!("bubbles"), context)?,
                init_flag(init, js_string!("cancelable"), context)?,
                init_flag(init, js_string!("composed"), context)?,
                false,
                0.0,
            ),
            view: None,
            detail,
        })
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(AttributeDef {
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

fn get_view(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
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
}

fn get_detail(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("UIEvent receiver is not an object"))?;
    with_ui_event_ref(&object, |ui_event| JsValue::from(ui_event.detail_value()))
}
