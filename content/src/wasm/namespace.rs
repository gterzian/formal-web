//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>

use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue, js_string, object::JsObject,
    builtins::promise::ResolvingFunctions,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
};
use wasmtime::{Func, Instance as WasmtimeInstance, Module, Store};

use crate::html::{PendingRequest, PendingState, Window};
use crate::wasm::{instance_export_list, js_val_to_wasm_val, wasm_val_to_js_value, types::WasmInstance, types::WasmModule};
use crate::webidl::new_pending_promise;

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-validate>
#[allow(dead_code)]
// Note: Duplicated from `functions.rs` for internal use within this module.
// When `namespace.rs` is converted to use the `functions.rs` version, remove this.
pub(crate) fn validate_wasm_module(stable_bytes: &[u8]) -> bool {
    // Step 2: "Compile stableBytes as a WebAssembly module and store the results as module."
    // Step 3: "If module is error, return false."
    // Note: Steps 4-6 (validating builtins and imported strings) are not yet implemented.
    let engine = wasmtime::Engine::default();
    matches!(Module::new(&engine, stable_bytes), Ok(_))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn asynchronously_compile_a_webassembly_module(
    stable_bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Note: "Let stableBytes be a copy of the bytes held by the buffer bytes"
    // was already executed by the bindings layer.
    //
    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = new_pending_promise(context);
    // Step 2: "Run the following steps in parallel:"
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let request_id = window.global_scope.next_wasm_request_id();
    window.global_scope.push_pending_request(PendingRequest::WasmCompile {
        bytes: stable_bytes,
        request_id,
        is_instantiate: false,
        state: PendingState::Pending,
    });
    window.global_scope.store_wasm_resolver(request_id, promise.clone(), resolvers);
    // Step 3: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn asynchronously_instantiate_a_webassembly_module(
    wasm_module: &WasmModule,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = new_pending_promise(context);
    // Step 2: "Let module be moduleObject.[[Module]]."
    // Note: Already done by the bindings layer (downcast_ref from JS object).
    //
    // Steps 3-5 (builtin sets, imported strings, reading imports) are not
    // yet implemented — instantiation proceeds with empty imports.
    //
    // Step 6: "Run the following steps in parallel:"
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let request_id = window.global_scope.next_wasm_request_id();
    window.global_scope.push_pending_request(PendingRequest::WasmInstantiate {
        module: wasm_module.module.clone(),
        request_id,
        state: PendingState::Pending,
    });
    window.global_scope.store_wasm_resolver(request_id, promise.clone(), resolvers);
    // Step 7: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate>
pub(crate) fn instantiate_bytes(
    stable_bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Note: Step 1 ("Let stableBytes be a copy of the bytes held by the buffer bytes")
    // was already executed by the bindings layer.
    //
    // Step 2: "Asynchronously compile a WebAssembly module from stableBytes using
    //          options and let promiseOfModule be the result."
    // Step 3: "Instantiate promiseOfModule with imports importObject and return the result."
    // Note: Steps 2 and 3 are merged into a single worker request with
    // is_instantiate: true, because the compile and instantiate phases run
    // sequentially in the worker and the content process resolves the promise
    // after both complete.
    let (promise, resolvers) = new_pending_promise(context);
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let request_id = window.global_scope.next_wasm_request_id();
    window.global_scope.push_pending_request(PendingRequest::WasmCompile {
        bytes: stable_bytes,
        request_id,
        is_instantiate: true,
        state: PendingState::Pending,
    });
    window.global_scope.store_wasm_resolver(request_id, promise.clone(), resolvers);
    Ok(JsValue::from(promise))
}

// ── Promise resolution (called by the content process on completion) ──

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_continuation(
    resolvers: &ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<()> {
    // Step 2.2.5.1: "Construct a WebAssembly module object from module, bytes,
    //                builtinSetNames, importedStringModule, and let moduleObject
    //                be the result."
    // Note: builtinSetNames and importedStringModule are not yet supported.
    let module_proto = get_wasm_module_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let module_object = JsObject::from_proto_and_data(Some(module_proto), WasmModule::new(module, bytes));
    // Step 2.2.5.2: "Resolve promise with moduleObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_rejection(
    resolvers: &ResolvingFunctions,
    message: String,
    context: &mut Context,
) -> JsResult<()> {
    // Step 2.2.1: "If module is error, reject promise with a CompileError exception
    //              and return."
    let error = create_compile_error(&message, context);
    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn instantiate_continuation(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &ResolvingFunctions,
    context: &mut Context,
) -> JsResult<()> {
    let mut store_guard = store.lock().unwrap();
    // Step 6.1.2: "Let instanceObject be a new Instance."
    // Note: Step 6.1.1 (instantiate the core) was already done by the worker.
    // Step 6.1.3: "Initialize instanceObject from module and instance."
    let exports = create_exports_object(module, instance, &mut *store_guard, store, context)?;
    drop(store_guard);
    let instance_proto = get_wasm_instance_prototype(context)
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let instance_object = JsObject::from_proto_and_data(
        Some(instance_proto),
        WasmInstance::new(exports, Arc::clone(store), *instance),
    );
    // Step 6.1.4: "Resolve promise with instanceObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)?;
    Ok(())
}

// ── Helpers ──

fn get_wasm_module_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Module"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj.get(js_string!("prototype"), context).ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

fn get_wasm_instance_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Instance"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj.get(js_string!("prototype"), context).ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

fn create_compile_error(message: &str, context: &mut Context) -> JsValue {
    // Use the registered CompileError constructor on the WebAssembly namespace.
    if let Ok(ns) = context.global_object().get(js_string!("WebAssembly"), context) {
        if let Some(ns_obj) = ns.as_object() {
            if let Ok(ce_ctor) = ns_obj.get(js_string!("CompileError"), context) {
                if let Some(ctor) = ce_ctor.as_object() {
                    if let Ok(error) = ctor.call(&JsValue::undefined(), &[js_string!(message).into()], context) {
                        return error;
                    }
                }
            }
        }
    }
    // Fallback: plain string.
    JsValue::from(js_string!(message))
}

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
