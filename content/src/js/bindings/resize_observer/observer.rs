use crate::dom::Document;
use crate::js::Types;
use crate::js::bindings::dom::try_with_element_ref;
use crate::js::bindings::{dictionary, string_member, this_as};
use crate::js::platform_objects::document_object;
use crate::resize_observer::{ResizeObserver, ResizeObserverBoxOptions};
use crate::webidl::bindings::{BindingFn, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::callback_function_value;
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for ResizeObserver {
    const NAME: &'static str = "ResizeObserver";

    fn constructor_length() -> usize {
        1
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let undefined = ec.value_undefined();
        let callback = callback_function_value(args.first().unwrap_or(&undefined), ec)?;
        let document_object = document_object(ec)?;
        let document = ec
            .with_object_any(&document_object)
            .and_then(|data| data.downcast_ref::<Document>().cloned())
            .ok_or_else(|| ec.new_type_error("the global's document is not a Document"))?;
        Ok(ResizeObserver::constructor(callback, &document, ec))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, length, method) in [
            ("observe", 1, observe as BindingFn<Types>),
            ("unobserve", 1, unobserve),
            ("disconnect", 0, disconnect),
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

fn observer(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<ResizeObserver, Types> {
    this_as::<ResizeObserver>(this, "ResizeObserver", ec)
}

fn observe(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let target = try_with_element_ref(args.first().unwrap_or(&undefined), ec, |element| {
        element.clone()
    })?;
    let options = dictionary(args.get(1), ec)?;
    let observed_box = match string_member(&options, "box", ec)? {
        Some(value) => ResizeObserverBoxOptions::from_idl(&value).ok_or_else(|| {
            ec.new_type_error(&format!(
                "'{value}' is not a valid value for enumeration ResizeObserverBoxOptions"
            ))
        })?,
        None => ResizeObserverBoxOptions::ContentBox,
    };
    observer(this, ec)?.observe(target, observed_box, ec);
    Ok(ec.value_undefined())
}

fn unobserve(
    this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let undefined = ec.value_undefined();
    let target = try_with_element_ref(args.first().unwrap_or(&undefined), ec, |element| {
        element.clone()
    })?;
    observer(this, ec)?.unobserve(&target, ec);
    Ok(ec.value_undefined())
}

fn disconnect(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    observer(this, ec)?.disconnect(ec);
    Ok(ec.value_undefined())
}
