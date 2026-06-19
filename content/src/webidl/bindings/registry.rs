use std::any::TypeId;
use std::collections::HashMap;

use boa_engine::{Context, JsData, JsObject};
use boa_gc::{Finalize, Trace};

use super::interface::WebIdlInterface;

/// An entry in the interface registry.
pub(crate) struct InterfaceEntry {
    pub(crate) prototype: JsObject,
    pub(crate) constructor: JsObject,
}

/// Registry of Web IDL interfaces stored in the context's HostDefined data.
#[derive(Trace, Finalize)]
pub(crate) struct InterfaceRegistry {
    #[unsafe_ignore_trace]
    map: HashMap<TypeId, InterfaceEntry>,
}

impl JsData for InterfaceRegistry {}

impl InterfaceRegistry {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub(crate) fn register<T: WebIdlInterface + 'static>(
        &mut self,
        prototype: JsObject,
        constructor: JsObject,
    ) {
        self.map.insert(
            TypeId::of::<T>(),
            InterfaceEntry {
                prototype,
                constructor,
            },
        );
    }

    pub(crate) fn get_prototype<T: 'static>(&self) -> Option<&JsObject> {
        self.map.get(&TypeId::of::<T>()).map(|e| &e.prototype)
    }

    pub(crate) fn get_constructor<T: 'static>(&self) -> Option<&JsObject> {
        self.map.get(&TypeId::of::<T>()).map(|e| &e.constructor)
    }
}

// ── Context-based helpers (used by all current callers) ──

/// Get a constructor from the HostDefined registry.
pub(crate) fn get_constructor_from_host_defined<T: 'static>(context: &Context) -> Option<JsObject> {
    context
        .get_data::<InterfaceRegistry>()
        .and_then(|r| r.get_constructor::<T>())
        .cloned()
}

/// Ensure the interface registry exists on the context.
pub(crate) fn initialize(context: &mut Context) {
    if context.get_data::<InterfaceRegistry>().is_none() {
        context.insert_data(InterfaceRegistry::new());
    }
}

/// Register an interface in the HostDefined registry.
pub(crate) fn register_in_host_defined<T: WebIdlInterface + 'static>(
    context: &mut Context,
    prototype: JsObject,
    constructor: JsObject,
) {
    if let Some(mut registry) = context.remove_data::<InterfaceRegistry>() {
        registry.register::<T>(prototype, constructor);
        context.insert_data(*registry);
    }
}

/// Get a prototype from the HostDefined registry.
pub(crate) fn get_prototype_from_host_defined<T: 'static>(context: &Context) -> Option<JsObject> {
    context
        .get_data::<InterfaceRegistry>()
        .and_then(|r| r.get_prototype::<T>())
        .cloned()
}

/// Wire the prototype chain for an interface that inherits from another.
pub(crate) fn wire_prototype<TChild: 'static, TParent: 'static>(context: &mut Context) {
    if let Some(registry) = context.remove_data::<InterfaceRegistry>() {
        let child_proto = registry.get_prototype::<TChild>().cloned();
        let parent_proto = registry.get_prototype::<TParent>().cloned();
        if let (Some(child), Some(parent)) = (child_proto, parent_proto) {
            child.set_prototype(Some(parent));
        }
        context.insert_data(*registry);
    }
}
