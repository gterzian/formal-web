use std::marker::PhantomData;
use std::collections::BTreeMap;

use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, JsObject, ObjectInitializer},
    property::Attribute,
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
};

use crate::dom::Element;
use crate::html::{
    inline_style_properties_for_element, HTMLAnchorElement, HTMLElement, HTMLIFrameElement,
    HTMLInputElement, HTMLMediaElement, HTMLVideoElement,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<js_engine::boa::BoaTypes> for HTMLElement {
    const NAME: &'static str = "HTMLElement";

    fn parent_name() -> Option<&'static str> {
        Some("Element")
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
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
            _phantom: PhantomData,
        
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
            _phantom: PhantomData,
        
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
            _phantom: PhantomData,
        
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
            _phantom: PhantomData,
        
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

    if let Some(input) = object.downcast_ref::<HTMLInputElement>() {
        return Ok(f(&input.html_element));
    }

    if let Some(iframe) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&iframe.html_element));
    }

    Err(JsNativeError::typ()
        .with_message("receiver is not an HTMLElement")
        .into())
}

fn get_title(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.title()))
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_title(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let title = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_title(&title))?;
    Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_lang(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.lang()))
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_lang(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let lang = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_lang(&lang))?;
    Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_dir(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| {
        JsValue::from(JsString::from(html_element.dir()))
    })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_dir(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let dir = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    with_html_element_ref(this, |html_element| html_element.set_dir(&dir))?;
    Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_hidden(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    with_html_element_ref(this, |html_element| JsValue::from(html_element.hidden()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_hidden(this: &JsValue, args: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let hidden = args.get_or_undefined(0).to_boolean();
    with_html_element_ref(this, |html_element| html_element.set_hidden(hidden))?;
    Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_style(this: &JsValue, _: &[JsValue], ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("style getter: receiver is not an object")
    })?;
    let element_ref = JsValue::from(object.clone());
    let realm = ctx.realm().clone();

    // Build the style declaration object with a reference to the element,
    // so that cssText and individual property setters can write back.
    let properties = with_html_element_ref(this, |html_element| {
        inline_style_properties_for_element(&html_element.element)
    })?;

    let mut initializer = ObjectInitializer::new(ctx);
    for (name, value) in &properties {
        // cssText is handled separately; skip it here to avoid conflict.
        if name == "cssText" {
            continue;
        }
        let js_value = JsValue::from(JsString::from(value.as_str()));
        initializer.property(
            JsString::from(name.as_str()),
            js_value.clone(),
            Attribute::all(),
        );
        let alias = camel_case_property_name(name);
        if alias != *name {
            initializer.property(JsString::from(alias.as_str()), js_value, Attribute::all());
        }
    }

    initializer.function(
        NativeFunction::from_fn_ptr(get_style_property_value),
        js_string!("getPropertyValue"),
        1,
    );

    let style_obj = initializer.build();

    // Store a reference to the element so cssText setter can write back.
    style_obj.set(js_string!("__element"), element_ref, false, ctx)?;

    // Implement cssText as a live getter/setter backed by the element's style attribute.
    let css_text_getter = NativeFunction::from_fn_ptr(style_css_text_getter);
    let css_text_setter = NativeFunction::from_fn_ptr(style_css_text_setter);
    let css_text_getter_obj = FunctionObjectBuilder::new(&realm, css_text_getter).build();
    let css_text_setter_obj = FunctionObjectBuilder::new(&realm, css_text_setter).build();
    let css_text_desc = boa_engine::property::PropertyDescriptor::builder()
        .get(css_text_getter_obj)
        .set(css_text_setter_obj)
        .enumerable(true)
        .configurable(true)
        .build();
    style_obj.define_property_or_throw(js_string!("cssText"), css_text_desc, ctx)?;

    Ok(style_obj.into())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn resolve_style_element(this: &JsValue, ec: &mut dyn ExecutionContext<BoaTypes>) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("expected style object"))?;
    obj.get(js_string!("__element"), ctx)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn style_css_text_getter(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsResult<JsValue> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    // Read the element's style attribute.
    let element_val = resolve_style_element(this, ctx)?;
    let element_obj = element_val
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("cssText getter: element not found"))?;

    let style_attr = if let Some(el) = element_obj.downcast_ref::<Element>() {
        el.get_attribute("style").unwrap_or_default()
    } else if let Some(html_el) = element_obj.downcast_ref::<HTMLElement>() {
        html_el.element.get_attribute("style").unwrap_or_default()
    } else if let Some(media) = element_obj.downcast_ref::<HTMLMediaElement>() {
        media
            .html_element
            .element
            .get_attribute("style")
            .unwrap_or_default()
    } else if let Some(video) = element_obj.downcast_ref::<HTMLVideoElement>() {
        video
            .media_element
            .html_element
            .element
            .get_attribute("style")
            .unwrap_or_default()
    } else if let Some(ifr) = element_obj.downcast_ref::<HTMLIFrameElement>() {
        ifr.html_element
            .element
            .get_attribute("style")
            .unwrap_or_default()
    } else if let Some(anc) = element_obj.downcast_ref::<HTMLAnchorElement>() {
        anc.html_element
            .element
            .get_attribute("style")
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(JsValue::from(JsString::from(style_attr)))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn style_css_text_setter(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsResult<JsValue> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let value = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    let element_val = resolve_style_element(this, ctx)?;
    let element_obj = element_val
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("cssText setter: element not found"))?;

    if let Some(el) = element_obj.downcast_ref::<Element>() {
        if value.is_empty() {
            el.remove_attribute("style");
        } else {
            el.set_attribute("style", &value);
        }
    } else if let Some(html_el) = element_obj.downcast_ref::<HTMLElement>() {
        if value.is_empty() {
            html_el.element.remove_attribute("style");
        } else {
            html_el.element.set_attribute("style", &value);
        }
    } else if let Some(media) = element_obj.downcast_ref::<HTMLMediaElement>() {
        if value.is_empty() {
            media.html_element.element.remove_attribute("style");
        } else {
            media.html_element.element.set_attribute("style", &value);
        }
    } else if let Some(video) = element_obj.downcast_ref::<HTMLVideoElement>() {
        if value.is_empty() {
            video
                .media_element
                .html_element
                .element
                .remove_attribute("style");
        } else {
            video
                .media_element
                .html_element
                .element
                .set_attribute("style", &value);
        }
    } else if let Some(ifr) = element_obj.downcast_ref::<HTMLIFrameElement>() {
        if value.is_empty() {
            ifr.html_element.element.remove_attribute("style");
        } else {
            ifr.html_element.element.set_attribute("style", &value);
        }
    } else if let Some(input) = element_obj.downcast_ref::<HTMLInputElement>() {
        if value.is_empty() {
            input.html_element.element.remove_attribute("style");
        } else {
            input.html_element.element.set_attribute("style", &value);
        }
    } else if let Some(anc) = element_obj.downcast_ref::<HTMLAnchorElement>() {
        if value.is_empty() {
            anc.html_element.element.remove_attribute("style");
        } else {
            anc.html_element.element.set_attribute("style", &value);
        }
    }
    Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsResult<JsValue> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    // Step 1.1 of CSSStyleDeclaration.getPropertyValue(property): if property is not a custom
    // property, convert it to ASCII lowercase.
    let property_name = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped()
        .trim()
        .to_ascii_lowercase();

    // Step 2: "If property is a case-sensitive match for a property name of a CSS declaration in
    // the declarations, then return the result of invoking serialize a CSS value of that
    // declaration."
    let Some(object) = this.as_object() else {
        return Ok(JsValue::from(JsString::from("")));
    };
    let value = object.get(JsString::from(property_name.as_str()), ctx)?;

    // Step 3: "Return the empty string."
    // Note: This snapshot object currently exposes directly materialized longhand values only, so
    // shorthand serialization still falls through to the empty string.
    if value.is_undefined() {
        return Ok(JsValue::from(JsString::from("")));
    }
    Ok(value)
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
