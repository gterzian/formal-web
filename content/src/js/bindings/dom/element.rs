type JsValue = <crate::js::Types as JsTypes>::JsValue;
type OperationMethod = fn(
    &JsValue,
    &[JsValue],
    &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types>;

use crate::dom::{Attr, DOMException, Element};
use crate::html::{
    HTMLAnchorElement, HTMLCanvasElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLLinkElement, HTMLMediaElement, HTMLScriptElement, HTMLVideoElement,
};
use crate::js::bindings::html::global_event_handlers::define_global_event_handlers;
use crate::js::bindings::this_as;
use crate::js::platform_objects::{invalidate_cached_node_ids, object_for_existing_node};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for Element {
    const NAME: &'static str = "Element";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        define_global_event_handlers(def);
        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            id: "className",
            getter: get_class_name,
            setter: Some(set_class_name),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "id",
            getter: get_id,
            setter: Some(set_id),
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
        });

        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "querySelector",
            length: 1,
            method: query_selector,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "querySelectorAll",
            length: 1,
            method: query_selector_all,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "matches",
            length: 1,
            method: matches,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "closest",
            length: 1,
            method: closest,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "insertAdjacentText",
            length: 2,
            method: insert_adjacent_text,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "setAttribute",
            length: 2,
            method: set_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "setAttributeNS",
            length: 3,
            method: set_attribute_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "getAttribute",
            length: 1,
            method: get_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "hasAttribute",
            length: 1,
            method: has_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "removeAttribute",
            length: 1,
            method: remove_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_attribute(AttributeDef {
            id: "attributes",
            getter: get_attributes,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
        for (id, length, method) in [
            ("hasAttributes", 0, has_attributes as OperationMethod),
            ("getAttributeNames", 0, get_attribute_names),
            ("getAttributeNS", 2, get_attribute_ns),
            ("hasAttributeNS", 2, has_attribute_ns),
            ("removeAttributeNS", 2, remove_attribute_ns),
            ("toggleAttribute", 1, toggle_attribute),
            ("getAttributeNode", 1, get_attribute_node),
            ("getAttributeNodeNS", 2, get_attribute_node_ns),
            ("setAttributeNode", 1, set_attribute_node),
            ("setAttributeNodeNS", 1, set_attribute_node),
            ("removeAttributeNode", 1, remove_attribute_node),
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
        def.add_operation(OperationDef {
            id: "getBoundingClientRect",
            length: 0,
            method: get_bounding_client_rect,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

pub(crate) fn try_with_element_ref<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Element) -> R,
) -> Completion<R, crate::js::Types> {
    let object = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("element receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&object) {
        if let Some(element) = data.downcast_ref::<Element>() {
            return Ok(f(element));
        }
        if let Some(html_element) = data.downcast_ref::<HTMLElement>() {
            return Ok(f(&html_element.element));
        }
        if let Some(html_anchor_element) = data.downcast_ref::<HTMLAnchorElement>() {
            return Ok(f(&html_anchor_element.html_element.element));
        }
        if let Some(html_canvas_element) = data.downcast_ref::<HTMLCanvasElement>() {
            return Ok(f(&html_canvas_element.html_element.element));
        }
        if let Some(html_iframe_element) = data.downcast_ref::<HTMLIFrameElement>() {
            return Ok(f(&html_iframe_element.html_element.element));
        }
        if let Some(html_script_element) = data.downcast_ref::<HTMLScriptElement>() {
            return Ok(f(&html_script_element.html_element.element));
        }
        if let Some(html_link_element) = data.downcast_ref::<HTMLLinkElement>() {
            return Ok(f(&html_link_element.html_element.element));
        }
        if let Some(html_input_element) = data.downcast_ref::<HTMLInputElement>() {
            return Ok(f(&html_input_element.html_element.element));
        }
        if let Some(html_media_element) = data.downcast_ref::<HTMLMediaElement>() {
            return Ok(f(&html_media_element.html_element.element));
        }
        if let Some(html_video_element) = data.downcast_ref::<HTMLVideoElement>() {
            return Ok(f(&html_video_element.media_element.html_element.element));
        }
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

fn set_id(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_element_ref(this, ec, |element| element.set_id(&value))?;
    Ok(ec.value_undefined())
}

fn get_class_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let class_name = try_with_element_ref(this, ec, |element| element.class_name())?;
    Ok(ec.value_from_string(ec.js_string_from_str(&class_name)))
}

fn set_class_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    try_with_element_ref(this, ec, |element| element.set_class_name(&value))?;
    Ok(ec.value_undefined())
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
    let add_fn = {
        let name_key = ec.property_key_from_str("add");
        let capture = crate::js::FnCapture {
            func: class_list_add,
        };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            crate::js::fn_capture_behaviour,
            1,
            name_key,
            false,
        )
    };
    ec.object_set_property(
        token_list.clone(),
        "add",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(add_fn)),
    )?;

    // "remove" method
    let remove_fn = {
        let name_key = ec.property_key_from_str("remove");
        let capture = crate::js::FnCapture {
            func: class_list_remove,
        };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            crate::js::fn_capture_behaviour,
            1,
            name_key,
            false,
        )
    };
    ec.object_set_property(
        token_list.clone(),
        "remove",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(remove_fn)),
    )?;

    // "toggle" method
    let toggle_fn = {
        let name_key = ec.property_key_from_str("toggle");
        let capture = crate::js::FnCapture {
            func: class_list_toggle,
        };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            crate::js::fn_capture_behaviour,
            1,
            name_key,
            false,
        )
    };
    ec.object_set_property(
        token_list.clone(),
        "toggle",
        crate::js::Types::value_from_object(crate::js::Types::object_from_function(toggle_fn)),
    )?;

    // "contains" method
    let contains_fn = {
        let name_key = ec.property_key_from_str("contains");
        let capture = crate::js::FnCapture {
            func: class_list_contains,
        };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            crate::js::fn_capture_behaviour,
            1,
            name_key,
            false,
        )
    };
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
    let len_fn = {
        let name_key = ec.property_key_from_str("get_length");
        let capture = crate::js::FnCapture {
            func: class_list_length,
        };
        crate::js::create_builtin_fn_with_traced_captures(
            ec,
            capture,
            crate::js::fn_capture_behaviour,
            0,
            name_key,
            false,
        )
    };
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

    let err = ec.new_type_error("classList: element data not found");
    let data = ec.with_object_any(&element_obj).ok_or(err)?;

    if let Some(el) = data.downcast_ref::<Element>() {
        return Ok(el.get_attribute("class").unwrap_or_default());
    }
    if let Some(html_el) = data.downcast_ref::<HTMLElement>() {
        return Ok(html_el.element.get_attribute("class").unwrap_or_default());
    }
    if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
        return Ok(media
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
        return Ok(video
            .media_element
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(ifr) = data.downcast_ref::<HTMLIFrameElement>() {
        return Ok(ifr
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(link) = data.downcast_ref::<HTMLLinkElement>() {
        return Ok(link
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(script) = data.downcast_ref::<HTMLScriptElement>() {
        return Ok(script
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
        return Ok(input
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(anc) = data.downcast_ref::<HTMLAnchorElement>() {
        return Ok(anc
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    if let Some(canvas) = data.downcast_ref::<HTMLCanvasElement>() {
        return Ok(canvas
            .html_element
            .element
            .get_attribute("class")
            .unwrap_or_default());
    }
    Ok(String::new())
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

    let set_class = |element: &Element| {
        if value.is_empty() {
            element.remove_an_attribute_by_name("class");
        } else {
            element.set_an_attribute_value("class", value, None, None);
        }
    };

    if let Some(data) = ec.with_object_any(&element_obj) {
        if let Some(el) = data.downcast_ref::<Element>() {
            set_class(el);
        } else if let Some(html_el) = data.downcast_ref::<HTMLElement>() {
            set_class(&html_el.element);
        } else if let Some(media) = data.downcast_ref::<HTMLMediaElement>() {
            set_class(&media.html_element.element);
        } else if let Some(video) = data.downcast_ref::<HTMLVideoElement>() {
            set_class(&video.media_element.html_element.element);
        } else if let Some(ifr) = data.downcast_ref::<HTMLIFrameElement>() {
            set_class(&ifr.html_element.element);
        } else if let Some(script) = data.downcast_ref::<HTMLScriptElement>() {
            set_class(&script.html_element.element);
        } else if let Some(link) = data.downcast_ref::<HTMLLinkElement>() {
            set_class(&link.html_element.element);
        } else if let Some(input) = data.downcast_ref::<HTMLInputElement>() {
            set_class(&input.html_element.element);
        } else if let Some(anc) = data.downcast_ref::<HTMLAnchorElement>() {
            set_class(&anc.html_element.element);
        } else if let Some(canvas) = data.downcast_ref::<HTMLCanvasElement>() {
            set_class(&canvas.html_element.element);
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
    let (document, node_id) = try_with_element_ref(this, ec, |element| {
        (
            element.node.document.clone(),
            element.query_selector(&selector),
        )
    })?;
    match node_id.map_err(|error| ec.new_syntax_error(&error))? {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn matches(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let matched = try_with_element_ref(this, ec, |element| element.matches(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    Ok(ec.value_from_bool(matched))
}

fn closest(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let (document, node_id) = try_with_element_ref(this, ec, |element| {
        (element.node.document.clone(), element.closest(&selector))
    })?;
    match node_id.map_err(|error| ec.new_syntax_error(&error))? {
        Some(node_id) => {
            let obj = object_for_existing_node(document, node_id, ec)?;
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
    let (document, node_ids) = try_with_element_ref(this, ec, |element| {
        (
            element.node.document.clone(),
            element.query_selector_all(&selector),
        )
    })?;
    let node_ids = node_ids.map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = object_for_existing_node(document.clone(), node_id, ec)?;
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
            .map(crate::js::Types::value_from_object)
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

fn dom_exception_value(
    error: DOMException,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsValue {
    create_interface_instance::<crate::js::Types, DOMException>(error, ec)
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|err| err)
}

fn nullable_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Option<String>, crate::js::Types> {
    match args.get(index) {
        Some(value)
            if !crate::js::Types::value_is_null(value)
                && !crate::js::Types::value_is_undefined(value) =>
        {
            Ok(Some(ec.to_rust_string(value.clone())?))
        }
        _ => Ok(None),
    }
}

fn attr_argument(
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Attr, crate::js::Types> {
    let undefined = ec.value_undefined();
    this_as::<Attr>(args.first().unwrap_or(&undefined), "Attr", ec)
}

fn attr_value(attr: Option<Attr>, ec: &mut dyn ExecutionContext<crate::js::Types>) -> JsValue {
    attr.and_then(|attr| attr.event_target.reflector.clone())
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null())
}

fn set_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let value = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| element.set_attribute(&name, &value))?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn set_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let qualified_name =
        ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined.clone()))?;
    let value = ec.to_rust_string(args.get(2).cloned().unwrap_or(value_undefined))?;
    try_with_element_ref(this, ec, |element| {
        element.set_attribute_ns(namespace.as_deref(), &qualified_name, &value)
    })?
    .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn remove_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    element.remove_attribute(&name, ec)?;
    Ok(ec.value_undefined())
}

fn get_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    match try_with_element_ref(this, ec, |element| {
        element.get_attribute_ns(namespace.as_deref(), &local_name)
    })? {
        Some(value) => Ok(ec.value_from_string(ec.js_string_from_str(value.as_str()))),
        None => Ok(ec.value_null()),
    }
}

fn has_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    let result = try_with_element_ref(this, ec, |element| {
        element.has_attribute_ns(namespace.as_deref(), &local_name)
    })?;
    Ok(ec.value_from_bool(result))
}

fn remove_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    element.remove_attribute_ns(namespace.as_deref(), &local_name, ec)?;
    Ok(ec.value_undefined())
}

fn has_attributes(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let result = try_with_element_ref(this, ec, Element::has_attributes)?;
    Ok(ec.value_from_bool(result))
}

fn get_attribute_names(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let names = try_with_element_ref(this, ec, Element::get_attribute_names)?;
    let array = ec.create_empty_array();
    for name in names {
        let value = ec.value_from_string(ec.js_string_from_str(&name));
        ec.array_push(&array, value)?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn toggle_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let force = match args.get(1) {
        Some(value) if !crate::js::Types::value_is_undefined(value) => Some(ec.to_boolean(value)),
        _ => None,
    };
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let toggled = element
        .toggle_attribute(&name, force, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_from_bool(toggled))
}

fn get_attributes(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let map = element.attributes(ec)?;
    map.reflector
        .clone()
        .map(crate::js::Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("NamedNodeMap without its object"))
}

fn get_attribute_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let attr = element.get_attribute_node(&name, ec)?;
    Ok(attr_value(attr, ec))
}

fn get_attribute_node_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let attr = element.get_attribute_node_ns(namespace.as_deref(), &local_name, ec)?;
    Ok(attr_value(attr, ec))
}

fn set_attribute_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let attr = attr_argument(args, ec)?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let old_attr = element
        .set_attribute_node(&attr, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(old_attr, ec))
}

fn remove_attribute_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let attr = attr_argument(args, ec)?;
    let element = try_with_element_ref(this, ec, Element::clone)?;
    let removed = element
        .remove_attribute_node(&attr, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(Some(removed), ec))
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
