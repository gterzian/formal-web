use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, ObjectInitializer, builtins::JsArray},
    property::Attribute,
};

use crate::js::platform_objects::invalidate_cached_node_ids;
use crate::dom::{DOMException, Element};
use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement, HTMLVideoElement};
use crate::webidl::bindings::{
    create_interface_instance,

    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for Element {
    const NAME: &'static str = "Element";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            id: "id",
            getter: get_id,
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
            id: "tagName",
            getter: get_tag_name,
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
            id: "innerHTML",
            getter: get_inner_html,
            setter: Some(set_inner_html),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
        def.add_attribute(AttributeDef {
            id: "classList",
            getter: get_class_list,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });

        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "querySelector",
            length: 1,
            method: query_selector,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "querySelectorAll",
            length: 1,
            method: query_selector_all,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "insertAdjacentText",
            length: 2,
            method: insert_adjacent_text,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "setAttribute",
            length: 2,
            method: set_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "setAttributeNS",
            length: 3,
            method: set_attribute_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "getAttribute",
            length: 1,
            method: get_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "hasAttribute",
            length: 1,
            method: has_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "removeAttribute",
            length: 1,
            method: remove_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "getBoundingClientRect",
            length: 0,
            method: get_bounding_client_rect,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}


pub(crate) fn with_element_ref<R>(this: &JsValue, f: impl FnOnce(&Element) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("element receiver is not an object"))?;
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(f(&element));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(f(&html_element.element));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&html_anchor_element.html_element.element));
    }
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&html_iframe_element.html_element.element));
    }
    if let Some(html_input_element) = object.downcast_ref::<HTMLInputElement>() {
        return Ok(f(&html_input_element.html_element.element));
    }
    if let Some(html_media_element) = object.downcast_ref::<HTMLMediaElement>() {
        return Ok(f(&html_media_element.html_element.element));
    }
    if let Some(html_video_element) = object.downcast_ref::<HTMLVideoElement>() {
        return Ok(f(&html_video_element.media_element.html_element.element));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not an Element")
        .into())
}

fn get_id(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_element_ref(this, |element| JsValue::from(JsString::from(element.id())))
}

fn get_tag_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_element_ref(this, |element| {
        JsValue::from(JsString::from(element.tag_name().as_str()))
    })
}

fn get_inner_html(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_element_ref(this, |element| {
        JsValue::from(JsString::from(element.inner_html().as_str()))
    })
}

fn set_inner_html(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let html = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let dropped_node_ids = with_element_ref(this, Element::child_subtree_node_ids)?;
    invalidate_cached_node_ids(context, &dropped_node_ids)?;
    with_element_ref(this, |element| {
        element.set_inner_html(&html);
    })?;
    Ok(JsValue::undefined())
}

