use boa_engine::{JsNativeError, JsResult, JsValue};
use std::marker::PhantomData;

use crate::dom::{DOMException, Element};
use crate::html::{
    HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement, HTMLMediaElement,
    HTMLVideoElement,
};
use crate::js::platform_objects::{invalidate_cached_node_ids, resolve_element_object};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for Element {
    const NAME: &'static str = "Element";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

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
            _phantom: PhantomData,

            id: "querySelector",
            length: 1,
            method: query_selector,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "querySelectorAll",
            length: 1,
            method: query_selector_all,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "insertAdjacentText",
            length: 2,
            method: insert_adjacent_text,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "setAttribute",
            length: 2,
            method: set_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "setAttributeNS",
            length: 3,
            method: set_attribute_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "getAttribute",
            length: 1,
            method: get_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "hasAttribute",
            length: 1,
            method: has_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "removeAttribute",
            length: 1,
            method: remove_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

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

fn try_with_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Element) -> R,
) -> Completion<R, crate::js::Types> {
    let object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("element receiver is not an object"))?;
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
    Err(ec.new_type_error("receiver is not an Element"))
}

fn get_id(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let id = try_with_element_ref(this, ec, |element| element.id())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&id)))
}

fn get_tag_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let name = try_with_element_ref(this, ec, |element| element.tag_name())?;
    Ok(ec.value_from_string(ec.js_string_from_str(name.as_str())))
}

fn get_inner_html(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let html = try_with_element_ref(this, ec, |element| element.inner_html())?;
    Ok(ec.value_from_string(ec.js_string_from_str(html.as_str())))
}

fn set_inner_html(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let html = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let dropped_node_ids = try_with_element_ref(this, ec, Element::child_subtree_node_ids)?;
    invalidate_cached_node_ids(ec, &dropped_node_ids)?;
    try_with_element_ref(this, ec, |element| element.set_inner_html(&html))?;
    Ok(ec.value_undefined())
}

/// <https://dom.spec.whatwg.org/#dom-element-classlist>
fn get_class_list(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("classList receiver is not an object"))?;
    let obj_clone = crate::js::Types::value_from_object(obj.clone());

    // Build a simple JS object that wraps class attribute manipulation.
    // <https://dom.spec.whatwg.org/#domtokenlist>
    let token_list = ec.create_plain_object(None);

    // "add" method
    let add_fn = ec.create_builtin_function(
        Box::new(
            move |args: &[<crate::js::Types as js_engine::JsTypes>::JsValue],
                  this_val: <crate::js::Types as js_engine::JsTypes>::JsValue,
                  inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                  -> Completion<
                <crate::js::Types as js_engine::JsTypes>::JsValue,
                crate::js::Types,
            > { class_list_add(&this_val, args, inner_ec) },
        ),
        1,
        ec.property_key_from_str("add"),
    );
    ec.object_set_property(
        token_list.clone(),
        "add",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(add_fn)),
    )?;

    // "remove" method
    let remove_fn = ec.create_builtin_function(
        Box::new(
            move |args: &[<crate::js::Types as js_engine::JsTypes>::JsValue],
                  this_val: <crate::js::Types as js_engine::JsTypes>::JsValue,
                  inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                  -> Completion<
                <crate::js::Types as js_engine::JsTypes>::JsValue,
                crate::js::Types,
            > { class_list_remove(&this_val, args, inner_ec) },
        ),
        1,
        ec.property_key_from_str("remove"),
    );
    ec.object_set_property(
        token_list.clone(),
        "remove",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(remove_fn)),
    )?;

    // "toggle" method
    let toggle_fn = ec.create_builtin_function(
        Box::new(
            move |args: &[<crate::js::Types as js_engine::JsTypes>::JsValue],
                  this_val: <crate::js::Types as js_engine::JsTypes>::JsValue,
                  inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                  -> Completion<
                <crate::js::Types as js_engine::JsTypes>::JsValue,
                crate::js::Types,
            > { class_list_toggle(&this_val, args, inner_ec) },
        ),
        1,
        ec.property_key_from_str("toggle"),
    );
    ec.object_set_property(
        token_list.clone(),
        "toggle",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(toggle_fn)),
    )?;

    // "contains" method
    let contains_fn = ec.create_builtin_function(
        Box::new(
            move |args: &[<crate::js::Types as js_engine::JsTypes>::JsValue],
                  this_val: <crate::js::Types as js_engine::JsTypes>::JsValue,
                  inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                  -> Completion<
                <crate::js::Types as js_engine::JsTypes>::JsValue,
                crate::js::Types,
            > { class_list_contains(&this_val, args, inner_ec) },
        ),
        1,
        ec.property_key_from_str("contains"),
    );
    ec.object_set_property(
        token_list.clone(),
        "contains",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(contains_fn)),
    )?;

    // Store a reference to the element so closures can access it.
    // Note: The spec requires that DOMTokenList is "live" — changes to
    // the element's class attribute are reflected. Our implementation
    // reads the class attribute fresh on each call.
    ec.object_set_property(token_list.clone(), "__element", obj_clone)?;

    // length getter
    let len_fn = ec.create_builtin_function(
        Box::new(
            move |_args: &[<crate::js::Types as js_engine::JsTypes>::JsValue],
                  this_val: <crate::js::Types as js_engine::JsTypes>::JsValue,
                  inner_ec: &mut dyn ExecutionContext<crate::js::Types>|
                  -> Completion<
                <crate::js::Types as js_engine::JsTypes>::JsValue,
                crate::js::Types,
            > { class_list_length(&this_val, &[], inner_ec) },
        ),
        0,
        ec.property_key_from_str("get_length"),
    );
    let len_desc = js_engine::PropertyDescriptor {
        value: None,
        writable: None,
        get: Some(len_fn),
        set: None,
        enumerable: Some(true),
        configurable: Some(true),
    };
    ec.define_property_or_throw(
        token_list.clone(),
        ec.property_key_from_str("length"),
        len_desc,
    )?;

    Ok(crate::js::Types::value_from_object(token_list))
}

