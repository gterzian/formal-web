use std::marker::PhantomData;
use std::rc::Rc;

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsValue, js_string,
    object::{JsObject, builtins::JsArray},
    property::Attribute,
};

use crate::dom::Document;
use crate::js::platform_objects::{
    document_object, document_object_ec, invalidate_cached_node_ids,
    resolve_element_object, resolve_element_object_ec,
    resolve_or_create_text_node_object,
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

fn with_document<R>(this: &JsValue, f: impl FnOnce(&Document) -> R) -> JsResult<R> {
    let object = this
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("document receiver is not an object"))?;
    let document = object
        .downcast_ref::<Document>()
        .ok_or_else(|| JsNativeError::typ().with_message("receiver is not a Document"))?;
    Ok(f(&document))
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
    // Note: keeps ec_to_ctx — .to_string(ctx)? needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let id = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.get_element_by_id(&id))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?;
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
    // Note: keeps ec_to_ctx — .to_string(ctx)? needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let selector = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.query_selector(&selector))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .map_err(|error| JsNativeError::syntax().with_message(error))
        .map_err(|e| boa_engine::JsError::from(e).into_opaque(ctx).unwrap_or(undefined))?;
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
    // Note: keeps ec_to_ctx — .to_string(ctx)? and JsArray::from_iter.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let selector = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_ids = with_document(this, |document| document.query_selector_all(&selector))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .map_err(|error| JsNativeError::syntax().with_message(error))
        .map_err(|e| boa_engine::JsError::from(e).into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| resolve_element_object(node_id, ctx).map(JsValue::from))
        .collect::<JsResult<Vec<_>>>()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    Ok(JsArray::from_iter(values, ctx).into())
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — .to_string(ctx)? and JsArray::from_iter.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let qualified_name = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_ids = with_document(this, |document| {
        document.get_elements_by_tag_name(&qualified_name)
    })
    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
    .map_err(|error| JsNativeError::syntax().with_message(error))
    .map_err(|e| boa_engine::JsError::from(e).into_opaque(ctx).unwrap_or(undefined.clone()))?;
    let values = node_ids
        .into_iter()
        .map(|node_id| resolve_element_object(node_id, ctx).map(JsValue::from))
        .collect::<JsResult<Vec<_>>>()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?;
    Ok(JsArray::from_iter(values, ctx).into())
}

fn create_element(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — .to_string(ctx)? needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let local_name = args
        .get_or_undefined(0)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.create_element(&local_name))
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined))?;
    let obj = resolve_element_object_ec(node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn create_element_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: keeps ec_to_ctx — .to_string(ctx)? needs Context.
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let undefined = JsValue::undefined();
    let namespace =
        if args.get_or_undefined(0).is_null() || args.get_or_undefined(0).is_undefined() {
            None
        } else {
            Some(
                args.get_or_undefined(0)
                    .to_string(ctx)
                    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
                    .to_std_string_escaped(),
            )
        };
    let qualified_name = args
        .get_or_undefined(1)
        .to_string(ctx)
        .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| {
        document.create_element_ns(namespace.as_deref(), &qualified_name)
    })
    .map_err(|e| e.into_opaque(ctx).unwrap_or(undefined.clone()))?
    .map_err(|error| JsNativeError::syntax().with_message(error))
    .map_err(|e| boa_engine::JsError::from(e).into_opaque(ctx).unwrap_or(undefined))?;
    let obj = resolve_element_object_ec(node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn create_text_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let text = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        let (document, node_id) = with_document(this, |document| {
            (
                Rc::clone(&document.node.document),
                document.create_text_node(&text),
            )
        })?;
        Ok(resolve_or_create_text_node_object(document, node_id, ctx)?.into())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn create_comment(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let data = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        let (document, node_id) = with_document(this, |document| {
            (
                Rc::clone(&document.node.document),
                document.create_comment(&data),
            )
        })?;
        Ok(resolve_or_create_text_node_object(document, node_id, ctx)?.into())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_body(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let node_id = with_document(this, Document::body)?
            .map_err(|error| JsNativeError::syntax().with_message(error))?;
        match node_id {
            Some(node_id) => Ok(resolve_element_object(node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_document_element(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        match with_document(this, Document::document_element)? {
            Some(node_id) => Ok(resolve_element_object(node_id, ctx)?.into()),
            None => Ok(JsValue::null()),
        }
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("document receiver is not an object"))?;
    let document = obj
        .downcast_ref::<Document>()
        .ok_or_else(|| ec.new_type_error("receiver is not a Document"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(document.title().as_str())))
}

fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let title = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        let dropped_node_ids = with_document(this, Document::title_subtree_node_ids)?;
        invalidate_cached_node_ids(ctx, &dropped_node_ids)?;
        with_document(this, |document| document.set_title(&title))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_dir(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("document receiver is not an object"))?;
    let document = obj
        .downcast_ref::<Document>()
        .ok_or_else(|| ec.new_type_error("receiver is not a Document"))?;
    Ok(ec.value_from_string(ec.js_string_from_str(document.dir().as_str())))
}

fn set_dir(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { js_engine::boa::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
        let dir = args
            .get_or_undefined(0)
            .to_string(ctx)?
            .to_std_string_escaped();
        with_document(this, |document| document.set_dir(&dir))?;
        Ok(JsValue::undefined())
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
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
