//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>

use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsValue, builtins::promise::ResolvingFunctions,
    js_string, native_function::NativeFunction, object::FunctionObjectBuilder, object::JsObject,
};
use wasmtime::{Func, Instance as WasmtimeInstance, Module, Store};

use crate::html::{PendingRequest, PendingState, Window};
use crate::wasm::conversions::{default_val_for_type, js_val_to_wasm_val, wasm_val_to_js_value};
use crate::wasm::types::{WasmInstance, WasmModule};
use crate::webidl::a_new_promise;

use js_engine::{Completion, ExecutionContext};

// ── Namespace operations ──

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-validate>
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
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Note: "Let stableBytes be a copy of the bytes held by the buffer bytes"
    // was already executed by the bindings layer.
    //
    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = a_new_promise(ec);
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 2: "Run the following steps in parallel:"
    let global = context.global_object();
    let window = match global.downcast_ref::<Window>() {
        Some(w) => w,
        None => {
            return Err(crate::js::native_error_to_js_value(
                JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                context,
            ));
        }
    };
    let request_id = window.global_scope.next_wasm_request_id();
    window
        .global_scope
        .push_pending_request(PendingRequest::WasmCompile {
            bytes: stable_bytes,
            request_id,
            is_instantiate: false,
            state: PendingState::Pending,
        });
    window
        .global_scope
        .store_wasm_resolver(request_id, promise.clone(), resolvers);
    // Step 3: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn asynchronously_instantiate_a_webassembly_module(
    wasm_module: &WasmModule,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = a_new_promise(ec);
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 2: "Let module be moduleObject.[[Module]]."
    let module = wasm_module.module.clone();
    //
    // Steps 3-5 (builtin sets, imported strings, reading imports) are not
    // yet implemented — instantiation proceeds with empty imports.
    //
    // Step 6: "Run the following steps in parallel:"
    let global = context.global_object();
    let window = match global.downcast_ref::<Window>() {
        Some(w) => w,
        None => {
            return Err(crate::js::native_error_to_js_value(
                JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                context,
            ));
        }
    };
    let request_id = window.global_scope.next_wasm_request_id();
    window
        .global_scope
        .push_pending_request(PendingRequest::WasmInstantiate {
            module,
            request_id,
            state: PendingState::Pending,
        });
    window
        .global_scope
        .store_wasm_resolver(request_id, promise.clone(), resolvers);
    // Step 7: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate>
pub(crate) fn instantiate_bytes(
    stable_bytes: Vec<u8>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
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
    let (promise, resolvers) = a_new_promise(ec);
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let global = context.global_object();
    let window = match global.downcast_ref::<Window>() {
        Some(w) => w,
        None => {
            return Err(crate::js::native_error_to_js_value(
                JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                context,
            ));
        }
    };
    let request_id = window.global_scope.next_wasm_request_id();
    window
        .global_scope
        .push_pending_request(PendingRequest::WasmCompile {
            bytes: stable_bytes,
            request_id,
            is_instantiate: true,
            state: PendingState::Pending,
        });
    window
        .global_scope
        .store_wasm_resolver(request_id, promise.clone(), resolvers);
    Ok(JsValue::from(promise))
}

