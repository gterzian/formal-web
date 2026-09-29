use crate::geometry::DOMRectReadOnly;
use crate::js::Types;
use crate::js::bindings::this_as;
use crate::webidl::bindings::{
    AttributeDef, BindingFn, InterfaceDefinition, OperationDef, WebIdlInterface,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

type JsValue = <Types as JsTypes>::JsValue;

impl WebIdlInterface<Types> for DOMRectReadOnly {
    const NAME: &'static str = "DOMRectReadOnly";

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [
            ("x", get_x as BindingFn<Types>),
            ("y", get_y),
            ("width", get_width),
            ("height", get_height),
            ("top", get_top),
            ("right", get_right),
            ("bottom", get_bottom),
            ("left", get_left),
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
        def.add_operation(OperationDef {
            id: "toJSON",
            length: 0,
            method: to_json,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });
    }
}

fn rect(
    this: &JsValue,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<DOMRectReadOnly, Types> {
    this_as::<DOMRectReadOnly>(this, "DOMRectReadOnly", ec)
}

macro_rules! number_getter {
    ($name:ident, $method:ident) => {
        fn $name(
            this: &JsValue,
            _args: &[JsValue],
            ec: &mut dyn ExecutionContext<Types>,
        ) -> Completion<JsValue, Types> {
            let value = rect(this, ec)?.$method();
            Ok(ec.value_from_number(value))
        }
    };
}

number_getter!(get_x, x);
number_getter!(get_y, y);
number_getter!(get_width, width);
number_getter!(get_height, height);
number_getter!(get_top, top);
number_getter!(get_right, right);
number_getter!(get_bottom, bottom);
number_getter!(get_left, left);

fn to_json(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let rect = rect(this, ec)?;
    let object = ec.create_plain_object(None);
    for (name, value) in [
        ("x", rect.x()),
        ("y", rect.y()),
        ("width", rect.width()),
        ("height", rect.height()),
        ("top", rect.top()),
        ("right", rect.right()),
        ("bottom", rect.bottom()),
        ("left", rect.left()),
    ] {
        let key = ec.property_key_from_str(name);
        let value = ec.value_from_number(value);
        ec.create_data_property(object.clone(), key, value)?;
    }
    Ok(Types::value_from_object(object))
}
