//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>

use std::sync::{Arc, Mutex};

use boa_engine::{Context, JsNativeError, JsValue, js_string, object::JsObject};
use wasmtime::{Func, Instance as WasmtimeInstance, Module, Store};

use crate::html::Window;
use crate::wasm::{PendingRequest, PendingState};
use crate::wasm::conversions::{default_val_for_type, js_val_to_wasm_val, wasm_val_to_js_value};
use crate::wasm::types::{WasmInstance, WasmModule};
use crate::webidl::bindings::create_interface_instance;
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes, records::PromiseResolvers};

/// Convert Boa-native `ResolvingFunctions` to generic `PromiseResolvers<crate::js::Types>`.
fn resolvers_to_generic(
    resolvers: boa_engine::builtins::promise::ResolvingFunctions,
) -> PromiseResolvers<crate::js::Types> {
    PromiseResolvers {
        resolve: resolvers.resolve.into(),
        reject: resolvers.reject.into(),
    }
}

/// Convert a `JsResult<T>` into a `Completion<T, Types>` by translating
/// any Boa error into its opaque form (a `JsValue`).
fn js_result_to_completion<T>(
    result: boa_engine::JsResult<T>,
    context: &mut boa_engine::Context,
) -> js_engine::Completion<T, crate::js::Types> {
    result.map_err(|error| {
        error
            .into_opaque(context)
            .unwrap_or_else(|_| boa_engine::JsValue::undefined())
    })
}

/// Convert a `JsNativeError` into a `JsValue` suitable as a `Completion` error.
fn native_error_to_js_value(
    error: boa_engine::JsNativeError,
    context: &mut boa_engine::Context,
) -> boa_engine::JsValue {
    let js_error: boa_engine::JsError = error.into();
    js_error
        .into_opaque(context)
        .unwrap_or_else(|_| boa_engine::JsValue::undefined())
}

/// Creates a new pending promise using Boa APIs directly.
/// Wrapper for Boa-only wasm code that works with `&mut Context`.
fn a_new_promise_boa(
    context: &mut boa_engine::Context,
) -> (
    boa_engine::JsObject,
    boa_engine::builtins::promise::ResolvingFunctions,
) {
    let (promise, resolvers) = boa_engine::object::builtins::JsPromise::new_pending(context);
    (promise.into(), resolvers)
}