/// <https://dom.spec.whatwg.org/#dom-element-classlist>
fn get_class_list(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let realm = context.realm().clone();
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("classList receiver is not an object")
    })?;
    let obj_clone = JsValue::from(obj.clone());

    // Build a simple JS object that wraps class attribute manipulation.
    // <https://dom.spec.whatwg.org/#domtokenlist>
    let token_list = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(class_list_add),
            js_string!("add"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(class_list_remove),
            js_string!("remove"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(class_list_toggle),
            js_string!("toggle"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(class_list_contains),
            js_string!("contains"),
            1,
        )
        .build();

    // Store a reference to the element so closures can access it.
    // Note: The spec requires that DOMTokenList is "live" — changes to
    // the element's class attribute are reflected. Our implementation
    // reads the class attribute fresh on each call.
    token_list.set(js_string!("__element"), obj_clone, false, context)?;

    // length getter
    let len_fn = NativeFunction::from_fn_ptr(class_list_length);
    let len_fn_obj = FunctionObjectBuilder::new(&realm, len_fn).build();
    let len_desc = boa_engine::property::PropertyDescriptor::builder()
        .get(len_fn_obj)
        .enumerable(true)
        .configurable(true)
        .build();
    token_list.define_property_or_throw(js_string!("length"), len_desc, context)?;

    Ok(token_list.into())
}

fn class_list_value(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<String> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("expected object")
    })?;
    let element_val = obj.get(js_string!("__element"), context)?;
    let element_obj = element_val.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("classList: element not found")
    })?;

    if let Some(el) = element_obj.downcast_ref::<Element>() {
        Ok(el.get_attribute("class").unwrap_or_default())
    } else if let Some(html_el) = element_obj.downcast_ref::<HTMLElement>() {
        Ok(html_el.element.get_attribute("class").unwrap_or_default())
    } else if let Some(media) = element_obj.downcast_ref::<HTMLMediaElement>() {
        Ok(media.html_element.element.get_attribute("class").unwrap_or_default())
    } else if let Some(video) = element_obj.downcast_ref::<HTMLVideoElement>() {
        Ok(video.media_element.html_element.element.get_attribute("class").unwrap_or_default())
    } else if let Some(ifr) = element_obj.downcast_ref::<HTMLIFrameElement>() {
        Ok(ifr.html_element.element.get_attribute("class").unwrap_or_default())
    } else if let Some(input) = element_obj.downcast_ref::<HTMLInputElement>() {
        Ok(input.html_element.element.get_attribute("class").unwrap_or_default())
    } else if let Some(anc) = element_obj.downcast_ref::<HTMLAnchorElement>() {
        Ok(anc.html_element.element.get_attribute("class").unwrap_or_default())
    } else {
        Ok(String::new())
    }
}

fn class_list_set_value(this: &JsValue, value: &str, context: &mut Context) -> JsResult<()> {
    let obj = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("expected object")
    })?;
    let element_val = obj.get(js_string!("__element"), context)?;
    let element_obj = element_val.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("classList: element not found")
    })?;

    if let Some(el) = element_obj.downcast_ref::<Element>() {
        if value.is_empty() {
            el.remove_attribute("class");
        } else {
            el.set_attribute("class", value);
        }
    } else if let Some(html_el) = element_obj.downcast_ref::<HTMLElement>() {
        if value.is_empty() {
            html_el.element.remove_attribute("class");
        } else {
            html_el.element.set_attribute("class", value);
        }
    } else if let Some(media) = element_obj.downcast_ref::<HTMLMediaElement>() {
        if value.is_empty() {
            media.html_element.element.remove_attribute("class");
        } else {
            media.html_element.element.set_attribute("class", value);
        }
    } else if let Some(video) = element_obj.downcast_ref::<HTMLVideoElement>() {
        if value.is_empty() {
            video.media_element.html_element.element.remove_attribute("class");
        } else {
            video.media_element.html_element.element.set_attribute("class", value);
        }
    } else if let Some(ifr) = element_obj.downcast_ref::<HTMLIFrameElement>() {
        if value.is_empty() {
            ifr.html_element.element.remove_attribute("class");
        } else {
            ifr.html_element.element.set_attribute("class", value);
        }
    } else if let Some(input) = element_obj.downcast_ref::<HTMLInputElement>() {
        if value.is_empty() {
            input.html_element.element.remove_attribute("class");
        } else {
            input.html_element.element.set_attribute("class", value);
        }
    } else if let Some(anc) = element_obj.downcast_ref::<HTMLAnchorElement>() {
        if value.is_empty() {
            anc.html_element.element.remove_attribute("class");
        } else {
            anc.html_element.element.set_attribute("class", value);
        }
    }
    Ok(())
}

fn class_list_add(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let token = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let current = class_list_value(this, &[], context)?;
    let mut classes: Vec<String> = if current.is_empty() {
        Vec::new()
    } else {
        current.split(' ').map(|s| s.to_string()).collect()
    };
    if !classes.contains(&token) {
        classes.push(token);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, context)?;
    }
    Ok(JsValue::undefined())
}

fn class_list_remove(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let token = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let current = class_list_value(this, &[], context)?;
    let classes: Vec<String> = current
        .split(' ')
        .filter(|c| !c.is_empty() && *c != token)
        .map(|s| s.to_string())
        .collect();
    let new_value = classes.join(" ");
    class_list_set_value(this, &new_value, context)?;
    Ok(JsValue::undefined())
}

