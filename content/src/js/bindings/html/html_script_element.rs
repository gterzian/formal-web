type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLScriptElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLScriptElement {
    const NAME: &'static str = "HTMLScriptElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        for (id, getter, setter) in [
            ("src", get_src as _, Some(set_src as _)),
            ("type", get_type as _, Some(set_type as _)),
            ("async", get_async as _, Some(set_async as _)),
            ("defer", get_defer as _, Some(set_defer as _)),
            ("noModule", get_no_module as _, Some(set_no_module as _)),
            ("text", get_text as _, None),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
                setter,
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

fn try_with_html_script_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLScriptElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLScriptElement receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj)
        && let Some(script) = data.downcast_ref::<HTMLScriptElement>()
    {
        return Ok(f(script));
    }
    Err(ec.new_type_error("receiver is not an HTMLScriptElement"))
}

fn string_argument(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<String, crate::js::Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.first().cloned().unwrap_or(undefined))
}

fn get_src(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document_url = super::hyperlink_element_utils::document_creation_url(ec)?;
    let src = try_with_html_script_element_ref(this, ec, |script| script.src(&document_url))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&src)))
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = string_argument(args, ec)?;
    try_with_html_script_element_ref(this, ec, |script| script.set_src(&value))?;
    Ok(ec.value_undefined())
}

fn get_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let type_ = try_with_html_script_element_ref(this, ec, |script| script.type_())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&type_)))
}

fn set_type(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = string_argument(args, ec)?;
    try_with_html_script_element_ref(this, ec, |script| script.set_type(&value))?;
    Ok(ec.value_undefined())
}

macro_rules! boolean_attribute {
    ($getter:ident, $setter:ident, $get:ident, $set:ident) => {
        fn $getter(
            this: &JsValue,
            _: &[JsValue],
            ec: &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types> {
            let value = try_with_html_script_element_ref(this, ec, |script| script.$get())?;
            Ok(ec.value_from_bool(value))
        }

        fn $setter(
            this: &JsValue,
            args: &[JsValue],
            ec: &mut dyn ExecutionContext<crate::js::Types>,
        ) -> Completion<JsValue, crate::js::Types> {
            let undefined = ec.value_undefined();
            let value = ec.to_boolean(args.first().unwrap_or(&undefined));
            try_with_html_script_element_ref(this, ec, |script| script.$set(value))?;
            Ok(ec.value_undefined())
        }
    };
}

boolean_attribute!(get_async, set_async, async_, set_async);
boolean_attribute!(get_defer, set_defer, defer, set_defer);
boolean_attribute!(get_no_module, set_no_module, no_module, set_no_module);

fn get_text(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let text = try_with_html_script_element_ref(this, ec, |script| script.text())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&text)))
}
