type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLLinkElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for HTMLLinkElement {
    const NAME: &'static str = "HTMLLinkElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        for (id, getter, setter) in [
            ("href", get_href as _, set_href as _),
            ("rel", get_rel as _, set_rel as _),
            ("type", get_type as _, set_type as _),
            ("as", get_as as _, set_as as _),
            ("media", get_media as _, set_media as _),
            ("hreflang", get_hreflang as _, set_hreflang as _),
            ("integrity", get_integrity as _, set_integrity as _),
            (
                "referrerPolicy",
                get_referrer_policy as _,
                set_referrer_policy as _,
            ),
            ("charset", get_charset as _, set_charset as _),
            ("crossOrigin", get_cross_origin as _, set_cross_origin as _),
            ("disabled", get_disabled as _, set_disabled as _),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
                setter: Some(setter),
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

fn link(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<HTMLLinkElement, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLLinkElement receiver is not an object"))?;
    ec.with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<HTMLLinkElement>().cloned())
        .ok_or_else(|| ec.new_type_error("receiver is not an HTMLLinkElement"))
}

fn string_argument(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<String, crate::js::Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.first().cloned().unwrap_or(undefined))
}

fn string_value(value: &str, ec: &mut dyn ExecutionContext<crate::js::Types>) -> JsValue {
    let string = ec.js_string_from_str(value);
    ec.value_from_string(string)
}

fn get_href(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let href = link(this, ec)?.href(ec);
    Ok(string_value(&href, ec))
}

fn set_href(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = string_argument(args, ec)?;
    link(this, ec)?.set_href(&value);
    Ok(ec.value_undefined())
}

fn get_rel(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let rel = link(this, ec)?.rel();
    Ok(string_value(&rel, ec))
}

fn set_rel(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = string_argument(args, ec)?;
    link(this, ec)?.set_rel(&value);
    Ok(ec.value_undefined())
}

macro_rules! reflected {
    ($($getter:ident, $setter:ident, $name:literal);* $(;)?) => {
        $(
            fn $getter(
                this: &JsValue,
                _args: &[JsValue],
                ec: &mut dyn ExecutionContext<crate::js::Types>,
            ) -> Completion<JsValue, crate::js::Types> {
                let value = link(this, ec)?.reflected($name);
                Ok(string_value(&value, ec))
            }

            fn $setter(
                this: &JsValue,
                args: &[JsValue],
                ec: &mut dyn ExecutionContext<crate::js::Types>,
            ) -> Completion<JsValue, crate::js::Types> {
                let value = string_argument(args, ec)?;
                link(this, ec)?.set_reflected($name, &value);
                Ok(ec.value_undefined())
            }
        )*
    };
}

reflected!(
    get_type, set_type, "type";
    get_as, set_as, "as";
    get_media, set_media, "media";
    get_hreflang, set_hreflang, "hreflang";
    get_integrity, set_integrity, "integrity";
    get_referrer_policy, set_referrer_policy, "referrerpolicy";
    get_charset, set_charset, "charset";
);

fn get_cross_origin(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    Ok(match link(this, ec)?.cross_origin() {
        Some(value) => string_value(&value, ec),
        None => ec.value_null(),
    })
}

fn set_cross_origin(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = args.first().unwrap_or(&undefined);
    let link = link(this, ec)?;
    if crate::js::Types::value_is_null(value) {
        link.set_cross_origin(None);
    } else {
        let value = ec.to_rust_string(value.clone())?;
        link.set_cross_origin(Some(&value));
    }
    Ok(ec.value_undefined())
}

fn get_disabled(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let disabled = link(this, ec)?.disabled();
    Ok(ec.value_from_bool(disabled))
}

fn set_disabled(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let disabled = ec.to_boolean(args.first().unwrap_or(&undefined));
    link(this, ec)?.set_disabled(disabled);
    Ok(ec.value_undefined())
}
