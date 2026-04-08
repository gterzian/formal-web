use std::rc::Rc;

use boa_engine::{
    Context, JsArgs, JsNativeError, JsResult, JsString, JsValue,
    class::{Class, ClassBuilder},
    js_string,
    native_function::NativeFunction,
    object::builtins::JsArray,
    property::Attribute,
};

use crate::boa::platform_objects::{
    document_object, resolve_element_object, resolve_or_create_text_node_object,
};
use crate::dom::Document;

use super::{event_target::register_event_target_methods, node::register_node_methods};

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
        register_event_target_methods(class)?;
        register_node_methods(class)?;
        register_document_methods(class)
    }
}

pub(crate) fn register_document_methods(class: &mut ClassBuilder<'_>) -> JsResult<()> {
    let realm = class.context().realm().clone();
    class
        .method(
            js_string!("getElementById"),
            1,
            NativeFunction::from_fn_ptr(get_element_by_id),
        )
        .method(
            js_string!("querySelector"),
            1,
            NativeFunction::from_fn_ptr(query_selector),
        )
        .method(
            js_string!("querySelectorAll"),
            1,
            NativeFunction::from_fn_ptr(query_selector_all),
        )
        .method(
            js_string!("createElement"),
            1,
            NativeFunction::from_fn_ptr(create_element),
        )
        .method(
            js_string!("createTextNode"),
            1,
            NativeFunction::from_fn_ptr(create_text_node),
        )
        .accessor(
            js_string!("body"),
            Some(NativeFunction::from_fn_ptr(get_body).to_js_function(&realm)),
            None,
            Attribute::all(),
        )
        .accessor(
            js_string!("title"),
            Some(NativeFunction::from_fn_ptr(get_title).to_js_function(&realm)),
            Some(NativeFunction::from_fn_ptr(set_title).to_js_function(&realm)),
            Attribute::all(),
        );
    Ok(())
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

fn create_element(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let local_name = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let node_id = with_document(this, |document| document.create_element(&local_name))?;
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

fn get_body(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let node_id = with_document(this, Document::body)?
        .map_err(|error| JsNativeError::syntax().with_message(error))?;
    match node_id {
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
    with_document(this, |document| document.set_title(&title))?;
    Ok(JsValue::undefined())
}

pub(crate) fn install_document_property(context: &mut Context) -> JsResult<()> {
    let document = document_object(context)?;
    context.register_global_property(js_string!("document"), document, Attribute::all())
}
