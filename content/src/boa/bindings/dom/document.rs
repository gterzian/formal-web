use std::rc::Rc;

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    object::{JsObject, builtins::JsArray},
    property::Attribute,
};

use crate::boa::platform_objects::{
    document_object, invalidate_cached_node_ids, resolve_element_object,
    resolve_or_create_text_node_object,
};
use crate::dom::Document;
use crate::webidl::binding::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, register_interface,
};

// ── WebIDL interface definition (§3) ──

impl WebIdlInterface for Document {
    const NAME: &'static str = "Document";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "getElementById",
            length: 1,
            method: get_element_by_id,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
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
            id: "getElementsByTagName",
            length: 1,
            method: get_elements_by_tag_name,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "createElement",
            length: 1,
            method: create_element,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "createElementNS",
            length: 2,
            method: create_element_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "createTextNode",
            length: 1,
            method: create_text_node,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });
        def.add_operation(OperationDef {
            id: "createComment",
            length: 1,
            method: create_comment,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });

        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
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

// ── Boa Class glue ──

impl Class for Document {
    const NAME: &'static str = "Document";

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
        register_interface::<Document>(class)
    }
}

fn with_document<R>(this: &JsValue, f: impl FnOnce(&Document) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("document receiver is not an object"))?;
    let document = object
        .downcast_ref::<Document>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not a Document"))?;
    Ok(f(&document))
}

fn get_element_by_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let id = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.get_element_by_id(&id))?;
    match node_id {
        Some(node_id) => Ok(resolve_element_object(node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn query_selector(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let selector = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.query_selector(&selector))?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    match node_id {
        Some(node_id) => Ok(resolve_element_object(node_id, context)?.into()),
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
    let node_ids = with_document(this, |document| document.query_selector_all(&selector))?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| resolve_element_object(node_id, context).map(JsValue::from))
        .collect::<JsResult<Vec<_>>>()?;
    Ok(JsArray::from_iter(values, context).into())
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let qualified_name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_ids = with_document(this, |document| {
        document.get_elements_by_tag_name(&qualified_name)
    })?
    .map_err(|error| JsNativeError::syntax().with_message(error))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| resolve_element_object(node_id, context).map(JsValue::from))
        .collect::<JsResult<Vec<_>>>()?;
    Ok(JsArray::from_iter(values, context).into())
}

fn create_element(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let local_name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.create_element(&local_name))?;
    Ok(resolve_element_object(node_id, context)?.into())
}

fn create_element_ns(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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
    let node_id = with_document(this, |document| {
        document.create_element_ns(namespace.as_deref(), &qualified_name)
    })?
    .map_err(|error| JsNativeError::syntax().with_message(error))?;
    Ok(resolve_element_object(node_id, context)?.into())
}

fn create_text_node(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let text = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let (document, node_id) = with_document(this, |document| {
        (
            Rc::clone(&document.node.document),
            document.create_text_node(&text),
        )
    })?;
    Ok(resolve_or_create_text_node_object(document, node_id, context)?.into())
}

fn create_comment(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let data = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let (document, node_id) = with_document(this, |document| {
        (
            Rc::clone(&document.node.document),
            document.create_comment(&data),
        )
    })?;
    Ok(resolve_or_create_text_node_object(document, node_id, context)?.into())
}

fn get_body(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let node_id = with_document(this, Document::body)?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    match node_id {
        Some(node_id) => Ok(resolve_element_object(node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_document_element(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    match with_document(this, Document::document_element)? {
        Some(node_id) => Ok(resolve_element_object(node_id, context)?.into()),
        None => Ok(JsValue::null()),
    }
}

fn get_title(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_document(this, |document| {
        JsValue::from(JsString::from(document.title().as_str()))
    })
}

fn set_title(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let title = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let dropped_node_ids = with_document(this, Document::title_subtree_node_ids)?;
    invalidate_cached_node_ids(context, &dropped_node_ids)?;
    with_document(this, |document| document.set_title(&title))?;
    Ok(JsValue::undefined())
}

fn get_dir(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_document(this, |document| {
        JsValue::from(JsString::from(document.dir().as_str()))
    })
}

fn set_dir(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let dir = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_document(this, |document| document.set_dir(&dir))?;
    Ok(JsValue::undefined())
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
