use std::marker::PhantomData;
use std::rc::Rc;

use boa_engine::{
    Context, JsResult, JsValue, js_string,
    object::JsObject,
    property::Attribute,
};

use crate::dom::Document;
use crate::js::platform_objects::{
    document_object, invalidate_cached_node_ids_ec,
    resolve_element_object_ec,
    resolve_or_create_text_node_object_ec,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};

use js_engine::{Completion, ExecutionContext, JsTypes};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface<crate::js::Types> for Document {
    const NAME: &'static str = "Document";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "getElementById",
            length: 1,
            method: get_element_by_id,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
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

            id: "getElementsByTagName",
            length: 1,
            method: get_elements_by_tag_name,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "createElement",
            length: 1,
            method: create_element,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "createElementNS",
            length: 2,
            method: create_element_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "createTextNode",
            length: 1,
            method: create_text_node,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            _phantom: PhantomData,

            id: "createComment",
            length: 1,
            method: create_comment,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });

        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,

            id: "body",
            getter: get_body,
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

            id: "documentElement",
            getter: get_document_element,
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
    }
}

fn try_with_document<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Document) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("document receiver is not an object"))?;
    if let Some(data) = ec.with_object_any(&obj) {
        if let Some(doc) = data.downcast_ref::<Document>() {
            return Ok(f(doc));
        }
    }
    Err(ec.new_type_error("receiver is not a Document"))
}

fn get_element_by_id(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let id = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let node_id = try_with_document(this, ec, |document| document.get_element_by_id(&id))?;
    match node_id {
        Some(node_id) => {
            let obj = resolve_element_object_ec(node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn query_selector(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let node_id = try_with_document(this, ec, |document| document.query_selector(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    match node_id {
        Some(node_id) => {
            let obj = resolve_element_object_ec(node_id, ec)?;
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
    let node_ids = try_with_document(this, ec, |document| document.query_selector_all(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = resolve_element_object_ec(node_id, ec)?;
        ec.array_push(&array, crate::js::Types::value_from_object(obj))?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let qualified_name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let node_ids = try_with_document(this, ec, |document| {
        document.get_elements_by_tag_name(&qualified_name)
    })?
    .map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = resolve_element_object_ec(node_id, ec)?;
        ec.array_push(&array, crate::js::Types::value_from_object(obj))?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn create_element(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let local_name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let node_id = try_with_document(this, ec, |document| document.create_element(&local_name))?;
    let obj = resolve_element_object_ec(node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn create_element_ns(
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
    let qualified_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined.clone()))?;
    let node_id = try_with_document(this, ec, |document| {
        document.create_element_ns(namespace.as_deref(), &qualified_name)
    })?
    .map_err(|error| ec.new_syntax_error(&error))?;
    let obj = resolve_element_object_ec(node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn create_text_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let text = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let (document, node_id) = try_with_document(this, ec, |document| {
        (
            Rc::clone(&document.node.document),
            document.create_text_node(&text),
        )
    })?;
    let obj = resolve_or_create_text_node_object_ec(document, node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn create_comment(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let data = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let (document, node_id) = try_with_document(this, ec, |document| {
        (
            Rc::clone(&document.node.document),
            document.create_comment(&data),
        )
    })?;
    let obj = resolve_or_create_text_node_object_ec(document, node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn get_body(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let node_id = try_with_document(this, ec, Document::body)?
        .map_err(|error| ec.new_syntax_error(&error))?;
    match node_id {
        Some(node_id) => {
            let obj = resolve_element_object_ec(node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_document_element(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match try_with_document(this, ec, Document::document_element)? {
        Some(node_id) => {
            let obj = resolve_element_object_ec(node_id, ec)?;
            Ok(crate::js::Types::value_from_object(obj))
        }
        None => Ok(ec.value_null()),
    }
}

fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let title = try_with_document(this, ec, |document| document.title())?;
    Ok(ec.value_from_string(ec.js_string_from_str(title.as_str())))
}

fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let title = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let dropped_node_ids = try_with_document(this, ec, Document::title_subtree_node_ids)?;
    invalidate_cached_node_ids_ec(ec, &dropped_node_ids)?;
    try_with_document(this, ec, |document| document.set_title(&title))?;
    Ok(ec.value_undefined())
}

fn get_dir(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let dir = try_with_document(this, ec, |document| document.dir())?;
    Ok(ec.value_from_string(ec.js_string_from_str(dir.as_str())))
}

fn set_dir(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let dir = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_document(this, ec, |document| document.set_dir(&dir))?;
    Ok(ec.value_undefined())
}

/// Install the `document` property on the global object using a pre-resolved
/// Document JsObject. Accepting the document as a parameter avoids an internal
/// `with_global_scope` call that would borrow the global object's RefCell,
/// which would then conflict with the subsequent `register_global_property`
/// that needs to mutably borrow the same global object.
pub(crate) fn install_document_property_with_object(
    context: &mut Context,
    document: JsObject,
) -> JsResult<()> {
    context.register_global_property(js_string!("document"), document, Attribute::all())
}

pub(crate) fn install_document_property(context: &mut Context) -> JsResult<()> {
    let document = document_object(context)?;
    install_document_property_with_object(context, document)
}