fn class_list_toggle(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let token = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let current = class_list_value(this, &[], context)?;
    let mut classes: Vec<String> = if current.is_empty() {
        Vec::new()
    } else {
        current.split(' ').map(|s| s.to_string()).collect()
    };
    if let Some(pos) = classes.iter().position(|c| c == &token) {
        classes.remove(pos);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, context)?;
        Ok(JsValue::new(false))
    } else {
        classes.push(token);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, context)?;
        Ok(JsValue::new(true))
    }
}

fn class_list_contains(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let token = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let current = class_list_value(this, &[], context)?;
    let classes: Vec<&str> = current.split(' ').collect();
    Ok(JsValue::new(classes.contains(&token.as_str())))
}

fn class_list_length(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let current = class_list_value(this, &[], context)?;
    let count = if current.is_empty() { 0 } else { current.split(' ').count() };
    Ok(JsValue::new(count))
}

fn query_selector(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let selector = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_id = with_element_ref(this, |element| element.query_selector(&selector))?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    match node_id {
        Some(node_id) => {
            Ok(crate::js::platform_objects::resolve_element_object(node_id, context)?.into())
        }
        None => Ok(JsValue::null()),
    }
}

fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let selector = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_ids = with_element_ref(this, |element| element.query_selector_all(&selector))?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| {
            crate::js::platform_objects::resolve_element_object(node_id, context)
                .map(JsValue::from)
        })
        .collect::<JsResult<Vec<_>>>()?;
    Ok(JsArray::from_iter(values, context).into())
}

fn insert_adjacent_text(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let where_ = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let data = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    with_element_ref(this, |element| element.insert_adjacent_text(&where_, &data))?.map_err(
        |error| {
            JsError::from_opaque(JsValue::from(
                create_interface_instance::<DOMException>(error, context)
                    .expect("DOMException construction should not fail"),
            ))
        },
    )?;
    Ok(JsValue::undefined())
}

fn get_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    Ok(
        match with_element_ref(this, |element| element.get_attribute(&name))? {
            Some(value) => JsValue::from(JsString::from(value.as_str())),
            None => JsValue::null(),
        },
    )
}

fn has_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    Ok(JsValue::from(with_element_ref(this, |element| {
        element.has_attribute(&name)
    })?))
}

fn set_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let value = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    with_element_ref(this, |element| {
        element.set_attribute(&name, &value);
    })?;
    Ok(JsValue::undefined())
}

fn set_attribute_ns(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let namespace = if args.get_or_undefined(0).is_null() || args.get_or_undefined(0).is_undefined()
    {
        None
    } else {
        Some(
            args.get_or_undefined(0)
                .to_string(context)?
                .to_std_string_escaped(),
        )
    };
    let qualified_name = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let value = args
        .get_or_undefined(2)
        .to_string(context)?
        .to_std_string_escaped();
    with_element_ref(this, |element| {
        element.set_attribute_ns(namespace.as_deref(), &qualified_name, &value);
    })?;
    Ok(JsValue::undefined())
}

fn remove_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_element_ref(this, |element| {
        element.remove_attribute(&name);
    })?;
    Ok(JsValue::undefined())
}

fn get_bounding_client_rect(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let rect = with_element_ref(this, |element| {
        element.bounding_client_rect().unwrap_or_default()
    })?;
    let mut initializer = ObjectInitializer::new(context);
    initializer.property(js_string!("x"), rect.x, Attribute::all());
    initializer.property(js_string!("y"), rect.y, Attribute::all());
    initializer.property(js_string!("width"), rect.width, Attribute::all());
    initializer.property(js_string!("height"), rect.height, Attribute::all());
    initializer.property(js_string!("top"), rect.top, Attribute::all());
    initializer.property(js_string!("right"), rect.right, Attribute::all());
    initializer.property(js_string!("bottom"), rect.bottom, Attribute::all());
    initializer.property(js_string!("left"), rect.left, Attribute::all());
    Ok(initializer.build().into())
}