fn class_list_value(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<String, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element_key = ec.property_key_from_str("__element");
    let element_val = ExecutionContext::get(ec, obj.clone(), element_key)?;
    let element_obj = crate::js::Types::value_as_object(&element_val)
        .ok_or_else(|| ec.new_type_error("classList: element not found"))?;

    if let Some(el) = element_obj.downcast_ref::<Element>() {
        Ok(el.get_attribute("class").unwrap_or_default())
    } else if let Some(html_el) = element_obj.downcast_ref::<HTMLElement>() {
        Ok(html_el.element.get_attribute("class").unwrap_or_default())
    } else if let Some(media) = element_obj.downcast_ref::<HTMLMediaElement>() {
        Ok(media
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default())
    } else if let Some(video) = element_obj.downcast_ref::<HTMLVideoElement>() {
        Ok(video
            .media_element
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default())
    } else if let Some(ifr) = element_obj.downcast_ref::<HTMLIFrameElement>() {
        Ok(ifr
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default())
    } else if let Some(input) = element_obj.downcast_ref::<HTMLInputElement>() {
        Ok(input
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default())
    } else if let Some(anc) = element_obj.downcast_ref::<HTMLAnchorElement>() {
        Ok(anc
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default())
    } else {
        Ok(String::new())
    }
}

fn class_list_set_value(
    this: &JsValue,
    value: &str,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("expected object"))?;
    let element_key = ec.property_key_from_str("__element");
    let element_val = ExecutionContext::get(ec, obj.clone(), element_key)?;
    let element_obj = crate::js::Types::value_as_object(&element_val)
        .ok_or_else(|| ec.new_type_error("classList: element not found"))?;

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
            video
                .media_element
                .html_element
                .element
                .remove_attribute("class");
        } else {
            video
                .media_element
                .html_element
                .element
                .set_attribute("class", value);
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

fn class_list_add(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let current = class_list_value(this, &[], ec)?;
    let value_undefined = ec.value_undefined();
    let token = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let mut classes: Vec<String> = if current.is_empty() {
        Vec::new()
    } else {
        current.split(' ').map(|s| s.to_string()).collect()
    };
    if !classes.contains(&token) {
        classes.push(token);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, ec)?;
    }
    Ok(ec.value_undefined())
}

fn class_list_remove(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let current = class_list_value(this, &[], ec)?;
    let value_undefined = ec.value_undefined();
    let token = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let classes: Vec<String> = current
        .split(' ')
        .filter(|c| !c.is_empty() && *c != token)
        .map(|s| s.to_string())
        .collect();
    let new_value = classes.join(" ");
    class_list_set_value(this, &new_value, ec)?;
    Ok(ec.value_undefined())
}

