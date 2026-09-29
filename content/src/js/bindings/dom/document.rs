use std::cell::RefCell;
use std::rc::Rc;

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::dom::{Attr, DOMException, Document};
use crate::js::bindings::html::global_event_handlers::define_global_event_handlers;
use crate::js::platform_objects::{
    document_object, invalidate_cached_node_ids, object_for_existing_node,
    resolve_or_create_text_node_object, with_global_scope,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

impl WebIdlInterface<crate::js::Types> for Document {
    const NAME: &'static str = "Document";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        define_global_event_handlers(def);
        // §3.7.7: Regular operations
        def.add_operation(OperationDef {
            id: "getElementById",
            length: 1,
            method: get_element_by_id,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
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
            id: "getElementsByTagName",
            length: 1,
            method: get_elements_by_tag_name,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createElement",
            length: 1,
            method: create_element,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createElementNS",
            length: 2,
            method: create_element_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createTextNode",
            length: 1,
            method: create_text_node,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createComment",
            length: 1,
            method: create_comment,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createAttribute",
            length: 1,
            method: create_attribute,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
        def.add_operation(OperationDef {
            id: "createAttributeNS",
            length: 2,
            method: create_attribute_ns,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });

        // §3.7.6: Regular attributes
        def.add_attribute(AttributeDef {
            id: "implementation",
            getter: get_implementation,
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
            id: "head",
            getter: get_head,
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
            id: "currentScript",
            getter: get_current_script,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
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
            exposed: None,
        });
    }
}

fn try_with_document<R>(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
    f: impl FnOnce(&Document, &mut dyn ExecutionContext<crate::js::Types>) -> R,
) -> Completion<R, crate::js::Types> {
    let obj = crate::js::Types::value_as_object(this)
        .ok_or_else(|| ec.new_type_error("document receiver is not an object"))?;
    // Clone the handle out of the object registry so `f` can borrow `ec`
    // mutably; the clone shares all GC-managed state with the registered
    // platform object.
    let document = ec
        .with_object_any(&obj)
        .and_then(|data| data.downcast_ref::<Document>().cloned());
    let Some(document) = document else {
        return Err(ec.new_type_error("receiver is not a Document"));
    };
    Ok(f(&document, ec))
}

fn document_of(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<Document, crate::js::Types> {
    try_with_document(this, ec, |document, _ec| document.clone())
}

fn node_value(
    document: &Document,
    node_id: usize,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let obj = object_for_existing_node(Rc::clone(&document.node.document), node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn nullable_node_value(
    document: &Document,
    node_id: Option<usize>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    match node_id {
        Some(node_id) => node_value(document, node_id, ec),
        None => Ok(ec.value_null()),
    }
}

fn node_array_value(
    document: &Document,
    node_ids: Vec<usize>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let value = node_value(document, node_id, ec)?;
        ec.array_push(&array, value)?;
    }
    Ok(crate::js::Types::value_from_object(array))
}

fn get_element_by_id(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let id = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let document = document_of(this, ec)?;
    let node_id = document.get_element_by_id(&id);
    nullable_node_value(&document, node_id, ec)
}

fn query_selector(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let document = document_of(this, ec)?;
    let node_id = document
        .query_selector(&selector)
        .map_err(|error| ec.new_syntax_error(&error))?;
    nullable_node_value(&document, node_id, ec)
}

fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let selector = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let document = document_of(this, ec)?;
    let node_ids = document
        .query_selector_all(&selector)
        .map_err(|error| ec.new_syntax_error(&error))?;
    node_array_value(&document, node_ids, ec)
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let qualified_name =
        ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let document = document_of(this, ec)?;
    let node_ids = document
        .get_elements_by_tag_name(&qualified_name)
        .map_err(|error| ec.new_syntax_error(&error))?;
    node_array_value(&document, node_ids, ec)
}

fn create_element(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let local_name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let document = document_of(this, ec)?;
    let node_id = document.create_element(&local_name);
    node_value(&document, node_id, ec)
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
    let qualified_name =
        ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined.clone()))?;
    let document = document_of(this, ec)?;
    let node_id = document
        .create_element_ns(namespace.as_deref(), &qualified_name)
        .map_err(|error| ec.new_syntax_error(&error))?;
    node_value(&document, node_id, ec)
}

fn create_text_node(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let text = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let (document, node_id) = try_with_document(this, ec, |document, _ec| {
        (
            Rc::clone(&document.node.document),
            document.create_text_node(&text),
        )
    })?;
    let obj = resolve_or_create_text_node_object(document, node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn attr_value(attr: Attr, ec: &mut dyn ExecutionContext<crate::js::Types>) -> JsValue {
    attr.event_target
        .reflector
        .clone()
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|| ec.value_null())
}

fn dom_exception_value(
    error: DOMException,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> JsValue {
    create_interface_instance::<crate::js::Types, DOMException>(error, ec)
        .map(crate::js::Types::value_from_object)
        .unwrap_or_else(|err| err)
}

fn create_attribute(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let local_name = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let document = try_with_document(this, ec, |document, _ec| document.clone())?;
    let attr = document
        .create_attribute(&local_name, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(attr, ec))
}

fn create_attribute_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let namespace = match args.first() {
        Some(value)
            if !crate::js::Types::value_is_null(value)
                && !crate::js::Types::value_is_undefined(value) =>
        {
            Some(ec.to_rust_string(value.clone())?)
        }
        _ => None,
    };
    let qualified_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined))?;
    let document = try_with_document(this, ec, |document, _ec| document.clone())?;
    let attr = document
        .create_attribute_ns(namespace.as_deref(), &qualified_name, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(attr, ec))
}

