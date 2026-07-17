use std::cell::RefCell;
use std::rc::Rc;

type JsValue = <crate::js::Types as JsTypes>::JsValue;

use crate::dom::Document;
use crate::js::platform_objects::{
    document_object, invalidate_cached_node_ids, resolve_element_object,
    resolve_or_create_text_node_object,
};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance};

use js_engine::{Completion, ExecutionContext, JsTypes};
use url::Url;

impl WebIdlInterface<crate::js::Types> for Document {
    const NAME: &'static str = "Document";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
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
            let obj = resolve_element_object(node_id, ec)?;
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
    let node_ids = try_with_document(this, ec, |document| document.query_selector_all(&selector))?
        .map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = resolve_element_object(node_id, ec)?;
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
    let qualified_name =
        ec.to_rust_string(args.first().cloned().unwrap_or(value_undefined.clone()))?;
    let node_ids = try_with_document(this, ec, |document| {
        document.get_elements_by_tag_name(&qualified_name)
    })?
    .map_err(|error| ec.new_syntax_error(&error))?;
    let array = ec.create_empty_array();
    for node_id in node_ids {
        let obj = resolve_element_object(node_id, ec)?;
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
    let obj = resolve_element_object(node_id, ec)?;
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
    let qualified_name =
        ec.to_rust_string(args.get(1).cloned().unwrap_or(value_undefined.clone()))?;
    let node_id = try_with_document(this, ec, |document| {
        document.create_element_ns(namespace.as_deref(), &qualified_name)
    })?
    .map_err(|error| ec.new_syntax_error(&error))?;
    let obj = resolve_element_object(node_id, ec)?;
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
    let obj = resolve_or_create_text_node_object(document, node_id, ec)?;
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
    let obj = resolve_or_create_text_node_object(document, node_id, ec)?;
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
            let obj = resolve_element_object(node_id, ec)?;
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
            let obj = resolve_element_object(node_id, ec)?;
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
    invalidate_cached_node_ids(ec, &dropped_node_ids)?;
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
pub(crate) fn create_document_platform_object(
    blitz_document: Rc<RefCell<blitz_dom::BaseDocument>>,
    creation_url: url::Url,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(<crate::js::Types as JsTypes>::JsObject, crate::dom::Document), crate::js::Types> {
    // Step 1: Assert: interface is exposed in realm.
    // (Checked inside create_interface_instance via prototype resolution.)
    // Step 9: Set instance.[[Prototype]] to prototype.
    // Step 11: Return instance.
    //
    // Internally create a new object implementing the Document interface.
    // The native data is moved into the JsObject; the returned JsObject
    // holds the canonical copy.
    let document = crate::dom::Document::new(blitz_document, creation_url);
    let document_object = create_interface_instance::<crate::js::Types, crate::dom::Document>(
        document,
        ec,
    )?;

    // The ESO needs a domain Document reference for access to shared
    // GcCell-backed state. Clone from the JsObject: GcCell fields share
    // their inner state, and the reflector was set automatically by
    // PostCreateReflector::set_reflector during create_interface_instance.
    // Note: This clone step is not part of the Web IDL spec — it is an
    // artifact of Rust's ownership model where the ESO needs both a
    // JsObject handle and a typed Rust reference to the same data.
    let extracted: crate::dom::Document = ec
        .with_object_any(&document_object)
        .and_then(|data| data.downcast_ref::<crate::dom::Document>().cloned())
        .ok_or_else(|| ec.new_type_error("document_object is not a Document"))?;

    Ok((document_object, extracted))
}
