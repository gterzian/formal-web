use crate::dom::DOMException;
use crate::html::Storage;
use crate::js::Types;
use crate::js::bindings::this_as;
use crate::webidl::LegacyPlatformObject;
use crate::webidl::bindings::{
    AttributeDef, BindingFn, InterfaceDefinition, OperationDef, WebIdlInterface,
    create_interface_instance,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for Storage {
    const NAME: &'static str = "Storage";

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
            ("key", 1, key as BindingFn<Types>),
            ("getItem", 1, get_item),
            ("setItem", 2, set_item),
            ("removeItem", 1, remove_item),
            ("clear", 0, clear),
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

impl LegacyPlatformObject for Storage {
    const SUPPORTS_INDEXED_PROPERTIES: bool = false;
    const SUPPORTS_NAMED_PROPERTIES: bool = true;
    const HAS_NAMED_PROPERTY_SETTER: bool = true;
    const HAS_NAMED_PROPERTY_DELETER: bool = true;

    fn supported_property_indices(
        &self,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<u32, Types> {
        Ok(0)
    }

    fn determine_the_value_of_an_indexed_property(
        &self,
        _index: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        Ok(ec.value_undefined())
    }

    fn supported_property_names(
        &self,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Vec<String>, Types> {
        Ok(Storage::supported_property_names(self))
    }

    fn determine_the_value_of_a_named_property(
        &self,
        name: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsValue, Types> {
        Ok(nullable_string_value(Storage::get_item(self, name), ec))
    }

    fn invoke_a_named_property_setter(
        &self,
        name: &str,
        value: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let value = ec.to_rust_string(value)?;
        Storage::set_item(self, name, &value).map_err(|error| dom_exception_value(error, ec))
    }

    fn invoke_a_named_property_deleter(
        &self,
        name: &str,
        _ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        Storage::remove_item(self, name);
        Ok(true)
    }
}

fn storage(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<Storage, Types> {
    this_as::<Storage>(this, "Storage", ec)
}

fn string_argument(
    args: &[JsValue],
    index: usize,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<String, Types> {
    let undefined = ec.value_undefined();
    ec.to_rust_string(args.get(index).cloned().unwrap_or(undefined))
}

fn nullable_string_value(value: Option<String>, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    match value {
        Some(value) => ec.value_from_string(ec.js_string_from_str(&value)),
        None => ec.value_null(),
    }
}

fn dom_exception_value(error: DOMException, ec: &mut dyn ExecutionContext<Types>) -> JsValue {
    create_interface_instance::<Types, DOMException>(error, ec)
        .map(Types::value_from_object)
        .unwrap_or_else(|err| err)
}

fn get_length(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let length = storage(this, ec)?.length();
    Ok(ec.value_from_number(f64::from(length)))
}

fn key(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let index = ec.to_uint32(args.first().cloned().unwrap_or(undefined))?;
    let key = storage(this, ec)?.key(index);
    Ok(nullable_string_value(key, ec))
}

fn get_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let key = string_argument(args, 0, ec)?;
    let value = storage(this, ec)?.get_item(&key);
    Ok(nullable_string_value(value, ec))
}

fn set_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let key = string_argument(args, 0, ec)?;
    let value = string_argument(args, 1, ec)?;
    storage(this, ec)?
        .set_item(&key, &value)
        .map_err(|error| dom_exception_value(error, ec))?;
    Ok(ec.value_undefined())
}

fn remove_item(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let key = string_argument(args, 0, ec)?;
    storage(this, ec)?.remove_item(&key);
    Ok(ec.value_undefined())
}

fn clear(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    storage(this, ec)?.clear();
    Ok(ec.value_undefined())
}
