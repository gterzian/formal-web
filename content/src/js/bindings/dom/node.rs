use std::marker::PhantomData;
use std::rc::Rc;

use blitz_dom::NodeData;
use boa_engine::{
    JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue, object::builtins::JsArray,
};
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

use crate::dom::{DOMException, Document, Element, Node};
use crate::html::{HTMLAnchorElement, HTMLElement, HTMLIFrameElement};
use crate::js::platform_objects::{
    collect_child_subtree_node_ids, document_object, invalidate_cached_node_ids,
    object_for_existing_node,
};
use crate::webidl::bindings::{
    AttributeDef, ConstantDef, InterfaceDefinition, OperationDef, WebIdlInterface,
    create_interface_instance,
};

impl WebIdlInterface<js_engine::boa::BoaTypes> for Node {
    const NAME: &'static str = "Node";
    fn parent_name() -> Option<&'static str> {
        Some("EventTarget")
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        use boa_engine::JsValue;
        // §3.7.5: Constants
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ELEMENT_NODE",
            value: JsValue::from(1),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ATTRIBUTE_NODE",
            value: JsValue::from(2),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "TEXT_NODE",
            value: JsValue::from(3),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "CDATA_SECTION_NODE",
            value: JsValue::from(4),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ENTITY_REFERENCE_NODE",
            value: JsValue::from(5),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "ENTITY_NODE",
            value: JsValue::from(6),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "PROCESSING_INSTRUCTION_NODE",
            value: JsValue::from(7),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "COMMENT_NODE",
            value: JsValue::from(8),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_NODE",
            value: JsValue::from(9),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_TYPE_NODE",
            value: JsValue::from(10),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_FRAGMENT_NODE",
            value: JsValue::from(11),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "NOTATION_NODE",
            value: JsValue::from(12),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_DISCONNECTED",
            value: JsValue::from(0x01),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_PRECEDING",
            value: JsValue::from(0x02),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_FOLLOWING",
            value: JsValue::from(0x04),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_CONTAINS",
            value: JsValue::from(0x08),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_CONTAINED_BY",
            value: JsValue::from(0x10),
        });
        def.add_constant(ConstantDef {
            _phantom: PhantomData,

            id: "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",
            value: JsValue::from(0x20),
        });

        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "nodeType",
            getter: get_node_type,
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

            id: "ownerDocument",
            getter: get_owner_document,
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

            id: "parentNode",
            getter: get_parent_node,
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

            id: "childNodes",
            getter: get_child_nodes,
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

            id: "firstChild",
            getter: get_first_child,
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

            id: "lastChild",
            getter: get_last_child,
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

            id: "previousSibling",
            getter: get_previous_sibling,
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

            id: "nextSibling",
            getter: get_next_sibling,
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

            id: "nodeName",
            getter: get_node_name,
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

            id: "nodeValue",
            getter: get_node_value,
            setter: Some(set_node_value),
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

            id: "textContent",
            getter: get_text_content,
            setter: Some(set_text_content),
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

            id: "hasChildNodes",
            length: 0,
            method: has_child_nodes,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "appendChild",
            length: 1,
            method: append_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "insertBefore",
            length: 2,
            method: insert_before,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "removeChild",
            length: 1,
            method: remove_child,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "remove",
            length: 0,
            method: remove,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
    }
}

// ── Member getters/setters/methods ──

pub(crate) fn with_node_ref<R>(this: &JsValue, f: impl FnOnce(&Node) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("node receiver is not an object"))?;
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(f(&node));
    }
    if let Some(document) = object.downcast_ref::<Document>() {
        return Ok(f(&document.node));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(f(&element.node));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(f(&html_element.element.node));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(f(&html_anchor_element.html_element.element.node));
    }
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(f(&html_iframe_element.html_element.element.node));
    }
    Err(JsNativeError::typ()
        .with_message("receiver is not a Node")
        .into())
}

