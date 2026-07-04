use std::collections::BTreeMap;
use std::marker::PhantomData;

type JsValue = <crate::js::Types as JsTypes>::JsValue;
type JsObject = <crate::js::Types as JsTypes>::JsObject;

use crate::dom::Element;
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement,
    HTMLVideoElement, inline_style_properties_for_element,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for HTMLElement {
    const NAME: &'static str = "HTMLElement";

    fn parent_name() -> Option<&'static str> {
        Some("Element")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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

fn try_with_html_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&HTMLElement) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("HTMLElement receiver is not an object"))?;

    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(f(html_element));
        }
        if let Some(anchor) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&anchor.html_element));
        }
        if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            return Ok(f(&input.html_element));
        }
        if let Some(iframe) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(&iframe.html_element));
        }
    }
    Err(ec.new_type_error("receiver is not an HTMLElement"))
}

fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let title = try_with_html_element_ref(this, ec, |html_element| html_element.title())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&title)))
}

fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let title = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_title(&title))?;
    Ok(ec.value_undefined())
}

fn get_lang(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let lang = try_with_html_element_ref(this, ec, |html_element| html_element.lang())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&lang)))
}

fn set_lang(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let lang = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_lang(&lang))?;
    Ok(ec.value_undefined())
}

fn get_dir(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let dir = try_with_html_element_ref(this, ec, |html_element| html_element.dir())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&dir)))
}

fn set_dir(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undef = ec.value_undefined();
    let dir = ec.to_rust_string(args.first().cloned().unwrap_or(undef))?;
    try_with_html_element_ref(this, ec, |html_element| html_element.set_dir(&dir))?;
    Ok(ec.value_undefined())
}

fn get_hidden(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let hidden = try_with_html_element_ref(this, ec, |html_element| html_element.hidden())?;
    Ok(ec.value_from_bool(hidden))
}

fn set_hidden(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let hidden = args.first().map_or(false, |v| ec.to_boolean(v));
    try_with_html_element_ref(this, ec, |html_element| html_element.set_hidden(hidden))?;
    Ok(ec.value_undefined())
}

