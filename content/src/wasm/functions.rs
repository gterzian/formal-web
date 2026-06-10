//! <https://webassembly.github.io/spec/js-api/>

use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
    property::PropertyDescriptor,
};
use wasmtime::{Instance as WasmtimeInstance, Module, Store};

use crate::wasm::types::{WasmInstance, WasmModule};

// ── Namespace operation implementations ──

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-validate>
pub(crate) fn validate_wasm_module(stable_bytes: &[u8]) -> bool {
    // Step 2: "Compile stableBytes as a WebAssembly module and store the results as module."
    // Step 3: "If module is error, return false."
    // Note: Steps 4-6 (validating builtins and imported strings) are not yet implemented.
    let engine = wasmtime::Engine::default();
    matches!(Module::new(&engine, stable_bytes), Ok(_))
}

/// <https://webassembly.github.io/spec/js-api/#dom-module-module>
///
/// Note: [[BuiltinSets]] and [[ImportedStringModule]] are not yet implemented,
/// so steps 4-6 and 9-10 are skipped.
pub(crate) fn module_constructor_fn(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("Module constructor: missing argument")
    })?;
    let stable_bytes = crate::webidl::get_stable_bytes(bytes_value, context)?;

    // Step 2: "Compile the WebAssembly module stableBytes and store the result as module."
    // Step 3: "If module is error, throw a CompileError exception."
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
        JsNativeError::typ()
            .with_message(format!("CompileError: {}", error))
    })?;

    // Steps 7-8: "Set this.[[Module]] to module."
    //            "Set this.[[Bytes]] to stableBytes."
    // Note: Steps 4-6 (validating builtins and imported strings) and steps 9-10
    // ([[BuiltinSets]], [[ImportedStringModule]]) are not yet implemented.
    let module_object = JsObject::from_proto_and_data(
        get_wasm_module_prototype(context)
            .unwrap_or_else(|| context.intrinsics().constructors().object().prototype()),
        WasmModule::new(module, stable_bytes),
    );
    Ok(module_object.into())
}

/// <https://webassembly.github.io/spec/js-api/#dom-module-exports>
pub(crate) fn module_exports_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Validate argument is a Module object.
    let module_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: missing argument")
    })?;
    let module_object = module_value.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: argument must be a Module object")
    })?;

    let wasm_module = &module_object.downcast_ref::<WasmModule>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("Module.exports: argument is not a WebAssembly.Module")
    })?;

    // Step 1: "Let module be moduleObject.[[Module]]."
    // Step 2: "Let exports be « »."
    let exports_array = boa_engine::object::builtins::JsArray::new(context)?;

    // Step 3: "For each (name, type) of module_exports(module),"
    for export in wasm_module.module.exports() {
        // Step: "Let kind be the string value of the extern type type."
        let name = export.name();
        let kind_str = match export.ty() {
            wasmtime::ExternType::Func(_) => "function",
            wasmtime::ExternType::Table(_) => "table",
            wasmtime::ExternType::Memory(_) => "memory",
            wasmtime::ExternType::Global(_) => "global",
            wasmtime::ExternType::Tag(_) => "tag",
        };

        // Step: "Let obj be «[ "name" → name, "kind" → kind ]»."
        let entry = context
            .intrinsics()
            .constructors()
            .object()
            .constructor()
            .call(&JsValue::undefined(), &[], context)?;
        let entry_obj = entry.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create export descriptor")
        })?;
        entry_obj.set(js_string!("name"), js_string!(name), false, context)?;
        entry_obj.set(js_string!("kind"), js_string!(kind_str), false, context)?;

        // Step: "Append obj to exports."
        exports_array.push(entry, context)?;
    }

    // Step 4: "Return exports."
    Ok(JsValue::from(exports_array))
}

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-jstag>
///
/// Note: Not yet implemented.
pub(crate) fn get_wasm_jstag(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    Err(JsNativeError::error()
        .with_message("WebAssembly.JSTag: not yet implemented")
        .into())
}

// ── Asynchronous compilation ──

/// Get the `WebAssembly.Module.prototype` from the context's global object.
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

