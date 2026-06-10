//! Domain-level implementations of WebAssembly JS API algorithms.
//!
//! This module contains the spec-mapped implementations of the WebAssembly
//! namespace operations (validate, compile, instantiate), the Module interface
//! (constructor, exports), error type setup, and helpers for converting buffer
//! sources to stable byte copies.  The matching JS bindings
//! (`content/src/js/bindings/wasm/`) define *which* Web IDL members the
//! namespace has and wire them up via `register_namespace_spec`; this module
//! implements *what those members do*.

use std::sync::Arc;

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, builtins::{JsArrayBuffer, JsTypedArray, JsUint8Array}},
    property::PropertyDescriptor,
};
use wasmtime::{Instance as WasmtimeInstance, Module, Store};

use crate::wasm::types::{WasmInstance, WasmModule};

// ── Buffer-source helpers ──

/// <https://webidl.spec.whatwg.org/#dfn-get-buffer-source-copy>
///
/// Extract a stable copy of the bytes held by a buffer source (ArrayBuffer
/// or typed array).  Used by all wasm namespace operations that accept bytes.
///
/// Step: "Let stableBytes be a copy of the bytes held by the buffer bytes."
pub(crate) fn get_stable_bytes(value: &JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebAssembly: argument must be an ArrayBuffer or typed array")
    })?;

    // Try as typed array first (Uint8Array, etc.).
    if let Ok(typed_array) = JsTypedArray::from_object(object.clone()) {
        let length = typed_array.length(context)?;
        let mut bytes = vec![0u8; length];
        for i in 0..length {
            let v = object.get(i, context).map_err(|_| {
                JsNativeError::typ().with_message("failed to read typed array")
            })?;
            if let Some(num) = v.as_number() {
                bytes[i] = num as u8;
            }
        }
        return Ok(bytes);
    }

    // Try as ArrayBuffer — create a Uint8Array view and read via indexed access.
    if let Ok(array_buffer) = JsArrayBuffer::from_object(object.clone()) {
        if let Some(buf_bytes) = array_buffer.to_vec() {
            return Ok(buf_bytes);
        }
        let view = JsUint8Array::from_array_buffer(array_buffer, context)?;
        let view_obj: JsObject = view.into();
        let len = view_obj
            .get(js_string!("length"), context)
            .ok()
            .and_then(|v| v.as_number())
            .map(|n| n as usize)
            .unwrap_or(0);
        let mut bytes = vec![0u8; len];
        for i in 0..len {
            let v = view_obj.get(i, context).map_err(|_| {
                JsNativeError::typ().with_message("failed to read array buffer")
            })?;
            if let Some(num) = v.as_number() {
                bytes[i] = num as u8;
            }
        }
        return Ok(bytes);
    }

    Err(JsNativeError::typ()
        .with_message("WebAssembly: argument must be an ArrayBuffer or typed array")
        .into())
}

/// <https://webidl.spec.whatwg.org/#dfn-buffer-source-type>
///
/// Check whether a value is a buffer source (ArrayBuffer or typed array).
pub(crate) fn is_buffer_source(value: &JsValue, _context: &mut Context) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    JsArrayBuffer::from_object(object.clone()).is_ok()
        || JsTypedArray::from_object(object.clone()).is_ok()
}

// ── Namespace operation implementations ──

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-validate>
///
/// Steps 1-6: "Compile stableBytes as a WebAssembly module and store the
/// results as module.  If module is error, return false.  Return true."
///
/// Note: Steps 4–6 (builtins, imported string constants validation) are
/// not yet implemented.
pub(crate) fn validate_wasm_module(stable_bytes: &[u8]) -> bool {
    // Step 2-3: "Compile the WebAssembly module ... If module is error, return false."
    let engine = wasmtime::Engine::default();
    matches!(Module::new(&engine, stable_bytes), Ok(_))
}

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

// ── Promise resolution (async compile, Step 2.2) ──

