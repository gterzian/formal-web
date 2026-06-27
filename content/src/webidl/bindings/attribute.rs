use std::marker::PhantomData;

use boa_engine::{
    js_string, native_function::NativeFunction, property::PropertyDescriptor, Context, JsResult,
    JsValue,
};

use js_engine::JsTypes;

/// Describes a single attribute on an interface.
///
/// https://webidl.spec.whatwg.org/#dfn-attribute
pub(crate) struct AttributeDef<T: JsTypes> {
    pub id: &'static str,
    pub getter: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
    pub setter: Option<fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>>,
    pub static_: bool,
    pub unforgeable: bool,
    pub promise_type: bool,
    pub legacy_lenient_this: bool,
    pub replaceable: bool,
    pub put_forwards: Option<&'static str>,
    pub legacy_lenient_setter: bool,
    pub _phantom: PhantomData<T>,
}

/// <https://webidl.spec.whatwg.org/#define-the-regular-attributes>
pub(crate) fn define_regular_attributes(
    proto: &JsValue,
    context: &mut Context,
    attributes: &[AttributeDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let regular: Vec<&AttributeDef<js_engine::boa::BoaTypes>> = attributes
        .iter()
        .filter(|a| !a.static_ && !a.unforgeable)
        .collect();
    define_attributes_on_target(proto, context, &regular)
}

pub(crate) fn define_static_attributes(
    constructor: &JsValue,
    context: &mut Context,
    attributes: &[AttributeDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let static_attrs: Vec<&AttributeDef<js_engine::boa::BoaTypes>> =
        attributes.iter().filter(|a| a.static_).collect();
    define_attributes_on_target(constructor, context, &static_attrs)
}

fn define_attributes_on_target(
    target: &JsValue,
    context: &mut Context,
    attributes: &[&AttributeDef<js_engine::boa::BoaTypes>],
) -> JsResult<()> {
    let realm = context.realm().clone();
    for attr in attributes {
        let getter_fn = NativeFunction::from_fn_ptr(attr.getter).to_js_function(&realm);
        let setter_fn = attr
            .setter
            .map(|s| NativeFunction::from_fn_ptr(s).to_js_function(&realm));
        let configurable = !attr.unforgeable;
        let mut desc = PropertyDescriptor::builder()
            .get(getter_fn)
            .enumerable(true)
            .configurable(configurable);
        if let Some(setter) = setter_fn {
            desc = desc.set(setter);
        }
        let target_obj = target.as_object().ok_or_else(|| {
            boa_engine::JsNativeError::typ()
                .with_message("target is not an object in attribute definition")
        })?;
        target_obj.define_property_or_throw(js_string!(attr.id), desc.build(), context)?;
    }
    Ok(())
}