fn get_style(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    use crate::js::Types;

    let object = Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("style getter: receiver is not an object"))?;
    let element_ref = Types::value_from_object(object.clone());

    // Build the style declaration object with a reference to the element,
    // so that cssText and individual property setters can write back.
    let properties = try_with_html_element_ref(this, ec, |html_element| {
        inline_style_properties_for_element(&html_element.element)
    })?;

    let style_obj = ec.create_plain_object(None);

    for (name, value) in &properties {
        // cssText is handled separately; skip it here to avoid conflict.
        if name == "cssText" {
            continue;
        }
        let js_value = ec.value_from_string(ec.js_string_from_str(value.as_str()));
        let name_key = ec.property_key_from_str(name);
        ec.set(style_obj.clone(), name_key, js_value.clone(), false)?;
        let alias = camel_case_property_name(name);
        if alias != *name {
            let alias_key = ec.property_key_from_str(&alias);
            ec.set(style_obj.clone(), alias_key, js_value, false)?;
        }
    }

    // getPropertyValue method
    let get_property_value_fn = ec.create_builtin_function(
        Box::new(move |args, this_ec_val, inner_ec| {
            let property_name = if let Some(arg) = args.first() {
                inner_ec
                    .to_rust_string(arg.clone())?
                    .trim()
                    .to_ascii_lowercase()
            } else {
                String::new()
            };
            let Some(object) = Types::value_as_object(&this_ec_val) else {
                return Ok(inner_ec.value_from_string(inner_ec.js_string_from_str("")));
            };
            let key = inner_ec.property_key_from_str(&property_name);
            let value = ExecutionContext::get(inner_ec, object, key)?;
            if Types::value_is_undefined(&value) {
                return Ok(inner_ec.value_from_string(inner_ec.js_string_from_str("")));
            }
            Ok(value)
        }),
        1,
        ec.property_key_from_str("getPropertyValue"),
    );
    let method_val = Types::value_from_object(Types::object_from_function(get_property_value_fn));
    ec.set(
        style_obj.clone(),
        ec.property_key_from_str("getPropertyValue"),
        method_val,
        false,
    )?;

    // Store a reference to the element so cssText setter can write back.
    ec.set(
        style_obj.clone(),
        ec.property_key_from_str("__element"),
        element_ref,
        false,
    )?;

    // Implement cssText as a live getter/setter backed by the element's style attribute.
    let css_text_getter = ec.create_builtin_function(
        Box::new(move |_args, this_ec_val, inner_ec| {
            let element_val = {
                let this_obj = Types::value_as_object(&this_ec_val).ok_or_else(|| {
                    inner_ec.new_type_error("cssText getter: receiver is not an object")
                })?;
                ExecutionContext::get(
                    inner_ec,
                    this_obj,
                    inner_ec.property_key_from_str("__element"),
                )?
            };
            let style = element_style_attribute(&element_val, inner_ec).unwrap_or_default();
            Ok(inner_ec.value_from_string(inner_ec.js_string_from_str(&style)))
        }),
        0,
        ec.property_key_from_str("get cssText"),
    );

    let css_text_setter = ec.create_builtin_function(
        Box::new(move |args, this_ec_val, inner_ec| {
            let value = if let Some(arg) = args.first() {
                inner_ec.to_rust_string(arg.clone())?
            } else {
                String::new()
            };
            let element_val = {
                let this_obj = Types::value_as_object(&this_ec_val).ok_or_else(|| {
                    inner_ec.new_type_error("cssText setter: receiver is not an object")
                })?;
                ExecutionContext::get(
                    inner_ec,
                    this_obj,
                    inner_ec.property_key_from_str("__element"),
                )?
            };
            set_element_style_attribute(&element_val, &value, inner_ec);
            Ok(inner_ec.value_undefined())
        }),
        1,
        ec.property_key_from_str("set cssText"),
    );

    let css_text_key = ec.property_key_from_str("cssText");
    let accessor_desc = js_engine::PropertyDescriptor {
        value: None,
        writable: None,
        get: Some(css_text_getter),
        set: Some(css_text_setter),
        enumerable: Some(true),
        configurable: Some(true),
    };
    ec.define_property_or_throw(style_obj.clone(), css_text_key, accessor_desc)?;

    Ok(Types::value_from_object(style_obj))
}

