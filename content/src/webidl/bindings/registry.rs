use std::any::TypeId;
use std::collections::HashMap;

use boa_engine::Context;
use js_engine::JsTypes;

use super::interface::WebIdlInterface;

/// An entry in the interface registry.
pub(crate) struct InterfaceEntry<T: JsTypes> {
    pub(crate) prototype: T::JsObject,
    pub(crate) constructor: T::JsObject,
}

/// Registry of Web IDL interfaces.
///
/// Generic over `T: JsTypes` so it can store engine-native object types.
/// Stored in the context's host-defined data via a `RegistryHost` wrapper.
pub(crate) struct InterfaceRegistry<T: JsTypes> {
    map: HashMap<TypeId, InterfaceEntry<T>>,
}

impl<T: JsTypes> InterfaceRegistry<T> {
    pub(crate) fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub(crate) fn register<U: 'static>(&mut self, prototype: T::JsObject, constructor: T::JsObject) {
        self.map.insert(
            TypeId::of::<U>(),
            InterfaceEntry { prototype, constructor },
        );
    }

    pub(crate) fn get_prototype<U: 'static>(&self) -> Option<&T::JsObject> {
        self.map.get(&TypeId::of::<U>()).map(|e| &e.prototype)
    }

    pub(crate) fn get_constructor<U: 'static>(&self) -> Option<&T::JsObject> {
        self.map.get(&TypeId::of::<U>()).map(|e| &e.constructor)
    }
}

/// Wrapper for storing an InterfaceRegistry in the Boa Context's HostDefined.
struct RegistryHost(Box<dyn std::any::Any>);

fn registry_type_id() -> TypeId {
    TypeId::of::<InterfaceRegistry<js_engine::boa::BoaTypes>>()
}

fn with_registry_mut<R>(context: &mut Context, f: impl FnOnce(&mut InterfaceRegistry<js_engine::boa::BoaTypes>) -> R) -> R {
    let mut host = context.remove_data::<RegistryHost>()
        .unwrap_or_else(|| Box::new(RegistryHost(Box::new(InterfaceRegistry::<js_engine::boa::BoaTypes>::new()))));
    let registry: &mut InterfaceRegistry<js_engine::boa::BoaTypes> = unsafe {
        &mut *(host.0.as_mut() as *mut dyn std::any::Any as *mut InterfaceRegistry<js_engine::boa::BoaTypes>)
    };
    let result = f(registry);
    context.insert_data(*host);
    result
}

fn get_registry_ref(context: &Context) -> Option<&InterfaceRegistry<js_engine::boa::BoaTypes>> {
    let host = context.get_data::<RegistryHost>()?;
    host.0.downcast_ref::<InterfaceRegistry<js_engine::boa::BoaTypes>>()
}

/// Ensure the interface registry exists on the context.
pub(crate) fn initialize(context: &mut Context) {
    with_registry_mut(context, |_| {});
}

/// Register an interface in the registry.
pub(crate) fn register_in_host_defined<T: WebIdlInterface<js_engine::boa::BoaTypes> + 'static>(
    context: &mut Context,
    prototype: boa_engine::JsObject,
    constructor: boa_engine::JsObject,
) {
    with_registry_mut(context, |registry| {
        registry.register::<T>(prototype, constructor);
    });
}

/// Get a prototype from the registry.
pub(crate) fn get_prototype_from_host_defined<T: 'static>(context: &Context) -> Option<boa_engine::JsObject> {
    get_registry_ref(context)?.get_prototype::<T>().cloned()
}

/// Wire the prototype chain for an interface that inherits from another.
pub(crate) fn wire_prototype<TChild: 'static, TParent: 'static>(context: &mut Context) {
    let child_proto = get_registry_ref(context)
        .and_then(|r| r.get_prototype::<TChild>().cloned());
    let parent_proto = get_registry_ref(context)
        .and_then(|r| r.get_prototype::<TParent>().cloned());
    if let (Some(child), Some(parent)) = (child_proto, parent_proto) {
        child.set_prototype(Some(parent));
    }
}

/// Get a prototype from the registry (aliased for external use).
pub(crate) fn get_registry_prototype<T: 'static>(context: &Context) -> Option<boa_engine::JsObject> {
    get_prototype_from_host_defined::<T>(context)
}
