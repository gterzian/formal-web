//! <https://webassembly.github.io/spec/js-api/>
//!
//! Bridge between the domain layer (`content/src/wasm/`) and JavaScript.
//! Domain functions return Rust types; this module wraps them in
//! `WebIdlInterface` impls, creates JS objects, and handles promise
//! resolution/rejection.  Everything here returns `JsValue` or `JsObject`.

use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::PropertyDescriptor,
};
use wasmtime::{Func, Instance as WasmtimeInstance, Module, Store};

use crate::wasm::{instance_export_list, js_val_to_wasm_val, wasm_val_to_js_value, WasmInstance, WasmModule};
use crate::webidl::get_a_copy_of_the_buffer_source;
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlInterface,
};

// WebIdlInterface: Module

/// <https://webassembly.github.io/spec/js-api/#modules>
impl WebIdlInterface for WasmModule {
    const NAME: &'static str = "Module";

    fn legacy_namespace() -> Option<&'static str> {
        Some("WebAssembly")
    }

    fn constructor_length() -> usize {
        1
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_operation(OperationDef {
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
        context: &mut Context,
    ) -> JsResult<Self> {
        let bytes_value = args.first().ok_or_else(|| {
            JsNativeError::typ().with_message("Module constructor: missing argument")
        })?;
        let stable_bytes = get_a_copy_of_the_buffer_source(bytes_value, context)?;
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
            JsNativeError::typ()
                .with_message(format!("CompileError: {}", error))
        })?;
        // Note: Steps 4-6 and 9-10 (builtins, imported string constants) are not yet implemented.
        Ok(WasmModule::new(module, stable_bytes))
    }
}

// WebIdlInterface: Instance

/// <https://webassembly.github.io/spec/js-api/#instances>
impl WebIdlInterface for WasmInstance {
    const NAME: &'static str = "Instance";

    fn legacy_namespace() -> Option<&'static str> {
        Some("WebAssembly")
    }

    fn define_members(def: &mut InterfaceDefinition) {
        def.add_attribute(AttributeDef {
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
    context: &mut Context,
) -> JsResult<JsValue> {
    let module_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: missing argument")
    })?;
    let module_object = module_value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: argument must be a Module object")
    })?;
    let wasm_module = module_object.downcast_ref::<WasmModule>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Module.exports: argument is not a WebAssembly.Module")
    })?;

    let descriptors = wasm_module.export_descriptors();
    let exports_array = boa_engine::object::builtins::JsArray::new(context)?;
    for (name, kind) in &descriptors {
        let entry = context
            .intrinsics()
            .constructors()
            .object()
            .constructor()
            .call(&JsValue::undefined(), &[], context)?;
        let entry_obj = entry.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create export descriptor")
        })?;
        entry_obj.set(js_string!("name"), js_string!(name.as_str()), false, context)?;
        entry_obj.set(js_string!("kind"), js_string!(*kind), false, context)?;
        exports_array.push(entry, context)?;
    }
    Ok(JsValue::from(exports_array))
}

fn get_instance_exports_binding(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Instance.exports getter: receiver is not an object")
    })?;
    let instance = object.downcast_ref::<WasmInstance>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Instance.exports getter: receiver is not a WebAssembly.Instance")
    })?;
    Ok(JsValue::from(instance.exports.clone()))
}

// Prototype-lookup helpers

pub(crate) fn get_wasm_module_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Module"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj
        .get(js_string!("prototype"), context)
        .ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

pub(crate) fn get_wasm_compile_error_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("CompileError"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj
        .get(js_string!("prototype"), context)
        .ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

pub(crate) fn get_wasm_instance_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Instance"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj
        .get(js_string!("prototype"), context)
        .ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

// Promise resolution

pub(crate) fn resolve_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<()> {
    let module_proto = get_wasm_module_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let module_object = JsObject::from_proto_and_data(
        Some(module_proto),
        WasmModule::new(module, bytes),
    );
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)?;
    Ok(())
}

