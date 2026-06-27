//! <https://webassembly.github.io/spec/js-api/>
//!
//! `WebIdlInterface` impls and JS-object creation for the
//! `WebAssembly.Module` and `WebAssembly.Instance` platform objects.
//! Each binding function is a thin wrapper that downcasts, calls a
//! domain method or function, and wraps the result.

use std::marker::PhantomData;
use boa_engine::{
    js_string, native_function::NativeFunction, object::FunctionObjectBuilder,
    property::PropertyDescriptor, Context, JsNativeError, JsObject, JsResult, JsValue,
};

use crate::wasm::{WasmInstance, WasmModule};
use crate::webidl::bindings::{AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface};
use crate::webidl::get_a_copy_of_the_buffer_source;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

// WebIdlInterface: Module

/// <https://webassembly.github.io/spec/js-api/#modules>
impl WebIdlInterface<js_engine::boa::BoaTypes> for WasmModule {
    const NAME: &'static str = "Module";

    fn legacy_namespace() -> Option<&'static str> {
        Some("WebAssembly")
    }

    fn constructor_length() -> usize {
        1
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "exports",
            length: 1,
            method: module_exports_binding,
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        // <https://webassembly.github.io/spec/js-api/#dom-module-imports>
        // Note: Not yet implemented.
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "imports",
            length: 1,
            method: |_this, _args, _ctx| {
                Err(JsNativeError::error()
                    .with_message("WebAssembly.Module.imports: not yet implemented")
                    .into())
            },
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
        // <https://webassembly.github.io/spec/js-api/#dom-module-customsections>
        // Note: Not yet implemented.
        def.add_operation(OperationDef {
            _phantom: PhantomData,
        
            id: "customSections",
            length: 2,
            method: |_this, _args, _ctx| {
                Err(JsNativeError::error()
                    .with_message("WebAssembly.Module.customSections: not yet implemented")
                    .into())
            },
            static_: true,
            unforgeable: false,
            promise_type: false,
        });
    }

    fn create_platform_object(
        _new_target: &JsValue,
        args: &[JsValue],
        ec: &mut dyn ExecutionContext<BoaTypes>,
    ) -> Completion<Self, BoaTypes> {
        let value_undefined = ec.value_undefined();
        let ctx = unsafe { crate::js::ec_to_ctx(ec) };
        (|| -> JsResult<Self> {
        let bytes_value = args.first().ok_or_else(|| {
            JsNativeError::typ().with_message("Module constructor: missing argument")
        })?;
        let stable_bytes = get_a_copy_of_the_buffer_source(bytes_value, ctx)?;
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
            JsNativeError::typ().with_message(format!("CompileError: {}", error))
        })?;
        // Note: Steps 4-6 and 9-10 (builtins, imported string constants) are not yet implemented.
        Ok(WasmModule::new(module, stable_bytes))
        })()
        .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
    }
}

// WebIdlInterface: Instance

/// <https://webassembly.github.io/spec/js-api/#instances>
impl WebIdlInterface<js_engine::boa::BoaTypes> for WasmInstance {
    const NAME: &'static str = "Instance";

    fn legacy_namespace() -> Option<&'static str> {
        Some("WebAssembly")
    }

    fn define_members(def: &mut InterfaceDefinition<js_engine::boa::BoaTypes>) {
        def.add_attribute(AttributeDef {
            _phantom: PhantomData,
        
            id: "exports",
            getter: get_instance_exports_binding,
            setter: None,
            static_: false,
            unforgeable: false,
            promise_type: false,
            legacy_lenient_this: false,
            replaceable: false,
            put_forwards: None,
            legacy_lenient_setter: false,
        });
    }
}

// Module.exports binding

fn module_exports_binding(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let module_value = args
        .first()
        .ok_or_else(|| JsNativeError::typ().with_message("Module.exports: missing argument"))?;
    let module_object = module_value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: argument must be a Module object")
    })?;
    let wasm_module = module_object.downcast_ref::<WasmModule>().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: argument is not a WebAssembly.Module")
    })?;

    let descriptors = wasm_module.export_descriptors();
    let exports_array = boa_engine::object::builtins::JsArray::new(ctx)?;
    for (name, kind) in &descriptors {
        let entry = ctx
            .intrinsics()
            .constructors()
            .object()
            .constructor()
            .call(&JsValue::undefined(), &[], ctx)?;
        let entry_obj = entry.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create export descriptor")
        })?;
        entry_obj.set(
            js_string!("name"),
            js_string!(name.as_str()),
            false,
            ctx,
        )?;
        entry_obj.set(js_string!("kind"), js_string!(*kind), false, ctx)?;
        exports_array.push(entry, ctx)?;
    }
    Ok(JsValue::from(exports_array))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

