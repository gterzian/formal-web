use crate::js::Types;
use crate::js::bindings::this_as;
use crate::js::platform_objects::object_for_existing_node;
use crate::resize_observer::{ResizeObserverEntry, ResizeObserverSize};
use crate::webidl::bindings::{AttributeDef, BindingFn, InterfaceDefinition, WebIdlInterface};
use js_engine::{Completion, ExecutionContext, IntegrityLevel, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for ResizeObserverEntry {
    const NAME: &'static str = "ResizeObserverEntry";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [
            ("target", get_target as BindingFn<Types>),
            ("contentRect", get_content_rect),
            ("borderBoxSize", get_border_box_size),
            ("contentBoxSize", get_content_box_size),
            (
                "devicePixelContentBoxSize",
                get_device_pixel_content_box_size,
            ),
        ] {
            def.add_attribute(AttributeDef {
                id,
                getter,
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
        }
    }
}

fn entry(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<ResizeObserverEntry, Types> {
    this_as::<ResizeObserverEntry>(this, "ResizeObserverEntry", ec)
}

fn frozen_array_of_sizes(
    sizes: &[ResizeObserverSize],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let array = ec.create_empty_array();
    for size in sizes {
        let value = size
            .reflector
            .clone()
            .map(Types::value_from_object)
            .ok_or_else(|| ec.new_type_error("ResizeObserverSize has no reflector"))?;
        ec.array_push(&array, value)?;
    }
    ec.set_integrity_level(array.clone(), IntegrityLevel::Frozen)?;
    Ok(Types::value_from_object(array))
}

fn get_target(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let target = entry(this, ec)?.target.clone();
    let object = object_for_existing_node(target.node.document.clone(), target.node.node_id, ec)?;
    Ok(Types::value_from_object(object))
}

fn get_content_rect(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    entry(this, ec)?
        .content_rect
        .reflector
        .clone()
        .map(Types::value_from_object)
        .ok_or_else(|| ec.new_type_error("DOMRectReadOnly has no reflector"))
}

fn get_border_box_size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sizes = entry(this, ec)?.border_box_size.clone();
    frozen_array_of_sizes(&sizes, ec)
}

fn get_content_box_size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sizes = entry(this, ec)?.content_box_size.clone();
    frozen_array_of_sizes(&sizes, ec)
}

fn get_device_pixel_content_box_size(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let sizes = entry(this, ec)?.device_pixel_content_box_size.clone();
    frozen_array_of_sizes(&sizes, ec)
}
