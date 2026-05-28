use std::collections::BTreeMap;

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::{JsObject, ObjectInitializer},
    property::Attribute,
};

use crate::dom::Element;
use crate::html::{HTMLAnchorElement, HTMLIFrameElement, HTMLElement};

use crate::boa::bindings::dom::{
    register_element_methods, register_event_target_methods, register_node_methods,
};

impl Class for HTMLElement {
    const NAME: &'static str = "HTMLElement";

    fn data_constructor(
        _this: &JsValue,
        _args: &[JsValue],
        _context: &mut Context,
    ) -> JsResult<Self> {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    }

    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        register_event_target_methods(class)?;
        register_node_methods(class)?;
        register_element_methods(class)?;
        register_html_element_methods(class)
    }
}

fn with_html_element_ref<R>(this: &JsValue, f: impl FnOnce(&HTMLElement) -> R) -> JsResult<R> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("HTMLElement receiver is not an object")
    })?;

    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(f(&html_element));
    }

    if let Some(anchor) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&anchor.html_element));
    }

    if let Some(iframe) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&iframe.html_element));
    }

    Err(JsNativeError::typ()
        .with_message("receiver is not an HTMLElement")
        .into())
}

pub(crate) fn register_html_element_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .accessor(
            js_string!("title"),
            Some(NativeFunction::from_fn_ptr(get_title).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_title).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("lang"),
            Some(NativeFunction::from_fn_ptr(get_lang).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_lang).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("dir"),
            Some(NativeFunction::from_fn_ptr(get_dir).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_dir).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("hidden"),
            Some(NativeFunction::from_fn_ptr(get_hidden).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_hidden).to_js_function(&realm)),
            Attribute::all(),
        )
        .accessor(
            js_string!("style"),
            Some(NativeFunction::from_fn_ptr(get_style).to_js_function(&realm)),
            None,
            Attribute::all(),
        );
    Ok(())
}

fn get_title(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.title()))
    })
}

fn set_title(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let title = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_title(&title))?;
    Ok(JsValue::undefined())
}

fn get_lang(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.lang()))
    })
}

fn set_lang(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let lang = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_lang(&lang))?;
    Ok(JsValue::undefined())
}

fn get_dir(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.dir()))
    })
}

fn set_dir(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let dir = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_dir(&dir))?;
    Ok(JsValue::undefined())
}

fn get_hidden(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| JsValue::from(html_element.hidden()))
}

fn set_hidden(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let hidden = args.get_or_undefined(0).to_boolean();
    with_html_element_ref(this, |html_element| html_element.set_hidden(hidden))?;
    Ok(JsValue::undefined())
}

fn get_style(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        inline_style_object_for_element(&html_element.element, context).map(JsValue::from)
    })?
}

pub(crate) fn computed_style_object_for_element(
    element: &Element,
    context: &mut Context,
) -> JsResult<JsObject> {
    style_declaration_object(&computed_style_properties_for_element(element), context)
}

fn inline_style_object_for_element(
    element: &Element,
    context: &mut Context,
) -> JsResult<JsObject> {
    style_declaration_object(&inline_style_properties_for_element(element), context)
}

