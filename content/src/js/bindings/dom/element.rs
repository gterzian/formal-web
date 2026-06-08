use boa_engine::{
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue,
    js_string,
    object::{ObjectInitializer, builtins::JsArray},
    property::Attribute,
};

use crate::js::platform_objects::invalidate_cached_node_ids;
use crate::dom::{DOMException, Element};
use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement};
use crate::webidl::binding::{
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
                crate::webidl::binding::create_interface_instance::<DOMException>(error, context)
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