// ── Promise resolution (called by the content process on completion) ──

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_continuation(
    resolvers: &ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 2.2.5.1: "Construct a WebAssembly module object from module, bytes,
    //                builtinSetNames, importedStringModule, and let moduleObject
    //                be the result."
    // Note: builtinSetNames and importedStringModule are not yet supported.
    let module_proto = get_wasm_module_prototype(js_engine::boa::context_as_ec(context))
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let module_object =
        JsObject::from_proto_and_data(Some(module_proto), WasmModule::new(module, bytes));
    // Step 2.2.5.2: "Resolve promise with moduleObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_rejection(
    resolvers: &ResolvingFunctions,
    message: String,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 2.2.1: "If module is error, reject promise with a CompileError exception
    //              and return."
    let error = create_compile_error(&message, js_engine::boa::context_as_ec(context));
    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn instantiate_continuation(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &ResolvingFunctions,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Note: Step 6.1.1 (instantiate the core) was already done by the worker.
    //
    // Step 6.1.2: "Let instanceObject be a new Instance."
    // Step 6.1.3: "Initialize instanceObject from module and instance."
    let instance_object = initialize_an_instance_object(
        module,
        instance,
        store,
        js_engine::boa::context_as_ec(context),
    )?;
    // Step 6.1.4: "Resolve promise with instanceObject."
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#initialize-an-instance-object>
fn initialize_an_instance_object(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 1: "Create an exports object from module and instance and let exportsObject be the result."
    let mut store_guard = store.lock().unwrap();
    let exports_object = create_an_exports_object(
        module,
        instance,
        &mut *store_guard,
        store,
        js_engine::boa::context_as_ec(context),
    )?;
    drop(store_guard);

    // Step 2: "Set instanceObject.[[Instance]] to instance."
    // Step 3: "Set instanceObject.[[Exports]] to exportsObject."
    // These are both done by constructing the WasmInstance with those fields.
    let instance_proto = get_wasm_instance_prototype(js_engine::boa::context_as_ec(context))
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let instance_object = JsObject::from_proto_and_data(
        Some(instance_proto),
        WasmInstance::new(exports_object, Arc::clone(store), *instance),
    );
    crate::js::js_result_to_completion(Ok(instance_object), context)
}

// ── Exports object ──

/// <https://webassembly.github.io/spec/js-api/#create-an-exports-object>
pub(crate) fn create_an_exports_object(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &mut Store<()>,
    store_arc: &Arc<Mutex<Store<()>>>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Step 1: "Let exportsObject be ! OrdinaryObjectCreate(null)."
    let exports_object = JsObject::from_proto_and_data(None, ());

    // Step 2: "For each (name, externtype) of module_exports(module),"
    // Note: instance_export_list wraps module.exports() (.Step 2 iterable)
    // and instance.get_export() (.Step 2.1 externval lookup).
    let export_list = instance_export_list(module, instance, store);

    // Note: Only exported functions are implemented.  For memory, table,
    // global, and tag exports the current implementation returns undefined.
    // The spec's per-externtype branches (Steps 2.3-2.7) are collapsed
    // into a single match because Func is the only implemented extern kind.
    for (name, extern_val) in &export_list {
        // Step 2.3-2.7: "If externtype is of the form ..."
        let value = match extern_val {
            wasmtime::Extern::Func(func) => {
                // Steps 2.3.x:  "Let func be the result of creating a new
                //                Exported Function from funcaddr."
                create_exported_function_wrapper(
                    *func,
                    Arc::clone(store_arc),
                    js_engine::boa::context_as_ec(context),
                )?
            }
            _ => JsValue::undefined(),
        };
        // Step 2.8: "Let status be ! CreateDataProperty(exportsObject, name, value)."
        exports_object
            .set(js_string!(name.as_str()), value, false, context)
            .map_err(|_| JsNativeError::typ().with_message("failed to set export property"))
            .map_err(|error| crate::js::native_error_to_js_value(error, context))?;
    }
    // Step 3: "Perform ! SetIntegrityLevel(exportsObject, "frozen")."
    // Note: Boa does not expose SetIntegrityLevel directly; skip for now.
    //
    // Step 4: "Return exportsObject."
    crate::js::js_result_to_completion(Ok(exports_object), context)
}

/// Return `(name, externval)` pairs for all exports of an instantiated module.
fn instance_export_list(
    module: &wasmtime::Module,
    instance: &WasmtimeInstance,
    store: &mut wasmtime::Store<()>,
) -> Vec<(String, wasmtime::Extern)> {
    module
        .exports()
        .filter_map(|export| {
            let name = export.name();
            instance
                .get_export(&mut *store, name)
                .map(|val| (name.to_string(), val))
        })
        .collect()
}

// ── Exported function wrapper (spec step: "creating a new Exported Function") ──

fn create_exported_function_wrapper(
    func: Func,
    store: Arc<Mutex<Store<()>>>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
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
                        let ec = js_engine::boa::context_as_ec(context);
                        js_val_to_wasm_val(&js_arg, &param_type, ec)
                            .map_err(|err_val| JsError::from_opaque(err_val))
                    })
                    .collect::<Result<_, _>>()?;
                let mut results: Vec<wasmtime::Val> = func_type
                    .results()
                    .map(|val_type| default_val_for_type(&val_type))
                    .collect();
                func.call(&mut *store_guard, &params, &mut results)
                    .map_err(|error| {
                        JsNativeError::error().with_message(format!("WebAssembly trap: {}", error))
                    })?;
                if results.len() == 1 {
                    let ec = js_engine::boa::context_as_ec(context);
                    wasm_val_to_js_value(&results[0], ec)
                        .map_err(|err_val| JsError::from_opaque(err_val))
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

// ── Helpers ──

fn get_wasm_module_prototype(ec: &mut dyn ExecutionContext<crate::js::Types>) -> Option<JsObject> {
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let ns = context
        .global_object()
        .get(js_string!("WebAssembly"), context)
        .ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Module"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj
        .get(js_string!("prototype"), context)
        .ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

fn get_wasm_instance_prototype(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Option<JsObject> {
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    let ns = context
        .global_object()
        .get(js_string!("WebAssembly"), context)
        .ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Instance"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj
        .get(js_string!("prototype"), context)
        .ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

fn create_compile_error(message: &str, ec: &mut dyn ExecutionContext<crate::js::Types>) -> JsValue {
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    // Use the registered CompileError constructor on the WebAssembly namespace.
    if let Ok(ns) = context
        .global_object()
        .get(js_string!("WebAssembly"), context)
    {
        if let Some(ns_obj) = ns.as_object() {
            if let Ok(ce_ctor) = ns_obj.get(js_string!("CompileError"), context) {
                if let Some(ctor) = ce_ctor.as_object() {
                    if let Ok(error) = ctor.call(
                        &JsValue::undefined(),
                        &[js_string!(message).into()],
                        context,
                    ) {
                        return error;
                    }
                }
            }
        }
    }
    // Fallback: plain string.
    JsValue::from(js_string!(message))
}
