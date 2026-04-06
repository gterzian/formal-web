use std::collections::HashMap;

use crate::parse::{InterfaceDef, MemberDef};

pub fn emit(interface: &InterfaceDef, descendants: &HashMap<String, Vec<String>>) -> String {
    let interface_snake = to_snake_case(&interface.name);
    let helper_name = format!("register_{interface_snake}_methods");
    let with_name = format!("with_{interface_snake}_mut");
    let descendants = descendants.get(&interface.name).cloned().unwrap_or_default();

    let mut output = String::new();
    output.push_str(&format!(
        "// GENERATED FROM: {} -- DO NOT EDIT\n",
        interface.source_file
    ));
    output.push_str("// Run `cargo run --manifest-path content/codegen/Cargo.toml` to regenerate.\n\n");
    output.push_str("use boa_engine::{\n");
    output.push_str("    Context, JsNativeError, JsResult, JsValue,\n");
    output.push_str("    class::ClassBuilder,\n");
    output.push_str("    js_string,\n");
    output.push_str("    native_function::NativeFunction,\n");
    output.push_str("    object::JsValue as _,\n");
    output.push_str("    property::Attribute,\n");
    output.push_str("};\n\n");
    output.push_str(&format!(
        "pub(super) fn {helper_name}(class: &mut ClassBuilder<'_>) -> JsResult<()> {{\n"
    ));
    output.push_str("    let realm = class.context().realm().clone();\n");
    for member in &interface.members {
        match member {
            MemberDef::Attribute { name, readonly } => {
                let getter = format!("get_{}", to_snake_case(name));
                let setter = if *readonly {
                    "None".to_owned()
                } else {
                    format!(
                        "Some(NativeFunction::from_fn_ptr({}).to_js_function(&realm))",
                        format!("set_{}", to_snake_case(name))
                    )
                };
                output.push_str(&format!(
                    "    class.accessor(js_string!(\"{}\"), Some(NativeFunction::from_fn_ptr({}).to_js_function(&realm)), {}, Attribute::all());\n",
                    name, getter, setter
                ));
            }
            MemberDef::Operation { name, argc } => {
                output.push_str(&format!(
                    "    class.method(js_string!(\"{}\"), {}, NativeFunction::from_fn_ptr({}));\n",
                    name,
                    argc,
                    to_snake_case(name)
                ));
            }
        }
    }
    output.push_str("    Ok(())\n}\n\n");
    output.push_str(&format!(
        "pub(super) fn {with_name}<R>(this: &JsValue, f: impl FnOnce(&mut {}) -> R) -> JsResult<R> {{\n",
        interface.name
    ));
    output.push_str("    let object = this.as_object().ok_or_else(|| JsNativeError::typ().with_message(\"receiver is not an object\"))?;\n");
    output.push_str(&format!(
        "    if let Some(mut value) = object.downcast_mut::<{}>() {{\n        return Ok(f(&mut value));\n    }}\n",
        interface.name
    ));
    for descendant in descendants {
        let field = to_snake_case(&interface.name);
        output.push_str(&format!(
            "    if let Some(mut value) = object.downcast_mut::<{}>() {{\n        return Ok(f(&mut value.{}));\n    }}\n",
            descendant, field
        ));
    }
    output.push_str(&format!(
        "    Err(JsNativeError::typ().with_message(\"receiver is not a {}\").into())\n}}\n",
        interface.name
    ));
    output
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}