pub(crate) fn reject_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    message: String,
    context: &mut Context,
) -> JsResult<()> {
    let ce_proto = get_wasm_compile_error_prototype(context);
    let error = if let Some(ref proto) = ce_proto {
        let error_obj = JsObject::from_proto_and_data(Some(proto.clone()), ());
        error_obj
            .set(js_string!("message"), js_string!(message.as_str()), false, context)
            .ok();
        let ns = context.global_object().get(js_string!("WebAssembly"), context).ok();
        if let Some(ns_val) = ns {
            if let Some(ns_obj) = ns_val.as_object() {
                if let Ok(ce_ctor) = ns_obj.get(js_string!("CompileError"), context) {
                    error_obj
                        .set(js_string!("constructor"), ce_ctor.clone(), false, context)
                        .ok();
                }
            }
        }
        error_obj
            .set(js_string!("name"), js_string!("CompileError"), false, context)
            .ok();
        JsValue::from(error_obj)
    } else {
        JsValue::from(js_string!(message.as_str()))
    };
    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)?;
    Ok(())
}

pub(crate) fn resolve_instantiate_promise(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    context: &mut Context,
) -> JsResult<()> {
    let mut store_guard = store.lock().unwrap();
    let exports = create_exports_object(module, instance, &mut *store_guard, store, context)?;
    drop(store_guard);
    let instance_proto = get_wasm_instance_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let instance_object = JsObject::from_proto_and_data(
        Some(instance_proto),
        WasmInstance::new(exports, Arc::clone(store), *instance),
    );
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)?;
    Ok(())
}

// Exports object creation

pub(crate) fn create_exports_object(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &mut Store<()>,
    store_arc: &Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> JsResult<JsObject> {
    let exports_object = JsObject::from_proto_and_data(None, ());
    let export_list = instance_export_list(module, instance, store);
    // Note: Only exported functions are implemented.  For memory, table,
    // global, and tag exports the current implementation returns undefined.
    for (name, extern_val) in &export_list {
        let value = match extern_val {
            wasmtime::Extern::Func(func) => {
                create_exported_function_wrapper(*func, Arc::clone(store_arc), context)?
            }
            _ => JsValue::undefined(),
        };
        exports_object
            .set(js_string!(name.as_str()), value, false, context)
            .map_err(|_| JsNativeError::typ().with_message("failed to set export property"))?;
    }
    // Note: Boa does not expose SetIntegrityLevel directly; skip for now.
    Ok(exports_object)
}

fn create_exported_function_wrapper(
    func: Func,
    store: Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let js_func = unsafe {
        NativeFunction::from_closure(
            move |_this: &JsValue, args: &[JsValue], context: &mut Context| -> JsResult<JsValue> {
                let mut store_guard = store.lock().unwrap();
                let func_type = func.ty(&*store_guard);
                let params: Vec<wasmtime::Val> = func_type
                    .params()
                    .enumerate()
                    .map(|(i, param_type)| {
                        let js_arg = args.get(i).cloned().unwrap_or(JsValue::undefined());
                        js_val_to_wasm_val(&js_arg, &param_type, context)
                    })
                    .collect::<Result<_, _>>()?;
                let mut results = vec![wasmtime::Val::I32(0); func_type.results().len()];
                func.call(&mut *store_guard, &params, &mut results).map_err(|error| {
                    JsNativeError::error()
                        .with_message(format!("wasm trap: {}", error))
                })?;
                if results.len() == 1 {
                    wasm_val_to_js_value(&results[0], context)
                } else {
                    Err(JsNativeError::error()
                        .with_message("multiple wasm results not yet supported")
                        .into())
                }
            },
        )
    };
    let realm = context.realm().clone();
    let func_object = FunctionObjectBuilder::new(&realm, js_func).build();
    Ok(JsValue::from(func_object))
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
        let ctor_fn = NativeFunction::from_fn_ptr(move |_new_target, args, ctx| {
            let message = args
                .first()
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();
            let error = ctx
                .intrinsics()
                .constructors()
                .error()
                .constructor()
                .call(&JsValue::undefined(), &[JsValue::from(js_string!(message.as_str()))], ctx)?;
            Ok(error)
        });
        let realm = context.realm().clone();
        let ctor = FunctionObjectBuilder::new(&realm, ctor_fn)
            .name(*name)
            .length(1)
            .constructor(true)
            .build();
        let ctor_obj: JsObject = ctor.into();
        let proto = JsObject::from_proto_and_data(
            Some(context.intrinsics().constructors().error().prototype()),
            (),
        );
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
) -> JsResult<JsValue> {
    Err(JsNativeError::error()
        .with_message("WebAssembly.JSTag: not yet implemented")
        .into())
}
