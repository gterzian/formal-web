use crate::file_api::{File, FilePropertyBag};
use crate::js::Types;
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, WebIdlInterface};
use crate::webidl::long_long;
use js_engine::{Completion, ExecutionContext, JsTypes};

use super::super::{dictionary, string_value, this_as, usv_string_argument};
use super::blob::{convert_blob_parts, convert_blob_property_bag};

type JsValue = <Types as JsTypes>::JsValue;

fn file(this: &JsValue, ec: &mut dyn ExecutionContext<Types>) -> Completion<File, Types> {
    this_as::<File>(this, "File", ec)
}

impl WebIdlInterface<Types> for File {
    const NAME: &'static str = "File";

    fn parent_name() -> Option<&'static str> {
        Some("Blob")
    }

    fn constructor_length() -> usize {
        2
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        if args.len() < 2 {
            return Err(ec.new_type_error(&format!(
                "File constructor: 2 arguments required, but only {} present",
                args.len()
            )));
        }
        let file_bits = convert_blob_parts(&args[0], ec)?;
        let file_name = usv_string_argument(args, 1, ec)?;
        let blob = convert_blob_property_bag(args.get(2), ec)?;
        let dict = dictionary(
            args.get(2)
                .filter(|value| !Types::value_is_undefined(value)),
            ec,
        )?;
        let last_modified = match dict.get_member("lastModified", ec)? {
            Some(value) => Some(long_long(&value, ec)?),
            None => None,
        };
        Ok(File::constructor(
            file_bits,
            file_name,
            FilePropertyBag {
                blob,
                last_modified,
            },
        ))
    }

    fn define_members(def: &mut InterfaceDefinition<Types>) {
        for (id, getter) in [("name", name as _), ("lastModified", last_modified as _)] {
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

fn name(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let name = file(this, ec)?.name();
    Ok(string_value(&name, ec))
}

fn last_modified(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsValue, Types> {
    let last_modified = file(this, ec)?.last_modified();
    Ok(ec.value_from_number(last_modified as f64))
}