/// Resolve a pending wasm promise with a compiled module.
///
/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
///
/// Steps 2.2.5.1-2.2.5.2:
///   "Construct a WebAssembly module object from module, bytes, ... and let
///    moduleObject be the result."
///   "Resolve promise with moduleObject."
pub(crate) fn resolve_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<()> {
    // Step 2.2.5.1: "Construct a WebAssembly module object ..."
    // Use WebAssembly.Module.prototype as the prototype.
    let module_proto = get_wasm_module_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());

    let module_object = JsObject::from_proto_and_data(
        Some(module_proto),
        WasmModule::new(module, bytes),
    );

    // Step 2.2.5.2: "Resolve promise with moduleObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)?;
    Ok(())
}

/// Reject a pending wasm promise with a CompileError.
///
/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
///
/// Step 2.2.1: "If module is error, reject promise with a CompileError
/// exception and return."
pub(crate) fn reject_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    message: String,
    context: &mut Context,
) -> JsResult<()> {
    // Create a proper CompileError instance by creating an Error and setting
    // its prototype to WebAssembly.CompileError.prototype.
    let error = context
        .intrinsics()
        .constructors()
        .error()
        .constructor()
        .call(
            &JsValue::undefined(),
            &[JsValue::from(js_string!(message.as_str()))],
            context,
        )?;

    if let Some(ce_proto) = get_wasm_compile_error_prototype(context) {
        if let Some(err_obj) = error.as_object() {
            err_obj.set_prototype(Some(ce_proto));
        }
    }

    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)?;
    Ok(())
}

// ── Error type registration (CompileError, LinkError, RuntimeError) ──

/// <https://www.w3.org/TR/wasm-js-api/#compilenamederror>
///
/// Register WebAssembly error types (CompileError, LinkError, RuntimeError)
/// as subclasses of `Error` on the namespace object.
///
/// Each error type has:
/// - A constructor function that delegates to `Error`'s constructor.
/// - A prototype whose `[[Prototype]]` is `Error.prototype`.
/// - The `name` property set to the error type name.
/// - The `message` property initialized to the empty string.
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
        // Create the error constructor.
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

        // Create the prototype that inherits from Error.prototype.
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

        // Wire F.prototype = proto and proto.constructor = F.
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

// ── Module type registration ──

/// <https://www.w3.org/TR/wasm-js-api/#module-objects>
///
/// Register `WebAssembly.Module` on the namespace, including the constructor
/// and static methods (`exports`, `imports`, `customSections`).
///
/// Note: `imports` and `customSections` are not yet implemented.
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
    // <https://www.w3.org/TR/wasm-js-api/#dom-module-exports>
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

    // Module.imports(moduleObject) — not yet implemented.
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

    // Module.customSections(moduleObject, sectionName) — not yet implemented.
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

    // Register the constructor.
    // <https://www.w3.org/TR/wasm-js-api/#dom-module-module>
    let ctor_fn = NativeFunction::from_fn_ptr(module_constructor_fn);
    register_wasm_constructor(namespace, "Module", ctor_fn, 1, proto, context)
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-module-module>
///
/// Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
/// Step 2: "Compile the WebAssembly module stableBytes and store the result as module."
/// Step 3: "If module is error, throw a CompileError exception."
/// Steps 7-10: Set [[Module]], [[Bytes]], [[BuiltinSets]], [[ImportedStringModule]].
fn module_constructor_fn(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: Let stableBytes be a copy of the bytes held by the buffer bytes.
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("Module constructor: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

    // Step 2: "Compile the WebAssembly module stableBytes and store the result as module."
    // Step 3: "If module is error, throw a CompileError exception."
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
        JsNativeError::typ()
            .with_message(format!("CompileError: {}", error))
    })?;

    // Steps 7-10: Set [[Module]], [[Bytes]], [[BuiltinSets]], [[ImportedStringModule]].
    // Note: [[BuiltinSets]] and [[ImportedStringModule]] are not yet implemented.
    let module_object = JsObject::from_proto_and_data(
        // Get Module.prototype as the prototype — note the constructor may be
        // called with a different `new.target`, but for now we use a fixed proto.
        get_wasm_module_prototype(context)
            .unwrap_or_else(|| context.intrinsics().constructors().object().prototype()),
        WasmModule::new(module, stable_bytes),
    );
    Ok(module_object.into())
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-module-exports>
///
/// Step 1: "Let module be moduleObject.[[Module]]."
/// Steps 2-4: "Let exports be « ».  For each (name, type) of module_exports(module),
///            create an object with "name" and "kind" properties and append it to exports."
/// Step 5: "Return exports."
fn module_exports_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: Validate argument is a Module object.
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

    let exports_array = boa_engine::object::builtins::JsArray::new(context)?;

    // Step 3: "For each (name, type) of module_exports(module),"
    for export in wasm_module.module.exports() {
        let name = export.name();
        let kind_str = match export.ty() {
            wasmtime::ExternType::Func(_) => "function",
            wasmtime::ExternType::Table(_) => "table",
            wasmtime::ExternType::Memory(_) => "memory",
            wasmtime::ExternType::Global(_) => "global",
            wasmtime::ExternType::Tag(_) => "tag",
        };

        // Create an export descriptor object: «[ "name" → name, "kind" → kind ]».
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

        // Step 3: "Append obj to exports."
        exports_array.push(entry, context)?;
    }

    // Step 5: "Return exports."
    Ok(JsValue::from(exports_array))
}

