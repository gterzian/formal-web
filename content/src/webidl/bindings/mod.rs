// Several members and helpers are defined here for completeness but not yet
// wired to call sites.  Acceptable as spec infrastructure scaffolding.
#![allow(dead_code)]

mod attribute;
mod constant;
mod interface;
mod operation;
pub(crate) mod registry;

pub(crate) use attribute::AttributeDef;
pub(crate) use constant::ConstantDef;
pub(crate) use interface::{
    InterfaceDefinition, PostCreateReflector, WebIdlInterface, WebIdlNamespace,
    create_interface_instance, register_interface_spec, register_namespace_spec,
};
pub(crate) use operation::OperationDef;
pub(crate) use registry::{
    get_legacy_platform_object_handler_from_host_defined, get_registry_prototype,
    initialize as initialize_registry, set_legacy_platform_object_handler_for_interface,
    wire_constructor_prototype as wire_registry_constructor_prototype,
    wire_prototype as wire_registry_prototype,
};

use js_engine::{Completion, ExecutionContext, JsTypes};

/// A binding function: receives the `this` value, the arguments and the
/// execution context.  Binding functions use `T::value_as_object` and
/// `ec.with_platform_data` for upcast/downcast, avoiding engine-specific
/// dependencies.
pub(crate) type BindingFn<T> = fn(
    &<T as JsTypes>::JsValue,
    &[<T as JsTypes>::JsValue],
    &mut dyn ExecutionContext<T>,
) -> Completion<<T as JsTypes>::JsValue, T>;
