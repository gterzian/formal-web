
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
    InterfaceDefinition, WebIdlInterface, WebIdlNamespace, create_interface_instance,
    register_interface_spec, register_namespace_spec,
};
pub(crate) use operation::OperationDef;
pub(crate) use registry::{
    get_registry_prototype, initialize as initialize_registry,
    wire_prototype as wire_registry_prototype,
};

#[cfg(boa_backend)]
pub(crate) use registry::wire_constructor_prototype as wire_registry_constructor_prototype;