fn create_comment(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let data = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let (document, node_id) = try_with_document(this, ec, |document, _ec| {
        (
            Rc::clone(&document.node.document),
            document.create_comment(&data),
        )
    })?;
    let obj = resolve_or_create_text_node_object(document, node_id, ec)?;
    Ok(crate::js::Types::value_from_object(obj))
}

fn get_body(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document = document_of(this, ec)?;
    let node_id = document
        .body()
        .map_err(|error| ec.new_syntax_error(&error))?;
    nullable_node_value(&document, node_id, ec)
}

fn get_implementation(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document = try_with_document(this, ec, |document, _ec| document.clone())?;
    document.implementation(ec)?;
    let object = with_global_scope(ec, |global_scope, ec| {
        global_scope
            .dom_implementation_object(ec)
            .ok_or_else(|| ec.new_type_error("document has no DOMImplementation object"))
    })?;
    Ok(crate::js::Types::value_from_object(object))
}

fn get_head(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document = document_of(this, ec)?;
    let node_id = document
        .head()
        .map_err(|error| ec.new_syntax_error(&error))?;
    nullable_node_value(&document, node_id, ec)
}

fn get_current_script(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document = document_of(this, ec)?;
    let node_id = document.current_script();
    nullable_node_value(&document, node_id, ec)
}

fn get_document_element(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let document = document_of(this, ec)?;
    let node_id = document.document_element();
    nullable_node_value(&document, node_id, ec)
}

fn get_title(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let title = try_with_document(this, ec, |document, _ec| document.title())?;
    Ok(ec.value_from_string(ec.js_string_from_str(title.as_str())))
}

fn set_title(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let title = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    let dropped_node_ids = try_with_document(this, ec, |document, _ec| {
        Document::title_subtree_node_ids(document)
    })?;
    invalidate_cached_node_ids(ec, &dropped_node_ids)?;
    try_with_document(this, ec, |document, ec| document.set_title(&title, ec))?;
    Ok(ec.value_undefined())
}

fn get_dir(
    this: &JsValue,
    _: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let dir = try_with_document(this, ec, |document, ec| document.dir(ec))?;
    Ok(ec.value_from_string(ec.js_string_from_str(dir.as_str())))
}

fn set_dir(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let value_undefined = ec.value_undefined();
    let dir = ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined))?;
    try_with_document(this, ec, |document, ec| document.set_dir(&dir, ec))?;
    Ok(ec.value_undefined())
}

pub(crate) fn install_document_property(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    let document = document_object(ec)?;
    let global = ec.realm_global_object();
    let key = ec.property_key_from_str("document");
    let value = <crate::js::Types as js_engine::JsTypes>::value_from_object(document);

    // Step 1: Define the "document" property on the global object.
    // Note: This replaces register_global_property which is Boa-specific.
    // The property is writable, enumerable, configurable (same as Attribute::all()).
    ec.define_property_or_throw(
        global,
        key,
        js_engine::PropertyDescriptor {
            value: Some(value),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            get: None,
            set: None,
        },
    )?;
    Ok(())
}

/// <https://webidl.spec.whatwg.org/#internally-create-a-new-object-implementing-the-interface>
// Note: This function does not implement the algorithm itself — it delegates
// to `create_interface_instance` which does. This is a Document-specific
// wrapper that also extracts a cloned Document reference for the ESO, which
// is an artifact of Rust's ownership model (the spec has no such concept).
pub(crate) fn create_document_platform_object(
    blitz_document: Rc<RefCell<blitz_dom::BaseDocument>>,
    creation_url: url::Url,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<
    (
        <crate::js::Types as JsTypes>::JsObject,
        crate::dom::Document,
    ),
    crate::js::Types,
> {
    let document = crate::dom::Document::new(blitz_document, creation_url, ec);
    let document_object =
        create_interface_instance::<crate::js::Types, crate::dom::Document>(document, ec)?;

    // The ESO needs a Document reference for access to shared GcCell-backed
    // state. The reflector was set automatically by
    // PostCreateReflector::set_reflector during create_interface_instance.
    let extracted: crate::dom::Document = ec
        .with_object_any(&document_object)
        .and_then(|data| data.downcast_ref::<crate::dom::Document>().cloned())
        .ok_or_else(|| ec.new_type_error("document_object is not a Document"))?;

    Ok((document_object, extracted))
}