/// Get the `WebAssembly.CompileError.prototype` from the context's global object.
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

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn resolve_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<()> {
    // Step: "Construct a WebAssembly module object from module, bytes, ... and let
    //        moduleObject be the result."
    let module_proto = get_wasm_module_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());

    let module_object = JsObject::from_proto_and_data(
        Some(module_proto),
        WasmModule::new(module, bytes),
    );

    // Step: "Resolve promise with moduleObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn reject_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    message: String,
    context: &mut Context,
) -> JsResult<()> {
    // Step: "If module is error, reject promise with a CompileError exception and return."
    let ce_proto = get_wasm_compile_error_prototype(context);
    let error = if let Some(ref proto) = ce_proto {
        let error_obj = JsObject::from_proto_and_data(Some(proto.clone()), ());
        error_obj
            .set(
                js_string!("message"),
                js_string!(message.as_str()),
                false,
                context,
            )
            .ok();
        let ns = context.global_object().get(js_string!("WebAssembly"), context).ok();
        if let Some(ns_val) = ns {
            if let Some(ns_obj) = ns_val.as_object() {
                if let Ok(ce_ctor) = ns_obj.get(js_string!("CompileError"), context) {
                    error_obj
                        .set(
                            js_string!("constructor"),
                            ce_ctor.clone(),
                            false,
                            context,
                        )
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

// ── Error types ──

/// <https://webassembly.github.io/spec/js-api/#error-objects>
///
/// Note: This creates Error subclass constructors (CompileError, LinkError,
/// RuntimeError) and sets their `name` and `message` properties per the
/// spec. Each constructor delegates to the built-in Error constructor.
pub(crate) fn register_wasm_error_types(
    namespace: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let error_names = [
        "CompileError",
        "LinkError",
        "RuntimeError",
    ];

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
                .call(
                    &JsValue::undefined(),
                    &[JsValue::from(js_string!(message.as_str()))],
                    ctx,
                )?;
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

        let error_ctor = context
            .intrinsics()
            .constructors()
            .error()
            .constructor();
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

// ── Module type ──

/// <https://webassembly.github.io/spec/js-api/#modules>
pub(crate) fn register_wasm_module_type(
    namespace: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    // Prototype with static methods.
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        (),
    );

    // Module.exports(moduleObject)
    // <https://webassembly.github.io/spec/js-api/#dom-module-exports>
    let exports_fn = NativeFunction::from_fn_ptr(module_exports_fn);
    let realm = context.realm().clone();
    let exports_func = FunctionObjectBuilder::new(&realm, exports_fn)
        .name("exports")
        .length(1)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("exports"),
        PropertyDescriptor::builder()
            .value(exports_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // <https://webassembly.github.io/spec/js-api/#dom-module-imports>
    // Note: Not yet implemented.
    let imports_fn = NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
        Err(JsNativeError::error()
            .with_message("WebAssembly.Module.imports: not yet implemented")
            .into())
    });
    let imports_func = FunctionObjectBuilder::new(&realm, imports_fn)
        .name("imports")
        .length(1)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("imports"),
        PropertyDescriptor::builder()
            .value(imports_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // <https://webassembly.github.io/spec/js-api/#dom-module-customsections>
    // Note: Not yet implemented.
    let cs_fn = NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
        Err(JsNativeError::error()
            .with_message("WebAssembly.Module.customSections: not yet implemented")
            .into())
    });
    let cs_func = FunctionObjectBuilder::new(&realm, cs_fn)
        .name("customSections")
        .length(2)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("customSections"),
        PropertyDescriptor::builder()
            .value(cs_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // <https://webassembly.github.io/spec/js-api/#dom-module-module>
    let ctor_fn = NativeFunction::from_fn_ptr(module_constructor_fn);
    register_wasm_constructor(namespace, "Module", ctor_fn, 1, proto, context)
}

// ── Instantiation ──

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn resolve_instantiate_promise(
    module: &wasmtime::Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    context: &mut Context,
) -> JsResult<()> {
    // Step: "Let instanceObject be a new Instance."
    // Step: "Initialize instanceObject from module and instance."
    let mut store_guard = store.lock().unwrap();
    let exports = create_exports_object(module, instance, &mut *store_guard, store, context)?;
    drop(store_guard);

    let instance_proto = get_wasm_instance_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());

    let instance_object = JsObject::from_proto_and_data(
        Some(instance_proto),
        WasmInstance::new(exports, Arc::clone(store), *instance),
    );

    // Step: "Resolve promise with instanceObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#create-an-exports-object>
///
/// Note: Only exported functions are implemented.  For memory, table, global,
/// and tag exports the current implementation returns undefined.
pub(crate) fn create_exports_object(
    module: &wasmtime::Module,
    instance: &WasmtimeInstance,
    store: &mut Store<()>,
    store_arc: &Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let exportsObject be ! OrdinaryObjectCreate(null)."
    let exports_object = JsObject::from_proto_and_data(None, ());

    // Step 2: "For each (name, externtype) of module_exports(module),"
    for export in module.exports() {
        let name = export.name();
        let _extern_type = export.ty();

        // Step: "Let externval be instance_export(instance, name)."
        let extern_val = instance.get_export(&mut *store, name);

        let Some(extern_val) = extern_val else {
            continue;
        };

        let value = match extern_val {
            // Steps: "If externtype is of the form func functype,"
            //        "... Let func be the result of creating a new Exported Function from funcaddr."
            //        "Let value be func."
            wasmtime::Extern::Func(func) => {
                create_exported_function_wrapper(func, Arc::clone(store_arc), context)?
            }
            // Note: global, memory, table, tag exports are not yet implemented.
            _ => JsValue::undefined(),
        };

        // Step: "Let status be ! CreateDataProperty(exportsObject, name, value)."
        exports_object
            .set(js_string!(name), value.clone(), false, context)
            .map_err(|_| JsNativeError::typ().with_message("failed to set export property"))?;
    }

    // Step: "Perform ! SetIntegrityLevel(exportsObject, "frozen")."
    // Note: Boa does not expose SetIntegrityLevel directly; skip for now.

    // Step: "Return exportsObject."
    Ok(exports_object)
}

/// Create a JS-callable function wrapper for a wasm exported function.
fn create_exported_function_wrapper(
    func: wasmtime::Func,
    store: Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // SAFETY: The closure captures `Arc<Mutex<Store<()>>>` and
    // `wasmtime::Func`.  Neither type contains Boa GC pointers,
    // so `from_closure` is safe.
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

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
fn js_val_to_wasm_val(
    value: &JsValue,
    wasm_type: &wasmtime::ValType,
    context: &mut Context,
) -> Result<wasmtime::Val, JsNativeError> {
    match wasm_type {
        wasmtime::ValType::I32 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for i32"))?;
            Ok(wasmtime::Val::I32(n as i32))
        }
        wasmtime::ValType::I64 => {
            Err(JsNativeError::typ().with_message("i64 wasm values not yet supported"))
        }
        wasmtime::ValType::F32 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for f32"))?;
            Ok(wasmtime::Val::F32(n as u32))
        }
        wasmtime::ValType::F64 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for f64"))?;
            Ok(wasmtime::Val::F64(n.to_bits()))
        }
        _ => Err(JsNativeError::typ().with_message("unsupported wasm value type")),
    }
}

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
fn wasm_val_to_js_value(val: &wasmtime::Val, _context: &mut Context) -> JsResult<JsValue> {
    match val {
        wasmtime::Val::I32(n) => Ok(JsValue::from(*n)),
        wasmtime::Val::I64(_) => Err(JsNativeError::typ()
            .with_message("i64 wasm values not yet supported")
            .into()),
        wasmtime::Val::F32(n) => Ok(JsValue::from(f32::from_bits(*n) as f64)),
        wasmtime::Val::F64(n) => Ok(JsValue::from(f64::from_bits(*n))),
        _ => Err(JsNativeError::typ()
            .with_message("unsupported wasm result type")
            .into()),
    }
}

/// Get the `WebAssembly.Instance.prototype` from the global object.
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

// ── Instance type ──

/// <https://webassembly.github.io/spec/js-api/#instances>
pub(crate) fn register_wasm_instance_type(
    namespace: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        (),
    );

    // <https://webassembly.github.io/spec/js-api/#dom-instance-exports>
    // Step: "Return this.exports."
    let getter = NativeFunction::from_fn_ptr(get_instance_exports_fn);
    let realm = context.realm().clone();
    let getter_func = FunctionObjectBuilder::new(&realm, getter)
        .name("get exports")
        .build();

    proto.define_property_or_throw(
        js_string!("exports"),
        PropertyDescriptor::builder()
            .get(getter_func)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // <https://webassembly.github.io/spec/js-api/#dom-instance-instance>
    // Note: Constructor throws "Illegal constructor" — direct instantiation
    // from JS is not yet supported.
    let ctor_fn = NativeFunction::from_fn_ptr(|_this, _args, _context| {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    });
    register_wasm_constructor(namespace, "Instance", ctor_fn, 0, proto, context)
}

/// <https://webassembly.github.io/spec/js-api/#dom-instance-exports>
fn get_instance_exports_fn(
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

/// Register a constructor function on a namespace object.
pub(crate) fn register_wasm_constructor(
    namespace: &JsObject,
    name: &str,
    ctor_fn: NativeFunction,
    ctor_length: usize,
    proto: JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let realm = context.realm().clone();
    let ctor = FunctionObjectBuilder::new(&realm, ctor_fn)
        .name(name)
        .length(ctor_length)
        .constructor(true)
        .build();
    let ctor_obj: JsObject = ctor.into();

    // Wire F.prototype = proto.
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

    // Define on namespace.
    namespace.define_property_or_throw(
        js_string!(name),
        PropertyDescriptor::builder()
            .value(ctor_obj)
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )?;

    Ok(())
}
