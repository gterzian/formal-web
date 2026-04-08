use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    property::Attribute,
};

use crate::dom::{Event, UIEvent};

use super::event::{init_flag, register_event_methods};

impl Class for UIEvent {
    const NAME: &'static str = "UIEvent";
    const LENGTH: usize = 1;

    fn data_constructor(
        _this: &JsValue,
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

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_methods(class)?;
        let realm = class.context().realm().clone();
        class
            .accessor(
                js_string!("view"),
                Some(NativeFunction::from_fn_ptr(get_view).to_js_function(&realm)),
                None,
                Attribute::all(),
            )
            .accessor(
                js_string!("detail"),
                Some(NativeFunction::from_fn_ptr(get_detail).to_js_function(&realm)),
                None,
                Attribute::all(),
            );
        Ok(())
    }
}

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