/// Read the element's `style` attribute via the generic EC trait.
fn element_style_attribute(
    element_val: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Option<String> {
    use crate::js::Types;
    let obj = Types::value_as_object(element_val)?;
    let data = ec.with_object_any(&obj)?;

    // Note: dyn Any::downcast_ref matches exact type only, so we check
    // most-derived types first and walk up to base types.
    if let Some(el) = data.downcast_ref::<HTMLVideoElement>() {
        Some(
            el.media_element
                .html_element
                .element
                .get_attribute("style")
                .unwrap_or_default(),
        )
    } else if let Some(el) = data.downcast_ref::<HTMLMediaElement>() {
        Some(
            el.html_element
                .element
                .get_attribute("style")
                .unwrap_or_default(),
        )
    } else if let Some(el) = data.downcast_ref::<HTMLAnchorElement>() {
        Some(
            el.html_element
                .element
                .get_attribute("style")
                .unwrap_or_default(),
        )
    } else if let Some(el) = data.downcast_ref::<HTMLIFrameElement>() {
        Some(
            el.html_element
                .element
                .get_attribute("style")
                .unwrap_or_default(),
        )
    } else if let Some(el) = data.downcast_ref::<HTMLInputElement>() {
        Some(
            el.html_element
                .element
                .get_attribute("style")
                .unwrap_or_default(),
        )
    } else if let Some(el) = data.downcast_ref::<HTMLElement>() {
        Some(el.element.get_attribute("style").unwrap_or_default())
    } else if let Some(el) = data.downcast_ref::<Element>() {
        Some(el.get_attribute("style").unwrap_or_default())
    } else {
        None
    }
}

/// Set/remove the element's `style` attribute via the generic EC trait.
fn set_element_style_attribute(
    element_val: &JsValue,
    value: &str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) {
    use crate::js::Types;
    let Some(obj) = Types::value_as_object(element_val) else {
        return;
    };
    let Some(data) = ec.with_object_any(&obj) else {
        return;
    };

    if let Some(el) = data.downcast_ref::<HTMLVideoElement>() {
        let elem = &el.media_element.html_element.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<HTMLMediaElement>() {
        let elem = &el.html_element.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<HTMLAnchorElement>() {
        let elem = &el.html_element.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<HTMLIFrameElement>() {
        let elem = &el.html_element.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<HTMLInputElement>() {
        let elem = &el.html_element.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<HTMLElement>() {
        let elem = &el.element;
        if value.is_empty() {
            elem.remove_attribute("style");
        } else {
            elem.set_attribute("style", value);
        }
    } else if let Some(el) = data.downcast_ref::<Element>() {
        if value.is_empty() {
            el.remove_attribute("style");
        } else {
            el.set_attribute("style", value);
        }
    }
}

pub(crate) fn style_declaration_object(
    properties: &BTreeMap<String, String>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let object = ec.create_plain_object(None);
    for (name, value) in properties {
        let js_value = ec.value_from_string(ec.js_string_from_str(value.as_str()));
        let key = ec.property_key_from_str(name.as_str());
        let desc = js_engine::PropertyDescriptor {
            value: Some(js_value.clone()),
            writable: Some(true),
            get: None,
            set: None,
            enumerable: Some(true),
            configurable: Some(true),
        };
        ec.define_property_or_throw(object.clone(), key, desc)?;

        let alias = camel_case_property_name(name);
        if alias != *name {
            let alias_key = ec.property_key_from_str(alias.as_str());
            let alias_desc = js_engine::PropertyDescriptor {
                value: Some(js_value),
                writable: Some(true),
                get: None,
                set: None,
                enumerable: Some(true),
                configurable: Some(true),
            };
            ec.define_property_or_throw(object.clone(), alias_key, alias_desc)?;
        }
    }

    // Add getPropertyValue method.
    let getter_fn = ec.create_builtin_function(
        Box::new(|args, this, ec| {
            // Step 1.1: convert to ASCII lowercase.
            let undef = ec.value_undefined();
            let property_name = ec
                .to_rust_string(args.first().cloned().unwrap_or(undef))?
                .trim()
                .to_ascii_lowercase();

            // Step 2: Look up property in the declaration object.
            let object = match <crate::js::Types as JsTypes>::value_as_object(&this) {
                Some(obj) => obj,
                None => return Ok(ec.value_from_string(ec.js_string_from_str(""))),
            };
            let key = ec.property_key_from_str(property_name.as_str());
            let value = js_engine::ExecutionContext::get(ec, object, key)?;

            // Step 3: Return empty string for undefined values.
            if value.is_undefined() {
                return Ok(ec.value_from_string(ec.js_string_from_str("")));
            }
            Ok(value)
        }),
        1,
        ec.property_key_from_str("getPropertyValue"),
    );
    let getter_value = <crate::js::Types as JsTypes>::value_from_object(
        <crate::js::Types as JsTypes>::object_from_function(getter_fn),
    );
    let getter_key = ec.property_key_from_str("getPropertyValue");
    let desc = js_engine::PropertyDescriptor {
        value: Some(getter_value),
        writable: Some(true),
        get: None,
        set: None,
        enumerable: Some(true),
        configurable: Some(true),
    };
    ec.define_property_or_throw(object.clone(), getter_key, desc)?;

    Ok(object)
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
