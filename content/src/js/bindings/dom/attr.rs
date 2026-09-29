use crate::dom::Attr;
use crate::js::Types;
use crate::js::bindings::this_as;
use crate::js::platform_objects::object_for_existing_node;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

macro_rules! attribute {
    ($def:ident, $id:literal, $getter:ident, $setter:expr) => {
        $def.add_attribute(AttributeDef {
            id: $id,
            getter: $getter,
            setter: $setter,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
            exposed: None,
        });
    };
}

impl WebIdlInterface<Types> for Attr {
    const NAME: &'static str = "Attr";

    fn parent_name() -> Option<&'static str> {
        Some("Node")
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        attribute!(def, "namespaceURI", get_namespace_uri, None);
        attribute!(def, "prefix", get_prefix, None);
        attribute!(def, "localName", get_local_name, None);
        attribute!(def, "name", get_name, None);
        attribute!(def, "value", get_value, Some(set_value));
        attribute!(def, "ownerElement", get_owner_element, None);
        attribute!(def, "specified", get_specified, None);
    }
}

fn attr(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Attr, Types> {
    this_as::<Attr>(this, "Attr", ec)
}

fn nullable_string_value(value: Option<String>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match value {
        Some(value) => {
            let string = ec.js_string_from_str(&value);
            ec.value_from_string(string)
        }
        None => ec.value_null(),
    }
}

fn get_namespace_uri(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let namespace = attr(this, ec)?.namespace_uri();
    Ok(nullable_string_value(namespace, ec))
}

fn get_prefix(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let prefix = attr(this, ec)?.prefix();
    Ok(nullable_string_value(prefix, ec))
}

fn get_local_name(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let local_name = attr(this, ec)?.local_name();
    Ok(nullable_string_value(Some(local_name), ec))
}

fn get_name(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let name = attr(this, ec)?.name();
    Ok(nullable_string_value(Some(name), ec))
}

fn get_value(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let value = attr(this, ec)?.value(ec);
    Ok(nullable_string_value(Some(value), ec))
}

fn set_value(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let value = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    attr(this, ec)?.set_value(&value, ec);
    Ok(ec.value_undefined())
}

fn get_owner_element(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let Some(element) = attr(this, ec)?.owner_element(ec) else {
        return Ok(ec.value_null());
    };
    let object = object_for_existing_node(element.node.document.clone(), element.node.node_id, ec)?;
    Ok(Types::value_from_object(object))
}

fn get_specified(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let specified = attr(this, ec)?.specified();
    Ok(ec.value_from_bool(specified))
}