fn get_text_content(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        with_node_ref(this, |node| match node.text_content() {
            Some(content) => JsValue::from(JsString::from(content.as_str())),
            None => JsValue::null(),
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_first_child(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_id) =
            with_node_ref(this, |node| (Rc::clone(&node.document), node.first_child()))?;
        match node_id {
            Some(node_id) => Ok(object_for_existing_node(document, node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_last_child(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_id) =
            with_node_ref(this, |node| (Rc::clone(&node.document), node.last_child()))?;
        match node_id {
            Some(node_id) => Ok(object_for_existing_node(document, node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_parent_node(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_id) =
            with_node_ref(this, |node| (Rc::clone(&node.document), node.parent_node()))?;
        match node_id {
            Some(0) => Ok(document_object(ctx)?.into()),
            Some(node_id) => Ok(object_for_existing_node(document, node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_previous_sibling(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_id) = with_node_ref(this, |node| {
            (Rc::clone(&node.document), node.previous_sibling())
        })?;
        match node_id {
            Some(node_id) => Ok(object_for_existing_node(document, node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_next_sibling(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_id) = with_node_ref(this, |node| {
            (Rc::clone(&node.document), node.next_sibling())
        })?;
        match node_id {
            Some(node_id) => Ok(object_for_existing_node(document, node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_child_nodes(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let (document, node_ids) = with_node_ref(this, |node| {
            (Rc::clone(&node.document), node.child_node_ids())
        })?;
        let values = node_ids
            .into_iter()
            .map(|node_id| {
                object_for_existing_node(Rc::clone(&document), node_id, ctx).map(JsValue::from)
            })
            .collect::<JsResult<Vec<_>>>()?;
        Ok(JsArray::from_iter(values, ctx).into())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_node_type(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> { with_node_ref(this, |node| JsValue::from(node.node_type())) })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_node_name(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        with_node_ref(this, |node| {
            JsValue::from(JsString::from(node.node_name().as_str()))
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_owner_document(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let owner_document = with_node_ref(this, Node::owner_document_node_id)?;
        match owner_document {
            Some(0) => Ok(document_object(ctx)?.into()),
            Some(node_id) => {
                let document = with_node_ref(this, |node| Rc::clone(&node.document))?;
                Ok(object_for_existing_node(document, node_id, ctx)?.into())
            }
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_node_value(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        with_node_ref(this, |node| match node.node_value() {
            Some(value) => JsValue::from(JsString::from(value.as_str())),
            None => JsValue::null(),
        })
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_node_value(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let value = args.get_or_undefined(0);
        let value = if value.is_null() {
            None
        } else {
            Some(value.to_string(ctx)?.to_std_string_escaped())
        };
        with_node_ref(this, |node| node.set_node_value(value.as_deref()))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn set_text_content(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let value = args.get_or_undefined(0);
        let text = if value.is_null() {
            None
        } else {
            Some(value.to_string(ctx)?.to_std_string_escaped())
        };
        let dropped_node_ids = with_node_ref(this, |node| {
            let should_invalidate = {
                let document = node.document.borrow();
                document
                    .get_node(node.node_id)
                    .is_some_and(|current| matches!(current.data, NodeData::Element(_)))
            };

            if should_invalidate {
                collect_child_subtree_node_ids(&node.document, node.node_id)
            } else {
                Vec::new()
            }
        })?;
        invalidate_cached_node_ids(ctx, &dropped_node_ids)?;
        with_node_ref(this, |node| {
            node.set_text_content(text.as_deref());
        })?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn append_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let child = appendable_node(args.get_or_undefined(0))?;
        with_node_ref(this, |node| node.append_child(&child))?
            .map_err(|error| dom_exception_error(error, crate::js::context_as_ec(ctx)))?;
        Ok(args.get_or_undefined(0).clone())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn insert_before(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let child = appendable_node(args.get_or_undefined(0))?;
        let reference_child = match args.get_or_undefined(1) {
            value if value.is_null() || value.is_undefined() => None,
            value => Some(appendable_node(value)?),
        };
        with_node_ref(this, |node| {
            node.insert_before(&child, reference_child.as_ref())
        })?
        .map_err(|error| dom_exception_error(error, crate::js::context_as_ec(ctx)))?;
        Ok(args.get_or_undefined(0).clone())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn remove_child(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let child = appendable_node(args.get_or_undefined(0))?;
        with_node_ref(this, |node| node.remove_child(&child))?
            .map_err(|error| JsNativeError::typ().with_message(error))?;
        Ok(args.get_or_undefined(0).clone())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn remove(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        with_node_ref(this, Node::remove)?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn has_child_nodes(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> { with_node_ref(this, |node| JsValue::from(node.has_child_nodes())) })(
    )
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn appendable_node(value: &JsValue) -> JsResult<Node> {
    let Some(object) = value.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("appendChild requires a Node")
            .into());
    };
    if let Some(node) = object.downcast_ref::<Node>() {
        return Ok(Node::new(Rc::clone(&node.document), node.node_id));
    }
    if let Some(element) = object.downcast_ref::<Element>() {
        return Ok(Node::new(
            Rc::clone(&element.node.document),
            element.node.node_id,
        ));
    }
    if let Some(html_element) = object.downcast_ref::<HTMLElement>() {
        return Ok(Node::new(
            Rc::clone(&html_element.element.node.document),
            html_element.element.node.node_id,
        ));
    }
    if let Some(html_anchor_element) = object.downcast_ref::<HTMLAnchorElement>() {
        return Ok(Node::new(
            Rc::clone(&html_anchor_element.html_element.element.node.document),
            html_anchor_element.html_element.element.node.node_id,
        ));
    }
    if let Some(html_iframe_element) = object.downcast_ref::<HTMLIFrameElement>() {
        return Ok(Node::new(
            Rc::clone(&html_iframe_element.html_element.element.node.document),
            html_iframe_element.html_element.element.node.node_id,
        ));
    }
    Err(JsNativeError::typ()
        .with_message("appendChild requires a Node")
        .into())
}

fn dom_exception_error(
    exception: DOMException,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> JsError {
    JsError::from_opaque(JsValue::from(
        create_interface_instance::<BoaTypes, DOMException>(exception, ec)
            .expect("DOMException construction should not fail"),
    ))
}