// ── JSTag ──

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-jstag>
///
/// Not yet implemented.
pub(crate) fn get_wasm_jstag(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    Err(JsNativeError::error()
        .with_message("WebAssembly.JSTag: not yet implemented")
        .into())
}

// ── Helper: register constructor on namespace ──

// ── Instantiation ──

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
///
/// Create a JS Instance object from a worker-completed instantiation.
/// Called on the main thread when processing `WasmResult::Instantiated`.
///
/// Steps 6.2–6.4: "Let instanceObject be a new Instance."
///                "Initialize instanceObject from module and instance."
///                "Resolve promise with instanceObject."
pub(crate) fn resolve_instantiate_promise(
    module: &wasmtime::Module,
    instance: &WasmtimeInstance,
    store: &Arc<Store<()>>,
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    context: &mut Context,
) -> JsResult<()> {
    // Step 6.2: "Let instanceObject be a new Instance."
    // Step 6.3: "Initialize instanceObject from module and instance."
    let exports = create_exports_object(module, instance, store, context)?;

    let instance_proto = get_wasm_instance_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());

    let instance_object = JsObject::from_proto_and_data(
        Some(instance_proto),
        WasmInstance::new(exports, Arc::clone(store), *instance),
    );

    // Step 6.4: "Resolve promise with instanceObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#create-an-exports-object>
///
/// Steps 1-8: Create a frozen object with wrapper values for each export.
///
/// For each `(name, externtype)` of `module_exports(module)`:
///   - func → wraps as a JS-callable NativeFunction
///   - memory, table, global, tag → not yet implemented (stub)
pub(crate) fn create_exports_object(
    module: &wasmtime::Module,
    instance: &WasmtimeInstance,
    store: &Arc<Store<()>>,
    context: &mut Context,
) -> JsResult<JsObject> {
    // Step 1: "Let exportsObject be ! OrdinaryObjectCreate(null)."
    // https://tc39.es/ecma262/#sec-ordinaryobjectcreate
    let exports_object = JsObject::from_proto_and_data(None, ());

    // Step 2: "For each (name, externtype) of module_exports(module),"
    for export in module.exports() {
        let name = export.name();
        let _extern_type = export.ty();

        // Step 3-4: "Let externval be instance_export(instance, name)."
        let extern_val = {
            // Borrow the Arc's Store via a temporary store handle.
            // `instance.get_export` needs `impl AsContextMut` which
            // `&mut Store<()>` satisfies.
            // SAFETY: `Arc::as_ptr` gives a raw pointer we only use through
            // the safe Store API that the Arc owns.
            let mut store_ref = unsafe { &mut *(Arc::as_ptr(store) as *mut Store<()>) };
            instance.get_export(&mut store_ref, name)
        };

        let Some(extern_val) = extern_val else {
            continue;
        };

        let value = match extern_val {
            // Step 5: func functype → create Exported Function
            wasmtime::Extern::Func(func) => {
                create_exported_function_wrapper(func, Arc::clone(store), context)?
            }
            // Steps 6-9: memory, global, table, tag — not yet implemented
            _ => JsValue::undefined(),
        };

        // Step 10: "Let status be ! CreateDataProperty(exportsObject, name, value)."
        // https://tc39.es/ecma262/#sec-createdataproperty
        exports_object
            .set(js_string!(name), value.clone(), false, context)
            .map_err(|_| JsNativeError::typ().with_message("failed to set export property"))?;
    }

    // Step 11: "Perform ! SetIntegrityLevel(exportsObject, "frozen")."
    // Note: Boa does not expose SetIntegrityLevel directly, skip for now.

    // Step 12: "Return exportsObject."
    Ok(exports_object)
}

