use crate::dom::{Attr, DOMException, NamedNodeMap};
use crate::js::Types;
use crate::js::bindings::this_as;
use crate::webidl::LegacyPlatformObject;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface, create_interface_instance,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;
type OperationMethod =
    fn(&JsValue, &[JsValue], &mut dyn ExecutionContext<Types>) -> Completion<JsValue, Types>;

impl WebIdlInterface<Types> for NamedNodeMap {
    const NAME: &'static str = "NamedNodeMap";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        def.add_attribute(AttributeDef {
            id: "length",
            getter: get_length,
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
            ("item", 1, item as OperationMethod),
            ("getNamedItem", 1, get_named_item),
            ("getNamedItemNS", 2, get_named_item_ns),
            ("setNamedItem", 1, set_named_item),
            ("setNamedItemNS", 1, set_named_item),
            ("removeNamedItem", 1, remove_named_item),
            ("removeNamedItemNS", 2, remove_named_item_ns),
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
    }
}

impl LegacyPlatformObject for NamedNodeMap {
    const SUPPORTS_INDEXED_PROPERTIES: bool = true;
    const SUPPORTS_NAMED_PROPERTIES: bool = true;
    const LEGACY_UNENUMERABLE_NAMED_PROPERTIES: bool = true;

    fn supported_property_indices(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<u32, Types> {
        NamedNodeMap::supported_property_indices(self, ec)
    }

    fn determine_the_value_of_an_indexed_property(
        &self,
        index: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        let attr = NamedNodeMap::item(self, index, ec)?;
        Ok(attr_value(attr, ec))
    }

    fn supported_property_names(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<String>, Types> {
        NamedNodeMap::supported_property_names(self, ec)
    }

    fn determine_the_value_of_a_named_property(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        let attr = NamedNodeMap::get_named_item(self, name, ec)?;
        Ok(attr_value(attr, ec))
    }
}

fn named_node_map(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<NamedNodeMap, Types> {
    this_as::<NamedNodeMap>(this, "NamedNodeMap", ec)
}

fn attr_value(attr: Option<Attr>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    attr.and_then(|attr| attr.event_target.reflector.clone())
        .map(Types::value_from_object)
        .unwrap_or_else(|| ec.value_null())
}

fn dom_exception_value(error: DOMException, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    create_interface_instance::<Types, DOMException>(error, ec)
        .map(Types::value_from_object)
        .unwrap_or_else(|err| err)
}

fn nullable_string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<Option<String>, Types> {
    match args.get(index) {
        Some(value) if !Types::value_is_null(value) && !Types::value_is_undefined(value) => {
            Ok(Some(ec.to_rust_string(value.clone())?))
        }
        _ => Ok(None),
    }
}

fn get_length(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let length = named_node_map(this, ec)?.length(ec)?;
    Ok(ec.value_from_number(length as f64))
}

fn item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let index = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    let attr = named_node_map(this, ec)?.item(index, ec)?;
    Ok(attr_value(attr, ec))
}

fn get_named_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let qualified_name = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let attr = named_node_map(this, ec)?.get_named_item(&qualified_name, ec)?;
    Ok(attr_value(attr, ec))
}

fn get_named_item_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(undefined))?;
    let attr =
        named_node_map(this, ec)?.get_named_item_ns(namespace.as_deref(), &local_name, ec)?;
    Ok(attr_value(attr, ec))
}

fn set_named_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let attr = this_as::<Attr>(args.first().unwrap_or(&undefined), "Attr", ec)?;
    let old_attr = named_node_map(this, ec)?
        .set_named_item(&attr, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(old_attr, ec))
}

fn remove_named_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let qualified_name = ec.to_rust_string(args.first().cloned().unwrap_or(undefined))?;
    let attr = named_node_map(this, ec)?
        .remove_named_item(&qualified_name, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(Some(attr), ec))
}

fn remove_named_item_ns(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let namespace = nullable_string_argument(args, 0, ec)?;
    let local_name = ec.to_rust_string(args.get(1).cloned().unwrap_or(undefined))?;
    let attr = named_node_map(this, ec)?
        .remove_named_item_ns(namespace.as_deref(), &local_name, ec)?
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(attr_value(Some(attr), ec))
}