fn class_list_toggle(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let current = class_list_value(this, &[], ec)?;
    let value_undefined = ec.value_undefined();
    let token = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let mut classes: Vec<String> = if current.is_empty() {
        Vec::new()
    } else {
        current.split(' ').map(|s| s.to_string()).collect()
    };
    if let Some(pos) = classes.iter().position(|c| c == &token) {
        classes.remove(pos);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, ec)?;
        Ok(ec.value_from_bool(false))
    } else {
        classes.push(token);
        let new_value = classes.join(" ");
        class_list_set_value(this, &new_value, ec)?;
        Ok(ec.value_from_bool(true))
    }
}

fn class_list_contains(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let current = class_list_value(this, &[], ec)?;
    let value_undefined = ec.value_undefined();
    let token = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let classes: Vec<&str> = current.split(' ').collect();
    Ok(ec.value_from_bool(classes.contains(&token.as_str())))
}

fn class_list_length(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let current = class_list_value(this, &[], ec)?;
    let count = if current.is_empty() {
        0
    } else {
        current.split(' ').count()
    };
    Ok(ec.value_from_number(count as f64))
}

fn query_selector(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let node_id = try_with_element_ref(this, ec, |element| element.query_selector(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    match node_id {
        Some(node_id) => {
            let obj = resolve_element_object(node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let node_ids = try_with_element_ref(this, ec, |element| element.query_selector_all(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = resolve_element_object(node_id, ec)?;
        ec.array_push(&array, crate::js::Types::value_from_object(obj))?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn insert_adjacent_text(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let where_ = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let data = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| {
        element.insert_adjacent_text(&where_, &data)
    })?
    .map_err(|error| {
        create_interface_instance::<crate::js::Types, DOMException>(error, ec)
            .map(|obj| crate::js::Types::value_from_object(obj))
            .unwrap_or_else(|err| err)
    })?;
    Ok(ec.value_undefined())
}

fn get_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    match try_with_element_ref(this, ec, |element| element.get_attribute(&name))? {
        Some(value) => Ok(ec.value_from_string(ec.js_string_from_str(value.as_str()))),
        None => Ok(ec.value_null()),
    }
}

fn has_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let result = try_with_element_ref(this, ec, |element| element.has_attribute(&name))?;
    Ok(ec.value_from_bool(result))
}

fn set_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let value = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| element.set_attribute(&name, &value))?;
    Ok(ec.value_undefined())
}

fn set_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let first = args.first().cloned().unwrap_or(value_undefined.clone());
    let is_nullish =
        crate::js::Types::value_is_null(&first) || crate::js::Types::value_is_undefined(&first);
    let namespace = if is_nullish {
        None
    } else {
        Some(ec.to_rust_string(first)?)
    };
    let qualified_name =
        ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined.clone()))?;
    let value = ec.to_rust_string(args.get(2).cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| {
        element.set_attribute_ns(namespace.as_deref(), &qualified_name, &value);
    })?;
    Ok(ec.value_undefined())
}

fn remove_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| element.remove_attribute(&name))?;
    Ok(ec.value_undefined())
}

fn get_bounding_client_rect(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let rect = try_with_element_ref(this, ec, |element| {
        element.bounding_client_rect().unwrap_or_default()
    })?;
    let obj = ec.create_plain_object(None);
    let vx = ec.value_from_number(rect.x);
    let vy = ec.value_from_number(rect.y);
    let vw = ec.value_from_number(rect.width);
    let vh = ec.value_from_number(rect.height);
    let vt = ec.value_from_number(rect.top);
    let vr = ec.value_from_number(rect.right);
    let vb = ec.value_from_number(rect.bottom);
    let vl = ec.value_from_number(rect.left);
    ec.object_set_property(obj.clone(), "x", vx)?;
    ec.object_set_property(obj.clone(), "y", vy)?;
    ec.object_set_property(obj.clone(), "width", vw)?;
    ec.object_set_property(obj.clone(), "height", vh)?;
    ec.object_set_property(obj.clone(), "top", vt)?;
    ec.object_set_property(obj.clone(), "right", vr)?;
    ec.object_set_property(obj.clone(), "bottom", vb)?;
    ec.object_set_property(obj.clone(), "left", vl)?;
    Ok(crate::js::Types::value_from_object(obj))
}
