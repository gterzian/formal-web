use crate::cssom_view::MediaQueryList;
use crate::js::Types;
use crate::js::bindings::{event_handlers, this_as};
use crate::webidl::bindings::{
    AttributeDef, BindingFn, InterfaceDefinition, OperationDef, WebIdlInterface,
};
use crate::webidl::{callback_interface_type_value, nullable_value};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for MediaQueryList {
    const NAME: &'static str = "MediaQueryList";

    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter, setter) in [
            ("media", get_media as BindingFn<Types>, None),
            ("matches", get_matches, None),
            (
                "onchange",
                get_onchange,
                Some(set_onchange as BindingFn<Types>),
            ),
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
        for (id, length, method) in [
            ("addListener", 1, add_listener as BindingFn<Types>),
            ("removeListener", 1, remove_listener),
        ] {
            def.add_operation(OperationDef {
                id,
                length,
                method,
                static_: false,
                unforgeable: false,
                promise_type: false,
                exposed: None,
            });
        }
    }
}

event_handlers! {
    get_onchange, set_onchange, "change";
}

fn media_query_list(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<MediaQueryList, Types> {
    this_as::<MediaQueryList>(this, "MediaQueryList", ec)
}

fn get_media(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let media = media_query_list(this, ec)?.media();
    Ok(ec.value_from_string(ec.js_string_from_str(&media)))
}

fn get_matches(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let matches = media_query_list(this, ec)?.matches();
    Ok(ec.value_from_bool(matches))
}

fn add_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let callback = nullable_value(
        args.first().unwrap_or(&undefined),
        ec,
        callback_interface_type_value,
    )?;
    media_query_list(this, ec)?.add_listener(callback, ec);
    Ok(ec.value_undefined())
}

fn remove_listener(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let callback = nullable_value(
        args.first().unwrap_or(&undefined),
        ec,
        callback_interface_type_value,
    )?;
    media_query_list(this, ec)?.remove_listener(callback, ec);
    Ok(ec.value_undefined())
}
