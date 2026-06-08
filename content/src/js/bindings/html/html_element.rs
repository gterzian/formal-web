use std::collections::BTreeMap;

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, ObjectInitializer},
    property::Attribute,
};

use crate::dom::Element;
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, inline_style_properties_for_element,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, WebIdlInterface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for HTMLElement {
    const NAME: &'static str = "HTMLElement";

    fn parent_name() -> Option<&'static str> {
        Some("Element")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(AttributeDef {
            id: "title",
            getter: get_title,
            setter: Some(set_title),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "lang",
            getter: get_lang,
            setter: Some(set_lang),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "dir",
            getter: get_dir,
            setter: Some(set_dir),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "hidden",
            getter: get_hidden,
            setter: Some(set_hidden),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "style",
            getter: get_style,
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

fn inline_style_object_for_element(element: &Element, context: &mut Context) -> JsResult<JsObject> {
    style_declaration_object(&inline_style_properties_for_element(element), context)
}

pub(crate) fn style_declaration_object(
    properties: &BTreeMap<String, String>,
    context: &mut Context,
) -> JsResult<JsObject> {
    let mut initializer = ObjectInitializer::new(context);
    for (name, value) in properties {
        let value = JsValue::from(JsString::from(value.as_str()));
        initializer.property(
            JsString::from(name.as_str()),
            value.clone(),
            Attribute::all(),
        );

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
    // Step 1.1 of CSSStyleDeclaration.getPropertyValue(property): if property is not a custom
    // property, convert it to ASCII lowercase.
    let property_name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .trim()
        .to_ascii_lowercase();

    // Step 2: "If property is a case-sensitive match for a property name of a CSS declaration in
    // the declarations, then return the result of invoking serialize a CSS value of that
    // declaration."
    let Some(object) = this.as_object() else {
        return Ok(JsValue::from(JsString::from("")));
    };
    let value = object.get(JsString::from(property_name.as_str()), context)?;

    // Step 3: "Return the empty string."
    // Note: This snapshot object currently exposes directly materialized longhand values only, so
    // shorthand serialization still falls through to the empty string.
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
