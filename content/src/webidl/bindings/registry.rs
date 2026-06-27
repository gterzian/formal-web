use std::any::TypeId;
use std::collections::HashMap;

use js_engine::{ExecutionContext, JsTypes, JsTypesWithRealm};

use super::interface::WebIdlInterface;

/// An entry in the interface registry.
pub(crate) struct InterfaceEntry<T: JsTypes> {
    pub(crate) prototype: T::JsObject,
    pub(crate) constructor: T::JsObject,
}

/// Registry of Web IDL interfaces.
///
/// Generic over `T: JsTypes` so it can store engine-native object types.
/// Stored in the EC's host-defined data store via `store_host_any`/
/// `get_host_any`.
pub(crate) struct InterfaceRegistry<T: JsTypes> {
    map: HashMap<TypeId, InterfaceEntry<T>>,
}

impl<T: JsTypes> InterfaceRegistry<T> {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub(crate) fn register<U: 'static>(
        &mut self,
        prototype: T::JsObject,
        constructor: T::JsObject,
    ) {
        self.map.insert(
            TypeId::of::<U>(),
            InterfaceEntry {
                prototype,
                constructor,
            },
        );
    }

    pub(crate) fn get_prototype<U: 'static>(&self) -> Option<&T::JsObject> {
        self.map.get(&TypeId::of::<U>()).map(|e| &e.prototype)
    }

    pub(crate) fn get_constructor<U: 'static>(&self) -> Option<&T::JsObject> {
        self.map.get(&TypeId::of::<U>()).map(|e| &e.constructor)
    }
}

fn registry_type_id<Ty: 'static + JsTypes>() -> TypeId {
    TypeId::of::<InterfaceRegistry<Ty>>()
}

fn with_registry_mut<Ty: JsTypes + JsTypesWithRealm, R>(
    ec: &mut dyn ExecutionContext<Ty>,
    f: impl FnOnce(&mut InterfaceRegistry<Ty>) -> R,
) -> R {
    let type_id = registry_type_id::<Ty>();
    let mut registry = ec
        .remove_host_any(&type_id)
        .map(|any| {
            *any.downcast::<InterfaceRegistry<Ty>>()
                .expect("InterfaceRegistry type mismatch")
        })
        .unwrap_or_else(|| InterfaceRegistry::<Ty>::new());
    let result = f(&mut registry);
    ec.store_host_any(type_id, Box::new(registry));
    result
}

fn with_registry_ref<Ty: JsTypes + JsTypesWithRealm, R>(
    ec: &dyn ExecutionContext<Ty>,
    f: impl FnOnce(&InterfaceRegistry<Ty>) -> R,
) -> R {
    let type_id = registry_type_id::<Ty>();
    let host = ec
        .get_host_any(&type_id)
        .unwrap_or_else(|| panic!("InterfaceRegistry not initialized"));
    let registry = host
        .downcast_ref::<InterfaceRegistry<Ty>>()
        .expect("InterfaceRegistry type mismatch");
    f(registry)
}

/// Ensure the interface registry exists on the context.
pub(crate) fn initialize<Ty: JsTypes + JsTypesWithRealm>(ec: &mut dyn ExecutionContext<Ty>) {
    with_registry_mut::<Ty, _>(ec, |_| {});
}

/// Register an interface in the registry.
pub(crate) fn register_in_host_defined<Ty, I>(
    ec: &mut dyn ExecutionContext<Ty>,
    prototype: Ty::JsObject,
    constructor: Ty::JsObject,
) where
    Ty: JsTypes + JsTypesWithRealm,
    I: WebIdlInterface<Ty> + 'static,
{
    with_registry_mut::<Ty, _>(ec, |registry| {
        registry.register::<I>(prototype, constructor);
    });
}

/// Get a prototype from the registry.
pub(crate) fn get_prototype_from_host_defined<Ty, I>(
    ec: &dyn ExecutionContext<Ty>,
) -> Option<Ty::JsObject>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: 'static,
{
    with_registry_ref::<Ty, _>(ec, |registry| registry.get_prototype::<I>().cloned())
}

/// Wire the prototype chain for an interface that inherits from another.
pub(crate) fn wire_prototype<Ty, TChild, TParent>(ec: &mut dyn ExecutionContext<Ty>)
where
    Ty: JsTypes + JsTypesWithRealm,
    TChild: 'static,
    TParent: 'static,
{
    let (child_proto, parent_proto) = {
        let reg = with_registry_ref::<Ty, _>(ec, |registry| {
            (
                registry.get_prototype::<TChild>().cloned(),
                registry.get_prototype::<TParent>().cloned(),
            )
        });
        reg
    };
    if let (Some(child), Some(parent)) = (child_proto, parent_proto) {
        let _ = ec.set_prototype(child, Some(parent));
    }
}

/// Get a prototype from the registry (generic, takes ExecutionContext).
pub(crate) fn get_registry_prototype<Ty, I>(ec: &dyn ExecutionContext<Ty>) -> Option<Ty::JsObject>
where
    Ty: JsTypes + JsTypesWithRealm,
    I: 'static,
{
    get_prototype_from_host_defined::<Ty, I>(ec)
}

/// Convenience: get prototype from &Context (Boa-specific, uses repr(transparent) cast).
pub(crate) fn get_registry_prototype_boa<I: 'static>(
    context: &boa_engine::Context,
) -> Option<boa_engine::JsObject> {
    get_prototype_from_host_defined::<js_engine::boa::BoaTypes, I>(crate::js::context_as_ec_ref(
        context,
    ))
}

/// Convenience: wire prototype using &mut Context.
pub(crate) fn wire_registry_prototype_boa<TChild: 'static, TParent: 'static>(
    context: &mut boa_engine::Context,
) {
    wire_prototype::<js_engine::boa::BoaTypes, TChild, TParent>(crate::js::context_as_ec(context));
}