fn get_instance_exports_binding(
    this: &JsValue,
    _args: &[JsValue],
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    let value_undefined = ec.value_undefined();
    let ctx = unsafe { crate::js::ec_to_ctx(ec) };
    (|| -> JsResult<JsValue> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Instance.exports getter: receiver is not an object")
    })?;
    let instance = object.downcast_ref::<WasmInstance>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Instance.exports getter: receiver is not a WebAssembly.Instance")
    })?;
    Ok(JsValue::from(instance.exports.clone()))
    })()
    .map_err(|e| e.into_opaque(ctx).unwrap_or(value_undefined))
}

// Error types

pub(crate) fn register_wasm_error_types(
    namespace: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    // Note: This creates Error subclass constructors (CompileError, LinkError,
    // RuntimeError) and sets their `name` and `message` properties per the
    // spec. Each constructor delegates to the built-in Error constructor.
    let error_names = ["CompileError", "LinkError", "RuntimeError"];
    for name in &error_names {
        // Create the prototype object before the constructor function so the
        // closure can capture it.  This ensures instanceof checks work even
        // when the constructor is called without `new`.
        let proto = JsObject::from_proto_and_data(
            Some(context.intrinsics().constructors().error().prototype()),
            (),
        );
        let ctor_fn = {
            let name_str = *name;
            // SAFETY: The closure is 'static — no borrowed data captured.
            unsafe {
                NativeFunction::from_closure(
                    move |_new_target: &JsValue,
                          args: &[JsValue],
                          ctx: &mut Context|
                          -> JsResult<JsValue> {
                        let message = args
                            .first()
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_std_string_escaped())
                            .unwrap_or_default();
                        // Create an Error via the built-in Error constructor.
                        let error = ctx.intrinsics().constructors().error().constructor().call(
                            &JsValue::undefined(),
                            &[JsValue::from(js_string!(message.as_str()))],
                            ctx,
                        )?;
                        // Set name to the error type name (CompileError, etc.)
                        // so the error is recognized by name-based checks.
                        if let Some(obj) = error.as_object() {
                            let _ = obj.set(
                                js_string!("name"),
                                JsValue::from(js_string!(name_str)),
                                false,
                                ctx,
                            );
                        }
                        Ok(error)
                    },
                )
            }
        };
        let realm = context.realm().clone();
        let ctor = FunctionObjectBuilder::new(&realm, ctor_fn)
            .name(*name)
            .length(1)
            .constructor(true)
            .build();
        let ctor_obj: JsObject = ctor.into();
        let writable_config = PropertyDescriptor::builder()
            .writable(true)
            .configurable(true)
            .enumerable(false);
        proto.define_property_or_throw(
            js_string!("name"),
            writable_config.clone().value(js_string!(*name)).build(),
            context,
        )?;
        proto.define_property_or_throw(
            js_string!("message"),
            writable_config.value(js_string!("")).build(),
            context,
        )?;
        ctor_obj.define_property_or_throw(
            js_string!("prototype"),
            PropertyDescriptor::builder()
                .value(proto)
                .writable(false)
                .enumerable(false)
                .configurable(false)
                .build(),
            context,
        )?;
        let error_ctor = context.intrinsics().constructors().error().constructor();
        let error_ctor_obj: JsObject = error_ctor.into();
        ctor_obj.set_prototype(Some(error_ctor_obj));
        namespace.define_property_or_throw(
            js_string!(*name),
            PropertyDescriptor::builder()
                .value(ctor_obj)
                .writable(true)
                .configurable(true)
                .build(),
            context,
        )?;
    }
    Ok(())
}

// JSTag getter

pub(crate) fn get_wasm_jstag(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> Completion<JsValue, BoaTypes> {
    Err(JsNativeError::error()
        .with_message("WebAssembly.JSTag: not yet implemented")
        .into())
}