/// Create a JS-callable function wrapper for a wasm exported function.
///
/// The returned NativeFunction captures the wasmtime `Func` handle and
/// the shared store.  When called from JS, it converts arguments
/// to `wasmtime::Val`, calls `func.call`, and converts results back.
fn create_exported_function_wrapper(
    func: wasmtime::Func,
    store: Arc<Store<()>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // SAFETY: The closure captures `Arc<Store<()>>` and `wasmtime::Func`.
    // Neither type contains Boa GC pointers, so `from_closure` is safe.
    let js_func = unsafe {
        NativeFunction::from_closure(
            move |_this: &JsValue, args: &[JsValue], context: &mut Context| -> JsResult<JsValue> {
                // SAFETY: `Arc::as_ptr` gives a raw pointer; we know the
                // `Arc` is alive because the closure (and hence the
                // `WasmInstance` that owns another clone) keeps it alive.
                // The store is Send + Sync per the wasmtime docs.
                let mut store_ref =
                    unsafe { &mut *(Arc::as_ptr(&store) as *mut Store<()>) };

                // Get the function type to determine parameter structure.
                let func_type = func.ty(&store_ref);

                // Convert JS args to wasm params.
                let params: Vec<wasmtime::Val> = func_type
                    .params()
                    .enumerate()
                    .map(|(i, param_type)| {
                        let js_arg = args.get(i).cloned().unwrap_or(JsValue::undefined());
                        js_val_to_wasm_val(&js_arg, &param_type, context)
                    })
                    .collect::<Result<_, _>>()?;

                // Allocate result storage.
                let mut results = vec![wasmtime::Val::I32(0); func_type.results().len()];

                // Call the wasm function.
                func.call(&mut store_ref, &params, &mut results).map_err(|error| {
                    JsNativeError::error()
                        .with_message(format!("wasm trap: {}", error))
                })?;

                // Convert results back to JS values.
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

    // Wrap the NativeFunction as a JsValue.
    let realm = context.realm().clone();
    let func_object = FunctionObjectBuilder::new(&realm, js_func).build();
    Ok(JsValue::from(func_object))
}

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
///
/// Convert a JS value to a wasmtime `Val` of the given type.
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
///
/// Convert a wasmtime `Val` to a JS value.
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

/// <https://webassembly.github.io/spec/js-api/#create-an-exports-object>
///
/// Register the `WebAssembly.Instance` interface on the namespace,
/// with the readonly `exports` attribute.
pub(crate) fn register_wasm_instance_type(
    namespace: &JsObject,
    context: &mut Context,
) -> JsResult<()> {
    // Prototype with the `exports` accessor.
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        (),
    );

    // Add `get exports` accessor to the prototype.
    // <https://webassembly.github.io/spec/js-api/#dom-instance-exports>
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

    // Register a constructor that throws "Illegal constructor" (user said
    // they don't want to implement `new Instance()`).
    let ctor_fn = NativeFunction::from_fn_ptr(|_this, _args, _context| {
        Err(JsNativeError::typ()
            .with_message("Illegal constructor")
            .into())
    });
    register_wasm_constructor(namespace, "Instance", ctor_fn, 0, proto, context)
}

/// <https://webassembly.github.io/spec/js-api/#dom-instance-exports>
///
/// Getter for `instance.exports`, returning the exports object that was
/// created during instantiation.
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
///
/// Creates a function with the given name and length, wires `F.prototype = proto`
/// and `proto.constructor = F`, and defines the constructor on the namespace.
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