fn computed_style_properties_for_element(element: &Element) -> BTreeMap<String, String> {
    let mut properties = inline_style_properties_for_element(element);
    let metrics = element.box_metrics().unwrap_or_default();

    properties.insert(
        String::from("display"),
        if element.has_attribute("hidden") {
            String::from("none")
        } else {
            properties
                .get("display")
                .cloned()
                .unwrap_or_else(|| default_display_for_tag_name(&element.tag_name()).to_owned())
        },
    );

    if !properties.contains_key("visibility") {
        properties.insert(String::from("visibility"), String::from("visible"));
    }
    if !properties.contains_key("opacity") {
        properties.insert(String::from("opacity"), String::from("1"));
    }
    if !properties.contains_key("transform") {
        properties.insert(String::from("transform"), String::from("none"));
    }
    if !properties.contains_key("pointer-events") {
        properties.insert(String::from("pointer-events"), String::from("auto"));
    }
    if !properties.contains_key("position") {
        properties.insert(String::from("position"), String::from("static"));
    }
    if !properties.contains_key("white-space") {
        properties.insert(String::from("white-space"), String::from("normal"));
    }
    if !properties.contains_key("cursor") {
        properties.insert(String::from("cursor"), String::from("auto"));
    }
    if !properties.contains_key("content") {
        properties.insert(String::from("content"), String::from("none"));
    }

    if !properties.contains_key("overflow") {
        properties.insert(String::from("overflow"), String::from("visible"));
    }
    let overflow = properties
        .get("overflow")
        .cloned()
        .unwrap_or_else(|| String::from("visible"));
    if !properties.contains_key("overflow-x") {
        properties.insert(String::from("overflow-x"), overflow.clone());
    }
    if !properties.contains_key("overflow-y") {
        properties.insert(String::from("overflow-y"), overflow);
    }

    properties
        .entry(String::from("border-top-width"))
        .or_insert_with(|| format_css_px(metrics.border_top));
    properties
        .entry(String::from("border-right-width"))
        .or_insert_with(|| format_css_px(metrics.border_right));
    properties
        .entry(String::from("border-bottom-width"))
        .or_insert_with(|| format_css_px(metrics.border_bottom));
    properties
        .entry(String::from("border-left-width"))
        .or_insert_with(|| format_css_px(metrics.border_left));
    properties
        .entry(String::from("padding-top"))
        .or_insert_with(|| format_css_px(metrics.padding_top));
    properties
        .entry(String::from("padding-right"))
        .or_insert_with(|| format_css_px(metrics.padding_right));
    properties
        .entry(String::from("padding-bottom"))
        .or_insert_with(|| format_css_px(metrics.padding_bottom));
    properties
        .entry(String::from("padding-left"))
        .or_insert_with(|| format_css_px(metrics.padding_left));
    properties
        .entry(String::from("margin-top"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-right"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-bottom"))
        .or_insert_with(|| String::from("0px"));
    properties
        .entry(String::from("margin-left"))
        .or_insert_with(|| String::from("0px"));

    properties
}

fn inline_style_properties_for_element(element: &Element) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    let Some(style_attribute) = element.get_attribute("style") else {
        return properties;
    };

    for declaration in style_attribute.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let normalized_name = name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() {
            continue;
        }
        properties.insert(normalized_name, value.trim().to_owned());
    }

    properties
}

fn style_declaration_object(
    properties: &BTreeMap<String, String>,
    context: &mut Context,
) -> JsResult<JsObject> {
    let mut initializer = ObjectInitializer::new(context);
    for (name, value) in properties {
        let value = JsValue::from(JsString::from(value.as_str()));
        initializer.property(JsString::from(name.as_str()), value.clone(), Attribute::all());

        let alias = camel_case_property_name(name);
        if alias != *name {
            initializer.property(JsString::from(alias.as_str()), value, Attribute::all());
        }
    }
    initializer.function(
        NativeFunction::from_fn_ptr(get_style_property_value),
        js_string!("getPropertyValue"),
        1,
    );
    Ok(initializer.build())
}

fn get_style_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property_name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .trim()
        .to_ascii_lowercase();
    let Some(object) = this.as_object() else {
        return Ok(JsValue::from(JsString::from("")));
    };
    let value = object.get(JsString::from(property_name.as_str()), context)?;
    if value.is_undefined() {
        return Ok(JsValue::from(JsString::from("")));
    }
    Ok(value)
}

fn camel_case_property_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut uppercase_next = false;
    for ch in name.chars() {
        if ch == '-' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            result.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn format_css_px(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}px")
    } else {
        format!("{value}px")
    }
}

fn default_display_for_tag_name(tag_name: &str) -> &'static str {
    match tag_name {
        "BODY" | "DIV" | "FORM" | "H1" | "H2" | "H3" | "H4" | "H5" | "H6" | "HEADER"
        | "HTML" | "IFRAME" | "LI" | "MAIN" | "NAV" | "OL" | "P" | "SECTION" | "TABLE"
        | "UL" => "block",
        "BUTTON" => "inline-block",
        _ => "inline",
    }
}
