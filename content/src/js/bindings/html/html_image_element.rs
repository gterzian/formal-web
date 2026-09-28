type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::html::HTMLImageElement;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

type Getter = fn(
    &JsValue,
    &[JsValue],
    &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types>;

type Setter = fn(
    &JsValue,
    &[JsValue],
    &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types>;

impl WebIdlInterface<crate::js::Types> for HTMLImageElement {
    const NAME: &'static str = "HTMLImageElement";

    fn parent_name() -> Option<&'static str> {
        Some("HTMLElement")
    }

    fn create_platform_object(
        _new_target: &JsValue,
        _args: &[JsValue],
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<Self, crate::js::Types> {
        Err(ec.new_type_error("Illegal constructor"))
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        attribute(def, "alt", get_alt, Some(set_alt));
        attribute(def, "src", get_src, Some(set_src));
        attribute(def, "srcset", get_srcset, Some(set_srcset));
        attribute(def, "sizes", get_sizes, Some(set_sizes));
        attribute(def, "crossOrigin", get_cross_origin, Some(set_cross_origin));
        attribute(def, "useMap", get_use_map, Some(set_use_map));
        attribute(def, "isMap", get_is_map, Some(set_is_map));
        attribute(def, "controls", get_controls, Some(set_controls));
        attribute(def, "width", get_width, Some(set_width));
        attribute(def, "height", get_height, Some(set_height));
        attribute(def, "naturalWidth", get_natural_width, None);
        attribute(def, "naturalHeight", get_natural_height, None);
        attribute(def, "complete", get_complete, None);
        attribute(def, "currentSrc", get_current_src, None);
        attribute(
            def,
            "referrerPolicy",
            get_referrer_policy,
            Some(set_referrer_policy),
        );
        attribute(def, "decoding", get_decoding, Some(set_decoding));
        attribute(def, "loading", get_loading, Some(set_loading));
        attribute(
            def,
            "fetchPriority",
            get_fetch_priority,
            Some(set_fetch_priority),
        );
        def.add_operation(OperationDef {
            id: "decode",
            length: 0,
            method: decode_method,
            static_: false,
            unforgeable: false,
            promise_type: true,
            exposed: None,
        });
    }
}

fn attribute(
    def: &mut InterfaceDefinition<crate::js::Types>,
    id: &'static str,
    getter: Getter,
    setter: Option<Setter>,
) {
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

fn try_with_image_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLImageElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    if let Some(data) = ec.with_object_any(&obj)
        && let Some(image) = data.downcast_ref::<HTMLImageElement>()
    {
        return Ok(f(image));
    }
    Err(ec.new_type_error("expected HTMLImageElement"))
}

fn get_alt(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::alt)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_alt(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_alt(&value))?;
    Ok(ec.value_undefined())
}

fn get_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let base_url = super::hyperlink_element_utils::document_creation_url(ec)?;
    let value = try_with_image_ref(this, ec, |image| image.src(&base_url))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_src(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_src(&value))?;
    Ok(ec.value_undefined())
}

fn get_srcset(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::srcset)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_srcset(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_srcset(&value))?;
    Ok(ec.value_undefined())
}

fn get_sizes(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::sizes)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_sizes(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_sizes(&value))?;
    Ok(ec.value_undefined())
}

fn get_cross_origin(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::cross_origin)?;
    match value {
        Some(value) => Ok(ec.value_from_string(ec.js_string_from_str(&value))),
        None => Ok(ec.value_null()),
    }
}

fn set_cross_origin(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let argument = args.first().cloned().unwrap_or(undefined);
    let value = if crate::js::Types::value_is_null(&argument)
        || crate::js::Types::value_is_undefined(&argument)
    {
        None
    } else {
        Some(ec.to_rust_string(argument)?)
    };
    try_with_image_ref(this, ec, |image| image.set_cross_origin(value.as_deref()))?;
    Ok(ec.value_undefined())
}

fn get_use_map(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::use_map)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_use_map(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_use_map(&value))?;
    Ok(ec.value_undefined())
}

fn get_is_map(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::is_map)?;
    Ok(ec.value_from_bool(value))
}

fn set_is_map(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().is_some_and(|value| ec.to_boolean(value));
    try_with_image_ref(this, ec, |image| image.set_is_map(value))?;
    Ok(ec.value_undefined())
}

fn get_controls(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::controls)?;
    Ok(ec.value_from_bool(value))
}

fn set_controls(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = args.first().is_some_and(|value| ec.to_boolean(value));
    try_with_image_ref(this, ec, |image| image.set_controls(value))?;
    Ok(ec.value_undefined())
}

fn get_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::width)?;
    Ok(ec.value_from_number(f64::from(value)))
}

fn set_width(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_width(value))?;
    Ok(ec.value_undefined())
}

fn get_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::height)?;
    Ok(ec.value_from_number(f64::from(value)))
}

fn set_height(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_height(value))?;
    Ok(ec.value_undefined())
}

fn get_natural_width(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::natural_width)?;
    Ok(ec.value_from_number(f64::from(value)))
}

fn get_natural_height(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::natural_height)?;
    Ok(ec.value_from_number(f64::from(value)))
}

fn get_complete(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::complete)?;
    Ok(ec.value_from_bool(value))
}

fn get_current_src(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let base_url = super::hyperlink_element_utils::document_creation_url(ec)?;
    let value = try_with_image_ref(this, ec, |image| image.current_src(&base_url))?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn get_referrer_policy(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::referrer_policy)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_referrer_policy(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_referrer_policy(&value))?;
    Ok(ec.value_undefined())
}

fn get_decoding(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::decoding)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_decoding(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_decoding(&value))?;
    Ok(ec.value_undefined())
}

fn get_loading(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::loading)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_loading(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_loading(&value))?;
    Ok(ec.value_undefined())
}

fn get_fetch_priority(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value = try_with_image_ref(this, ec, HTMLImageElement::fetch_priority)?;
    Ok(ec.value_from_string(ec.js_string_from_str(&value)))
}

fn set_fetch_priority(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_image_ref(this, ec, |image| image.set_fetch_priority(&value))?;
    Ok(ec.value_undefined())
}

fn decode_method(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    // Clone the image out of the object registry so the domain method can
    // use `ec` (it creates the promise, and on failure a DOMException).
    let image = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<HTMLImageElement>().cloned())
        .ok_or_else(|| ec.new_type_error("expected HTMLImageElement"))?;
    image.decode(ec)
}