/// Extract the `Window` from a Boa `Context`'s global object.
/// This works around Boa's `JsObject::downcast_ref` not seeing through
/// the `NativeDataWrapper(TraceableBox(Window))` indirection by using
/// `create_object_with_any`'s companion `with_object_any` method.
fn window_from_context(context: &mut Context) -> Option<&Window> {
    let global = context.global_object();
    // SAFETY: BoaContext is repr(transparent) over Context.
    let boa_ctx: &js_engine::boa::BoaContext =
        unsafe { &*(context as *mut Context as *const js_engine::boa::BoaContext) };
    boa_ctx
        .with_object_any(&global)
        .and_then(|data| data.downcast_ref::<Window>())
}

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
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    asynchronously_compile_a_webassembly_module_boa(stable_bytes, context)
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
fn asynchronously_compile_a_webassembly_module_boa(
    stable_bytes: Vec<u8>,
    context: &mut Context,
) -> Completion<JsValue, crate::js::Types> {
    // Note: "Let stableBytes be a copy of the bytes held by the buffer bytes"
    // was already executed by the bindings layer.

    // Step 1: "Let promise be a new promise."
    // Step 2: "Run the following steps in parallel:"

    // Phase 1 — Extract request_id and push pending request.
    let request_id = {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        let id = window.global_scope.next_wasm_request_id();
        window
            .global_scope
            .push_pending_request(PendingRequest::WasmCompile {
                bytes: stable_bytes,
                request_id: id,
                is_instantiate: false,
                state: PendingState::Pending,
            });
        id
    };
    // Phase 2 — Create promise (&mut Context, no outstanding Window borrows).
    let (promise, resolvers) = a_new_promise_boa(context);
    // Phase 3 — Store resolver (re-acquire &Window).
    {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        window.global_scope.store_wasm_resolver(
            request_id,
            promise.clone(),
            resolvers_to_generic(resolvers),
        );
    }
    // Step 3: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn asynchronously_instantiate_a_webassembly_module(
    wasm_module: &WasmModule,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    asynchronously_instantiate_a_webassembly_module_boa(wasm_module, context)
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
fn asynchronously_instantiate_a_webassembly_module_boa(
    wasm_module: &WasmModule,
    context: &mut Context,
) -> Completion<JsValue, crate::js::Types> {
    // Step 1: "Let promise be a new promise."
    // Step 2: "Let module be moduleObject.[[Module]]."
    let module = wasm_module.module.clone();
    //
    // Steps 3-5 (builtin sets, imported strings, reading imports) are not
    // yet implemented — instantiation proceeds with empty imports.
    //
    // Step 6: "Run the following steps in parallel:"
    // Phase 1 — Push pending request before creating the promise.
    let request_id = {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        let id = window.global_scope.next_wasm_request_id();
        window
            .global_scope
            .push_pending_request(PendingRequest::WasmInstantiate {
                module,
                request_id: id,
                state: PendingState::Pending,
            });
        id
    };
    // Phase 2 — Create promise (&mut Context, no outstanding Window borrows).
    let (promise, resolvers) = a_new_promise_boa(context);
    // Phase 3 — Store resolver.
    {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        window.global_scope.store_wasm_resolver(
            request_id,
            promise.clone(),
            resolvers_to_generic(resolvers),
        );
    }
    // Step 7: "Return promise."
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate>
pub(crate) fn instantiate_bytes(
    stable_bytes: Vec<u8>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    instantiate_bytes_boa(stable_bytes, context)
}

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate>
fn instantiate_bytes_boa(
    stable_bytes: Vec<u8>,
    context: &mut Context,
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
    let request_id = {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        let id = window.global_scope.next_wasm_request_id();
        window
            .global_scope
            .push_pending_request(PendingRequest::WasmCompile {
                bytes: stable_bytes,
                request_id: id,
                is_instantiate: true,
                state: PendingState::Pending,
            });
        id
    };
    let (promise, resolvers) = a_new_promise_boa(context);
    {
        let window = match window_from_context(context) {
            Some(w) => w,
            None => {
                return Err(native_error_to_js_value(
                    JsNativeError::typ().with_message("WebAssembly: global object is not a Window"),
                    context,
                ));
            }
        };
        window.global_scope.store_wasm_resolver(
            request_id,
            promise.clone(),
            resolvers_to_generic(resolvers),
        );
    }
    Ok(JsValue::from(promise))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_continuation(
    resolvers: &PromiseResolvers<crate::js::Types>,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    compile_continuation_boa(resolvers, module, bytes, context)
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
fn compile_continuation_boa(
    resolvers: &PromiseResolvers<crate::js::Types>,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> Completion<(), crate::js::Types> {
    // Step 2.2.5.1: "Construct a WebAssembly module object from module, bytes,
    //                builtinSetNames, importedStringModule, and let moduleObject
    //                be the result."
    // Note: builtinSetNames and importedStringModule are not yet supported.
    // Use create_interface_instance to wrap data in NativeDataWrapper so
    // ec.with_object_any can find it later during instantiate.
    let ec = js_engine::boa::context_as_ec(context);
    let module_object: JsObject = create_interface_instance::<crate::js::Types, WasmModule>(
        WasmModule::new(module, bytes),
        ec,
    )?
    .into();
    // Step 2.2.5.2: "Resolve promise with moduleObject."
    let resolve: JsObject = resolvers.resolve.clone();
    resolve
        .call(&JsValue::undefined(), &[module_object.into()], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
pub(crate) fn compile_rejection(
    resolvers: &PromiseResolvers<crate::js::Types>,
    message: String,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    compile_rejection_boa(resolvers, message, context)
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
fn compile_rejection_boa(
    resolvers: &PromiseResolvers<crate::js::Types>,
    message: String,
    context: &mut Context,
) -> Completion<(), crate::js::Types> {
    // Step 2.2.1: "If module is error, reject promise with a CompileError exception
    //              and return."
    let error = create_compile_error_boa(&message, context);
    let reject: JsObject = resolvers.reject.clone();
    reject
        .call(&JsValue::undefined(), &[error], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
pub(crate) fn instantiate_continuation(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &PromiseResolvers<crate::js::Types>,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<(), crate::js::Types> {
    // SAFETY: ec is backed by BoaContext repr(transparent) over Context.
    let context = unsafe { js_engine::boa::ec_to_ctx(ec) };
    instantiate_continuation_boa(module, instance, store, resolvers, context)
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-instantiate-a-webassembly-module>
fn instantiate_continuation_boa(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    resolvers: &PromiseResolvers<crate::js::Types>,
    context: &mut Context,
) -> Completion<(), crate::js::Types> {
    // Note: Step 6.1.1 (instantiate the core) was already done by the worker.
    //
    // Step 6.1.2: "Let instanceObject be a new Instance."
    // Step 6.1.3: "Initialize instanceObject from module and instance."
    let instance_object = initialize_an_instance_object_boa(module, instance, store, context)?;
    // Step 6.1.4: "Resolve promise with instanceObject."
    let resolve: JsObject = resolvers.resolve.clone();
    resolve
        .call(&JsValue::undefined(), &[instance_object.into()], context)
        .map_err(|error| error.into_opaque(context).unwrap_or(JsValue::undefined()))?;
    Ok(())
}

/// <https://webassembly.github.io/spec/js-api/#initialize-an-instance-object>
fn initialize_an_instance_object_boa(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Create an exports object from module and instance and let exportsObject be the result."
    let mut store_guard = store.lock().unwrap();
    let exports_object =
        create_an_exports_object_boa(module, instance, &mut *store_guard, store, context)?;
    drop(store_guard);

    // Step 2: "Set instanceObject.[[Instance]] to instance."
    // Step 3: "Set instanceObject.[[Exports]] to exportsObject."
    // These are both done by constructing the WasmInstance with those fields.
    // Use create_interface_instance to wrap data in NativeDataWrapper so
    // ec.with_object_any can find it during exports getter access.
    let ec = js_engine::boa::context_as_ec(context);
    let instance_object: JsObject = create_interface_instance::<crate::js::Types, WasmInstance>(
        WasmInstance::new(exports_object, Arc::clone(store), *instance),
        ec,
    )?
    .into();
    js_result_to_completion(Ok(instance_object), context)
}

/// <https://webassembly.github.io/spec/js-api/#create-an-exports-object>
fn create_an_exports_object_boa(
    module: &Module,
    instance: &WasmtimeInstance,
    store: &mut Store<()>,
    store_arc: &Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> Completion<JsObject, crate::js::Types> {
    // Step 1: "Let exportsObject be ! OrdinaryObjectCreate(null)."
    let exports_object = JsObject::from_proto_and_data(None, ());

    // Step 2: "For each (name, externtype) of module_exports(module),"
    // Note: instance_export_list wraps module.exports() (Step 2 iterable)
    // and instance.get_export() (Step 2.1 externval lookup).
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
                create_exported_function_wrapper_boa(*func, Arc::clone(store_arc), context)?
            }
            _ => JsValue::undefined(),
        };
        // Step 2.8: "Let status be ! CreateDataProperty(exportsObject, name, value)."
        exports_object
            .set(js_string!(name.as_str()), value, false, context)
            .map_err(|_| JsNativeError::typ().with_message("failed to set export property"))
            .map_err(|error| native_error_to_js_value(error, context))?;
    }
    // Step 3: "Perform ! SetIntegrityLevel(exportsObject, "frozen")."
    // Note: Boa does not expose SetIntegrityLevel directly; skip for now.
    //
    // Step 4: "Return exportsObject."
    js_result_to_completion(Ok(exports_object), context)
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

fn create_exported_function_wrapper_boa(
    func: Func,
    store: Arc<Mutex<Store<()>>>,
    context: &mut Context,
) -> Completion<JsValue, crate::js::Types> {
    #[gc_struct]
    struct WasmExportCapture {
        #[ignore_trace]
        func: wasmtime::Func,
        #[ignore_trace]
        store: std::sync::Arc<std::sync::Mutex<wasmtime::Store<()>>>,
    }

    fn wasm_export_fn(
        args: &[<crate::js::Types as JsTypes>::JsValue],
        _this: <crate::js::Types as JsTypes>::JsValue,
        captures: &WasmExportCapture,
        ec: &mut dyn ExecutionContext<crate::js::Types>,
    ) -> Completion<<crate::js::Types as JsTypes>::JsValue, crate::js::Types> {
        let mut store_guard = captures.store.lock().unwrap();
        let func_type = captures.func.ty(&*store_guard);
        let params: Vec<wasmtime::Val> = func_type
            .params()
            .enumerate()
            .map(|(i, param_type)| {
                let js_arg = args.get(i).cloned().unwrap_or_else(|| ec.value_undefined());
                js_val_to_wasm_val(&js_arg, &param_type, ec).map_err(|err_val| err_val)
            })
            .collect::<Result<_, _>>()?;
        let mut results: Vec<wasmtime::Val> = func_type
            .results()
            .map(|val_type| default_val_for_type(&val_type))
            .collect();
        captures
            .func
            .call(&mut *store_guard, &params, &mut results)
            .map_err(|error| ec.new_type_error(&format!("WebAssembly trap: {}", error)))?;
        if results.len() == 1 {
            wasm_val_to_js_value(&results[0], ec).map_err(|err_val| err_val)
        } else {
            Err(ec.new_type_error("multiple wasm results not yet supported"))
        }
    }

    // Bridge once to get the generic EC — eliminating per-invocation
    // context_as_ec bridges.
    let engine = js_engine::boa::context_as_engine(context);
    let name_key = engine.property_key_from_str("");
    let js_func = crate::js::create_builtin_fn_with_traced_captures(
        engine,
        WasmExportCapture { func, store },
        wasm_export_fn,
        0,
        name_key,
        false,
    );
    Ok(js_func.into())
}

fn create_compile_error_boa(message: &str, context: &mut Context) -> JsValue {
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
