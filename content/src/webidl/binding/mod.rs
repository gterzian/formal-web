// Note: The entire binding module is `#[allow(dead_code)]` until the first
// platform object is migrated from the `boa/bindings/` pattern to
// `WebIdlInterface`.  Each struct, trait, and function here will gain real
// call sites as interfaces adopt the trait incrementally.
#![allow(dead_code)]

/// Web IDL JavaScript Binding Implementation
///
/// https://webidl.spec.whatwg.org/#ecmascript-binding
///
/// This module implements the algorithms from the Web IDL specification's
/// "JavaScript binding" section (§3).  It provides a trait-based abstraction
/// for Web IDL interfaces that:
///
/// - Follows the spec algorithms for creating interface objects, interface
///   prototype objects, and defining members (attributes, operations,
///   constants).
/// - Only calls into the ECMAScript engine (boa) where the spec itself says
///   to (CreateBuiltinFunction, DefinePropertyOrThrow, Get, Call, etc.).
/// - Keeps boilerplate getter/setter/method code in the platform object's
///   Rust module, while moving the spec-mandated registration logic here.
///
/// # Architecture
///
/// Each platform object (e.g. `Event`, `Node`, `Window`) implements the
/// `WebIdlInterface` trait, which provides:
///
/// - The interface name and parent interface name.
/// - An `define_members` method that populates an `InterfaceDefinition`
///   with the interface's attributes, operations, and constants.
/// - An optional `create_platform_object` for constructible interfaces.
///
/// The `register_interface` function (called from a `Class::init` impl)
/// then applies the spec algorithms to create the JavaScript bindings.
///
/// ```ignore
/// // In boa/bindings/dom/event.rs:
/// impl WebIdlInterface for Event {
///     const NAME: &'static str = "Event";
///     fn parent_name() -> Option<&'static str> { Some("EventTarget") }
///
///     fn define_members(def: &mut InterfaceDefinition) {
///         def.add_attribute(AttributeDef {
///             id: "type",
///             getter: get_type,
///             setter: None,
///             static_: false,
///             unforgeable: false,
///             promise_type: false,
///             legacy_lenient_this: false,
///             replaceable: false,
///             put_forwards: None,
///             legacy_lenient_setter: false,
///         });
///         def.add_operation(OperationDef {
///             id: "stopPropagation",
///             length: 0,
///             method: stop_propagation,
///             static_: false,
///             unforgeable: false,
///             promise_type: false,
///         });
///     }
/// }
///
/// impl Class for Event {
///     fn init(class: &mut ClassBuilder<'_>) -> JsResult<()> {
///         register_interface::<Event>(class)
///     }
/// }
/// ```

mod attribute;
mod constant;
mod interface;
mod operation;
pub(crate) mod registry;

pub(crate) use attribute::AttributeDef;
pub(crate) use constant::ConstantDef;
pub(crate) use interface::{
    create_interface_instance, InterfaceDefinition,
    register_interface_spec, WebIdlInterface,
};
pub(crate) use operation::OperationDef;
pub(crate) use registry::{
    get_constructor_from_host_defined as get_registry_constructor,
    get_prototype_from_host_defined as get_registry_prototype,
    initialize as initialize_registry, wire_prototype as wire_registry_prototype,
};
