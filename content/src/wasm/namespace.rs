use boa_engine::{
    Context, JsNativeError, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    object::{JsObject, ObjectInitializer, builtins::{JsArrayBuffer, JsPromise, JsTypedArray, JsUint8Array}},
    property::Attribute,
};

use crate::html::{PendingRequest, PendingState, Window};
use crate::wasm::types::WasmModule;

/// <https://www.w3.org/TR/wasm-js-api/#webassembly-namespace>
///
/// Install the `WebAssembly` namespace object on the global object.
pub(crate) fn install_wasm_namespace(context: &mut Context) -> JsResult<()> {
    let mut namespace_init = ObjectInitializer::new(context);

    namespace_init.function(
        NativeFunction::from_fn_ptr(validate_fn),
        js_string!("validate"),
        1,
    );
    namespace_init.function(
        NativeFunction::from_fn_ptr(compile_fn),
        js_string!("compile"),
        1,
    );
    namespace_init.function(
        NativeFunction::from_fn_ptr(instantiate_fn),
        js_string!("instantiate"),
        1,
    );

    let namespace = namespace_init.build();

    // Register error types
    register_error_types(&namespace, context)?;

    // Register type constructors (Module and Instance stubs)
    register_module_type(&namespace, context)?;

    context.register_global_property(js_string!("WebAssembly"), namespace, Attribute::all())
}

fn register_error_types(namespace: &JsObject, context: &mut Context) -> JsResult<()> {
    let error_names = [
        ("CompileError", NativeFunction::from_fn_ptr(compile_error_ctor)),
        ("LinkError", NativeFunction::from_fn_ptr(compile_error_ctor)),
        ("RuntimeError", NativeFunction::from_fn_ptr(compile_error_ctor)),
    ];
    for (name, ctor_fn) in error_names {
        let error_proto = JsObject::from_proto_and_data(
            Some(context.intrinsics().constructors().error().prototype()),
            (),
        );
        let writable_config = boa_engine::property::PropertyDescriptor::builder()
            .writable(true)
            .configurable(true)
            .enumerable(false);

        error_proto.define_property_or_throw(
            js_string!("name"),
            writable_config.clone().value(js_string!(name)).build(),
            context,
        )?;
        error_proto.define_property_or_throw(
            js_string!("message"),
            writable_config.value(js_string!("")).build(),
            context,
        )?;

        let ctor = {
            let realm = context.realm().clone();
            let f = boa_engine::object::FunctionObjectBuilder::new(
                &realm,
                ctor_fn,
            )
            .name(name)
            .length(1)
            .constructor(true)
            .build();
            let ctor_obj: JsObject = f.into();
            ctor_obj.define_property_or_throw(
                js_string!("prototype"),
                boa_engine::property::PropertyDescriptor::builder()
                    .value(error_proto)
                    .writable(false)
                    .enumerable(false)
                    .configurable(false)
                    .build(),
                context,
            )?;
            ctor_obj
        };

        namespace.define_property_or_throw(
            js_string!(name),
            boa_engine::property::PropertyDescriptor::builder()
                .value(ctor)
                .writable(true)
                .configurable(true)
                .build(),
            context,
        )?;
    }
    Ok(())
}

fn compile_error_ctor(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let message = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    let error = context.intrinsics().constructors().error().constructor()
        .call(&JsValue::undefined(), &[JsValue::from(js_string!(message.as_str()))], context)?;
    Ok(error)
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-validate>
fn validate_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.validate: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

    let engine = wasmtime::Engine::default();
    match wasmtime::Module::new(&engine, &stable_bytes) {
        Err(_) => Ok(JsValue::new(false)),
        Ok(_) => Ok(JsValue::new(true)),
    }
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-compile>
///
/// Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
/// Step 2: "Asynchronously compile a WebAssembly module from stableBytes using
/// options and return the result."
///
/// Note: The compilation runs on the background thread. The promise is stored
/// on the GlobalScope and resolved when the background thread finishes.
fn compile_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.compile: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

    // Get the GlobalScope through the Window.
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let global_scope = &window.global_scope;

    // Allocate a request id for correlating the result.
    let request_id = global_scope.next_wasm_request_id();

    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = JsPromise::new_pending(context);
    let promise_obj: JsObject = promise.clone().into();

    // Push the pending request onto the GlobalScope.  The content process
    // will drain this after JS execution and submit to the background thread.
    global_scope.push_pending_request(PendingRequest::WasmCompile {
        bytes: stable_bytes,
        request_id,
        is_instantiate: false,
        promise: promise_obj,
        resolvers,
        state: PendingState::Pending,
    });

    // Step 3: "Return promise."
    Ok(promise.into())
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate>
fn instantiate_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let first = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.instantiate: missing argument")
    })?;

    if is_buffer_source(first, context) {
        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate-bytes>
        let stable_bytes = get_stable_bytes(first, context)?;

        let global = context.global_object();
        let window = global.downcast_ref::<Window>().ok_or_else(|| {
            JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
        let global_scope = &window.global_scope;

        let request_id = global_scope.next_wasm_request_id();
        let (promise, resolvers) = JsPromise::new_pending(context);
        let promise_obj: JsObject = promise.clone().into();

        global_scope.push_pending_request(PendingRequest::WasmCompile {
            bytes: stable_bytes,
            request_id,
            is_instantiate: true,
            promise: promise_obj,
            resolvers,
            state: PendingState::Pending,
        });

        return Ok(promise.into());
    } else {
        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate-module>
        Err(JsNativeError::error()
            .with_message("WebAssembly.instantiate(moduleObject): not yet implemented")
            .into())
    }
}

// ── Helpers ──

fn get_stable_bytes(value: &JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    let object = value.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebAssembly: argument must be an ArrayBuffer or typed array")
    })?;

    // Try as typed array first (Uint8Array, etc.).
    // Use indexed access on the object itself (typed arrays support [[Get]]).
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

    // Try as ArrayBuffer - create a Uint8Array view and read via indexed access
    if let Ok(array_buffer) = JsArrayBuffer::from_object(object.clone()) {
        if let Some(buf_bytes) = array_buffer.to_vec() {
            return Ok(buf_bytes);
        }
        let view = JsUint8Array::from_array_buffer(array_buffer, context)?;
        let view_obj: JsObject = view.into();
        let len = view_obj.get(js_string!("length"), context)
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

fn is_buffer_source(value: &JsValue, _context: &mut Context) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    JsArrayBuffer::from_object(object.clone()).is_ok()
        || JsTypedArray::from_object(object.clone()).is_ok()
}

// ── Promise resolution helpers (for future async use) ──

/// Get the WebAssembly.Module.prototype from the context's global object.
fn get_module_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("Module"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj.get(js_string!("prototype"), context).ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

/// Get the WebAssembly.CompileError.prototype from the context's global object.
fn get_compile_error_prototype(context: &mut Context) -> Option<JsObject> {
    let ns = context.global_object().get(js_string!("WebAssembly"), context).ok()?;
    let ns_obj = ns.as_object()?;
    let ctor = ns_obj.get(js_string!("CompileError"), context).ok()?;
    let ctor_obj = ctor.as_object()?;
    ctor_obj.get(js_string!("prototype"), context).ok()
        .and_then(|p| p.as_object().map(|o| o.clone()))
}

/// Resolve a pending wasm promise with a compiled module.
///
/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
/// Steps 2.2.5.1-2.2.5.2
pub(crate) fn resolve_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    module: wasmtime::Module,
    bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<()> {
    // Use WebAssembly.Module.prototype as the prototype
    let module_proto = get_module_prototype(context)
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

/// Reject a pending wasm promise with a CompileError.
///
/// <https://www.w3.org/TR/wasm-js-api/#asynchronously-compile-a-webassembly-module>
/// Step 2.2.1: "reject promise with a CompileError exception and return."
pub(crate) fn reject_compile_promise(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    message: String,
    context: &mut Context,
) -> JsResult<()> {
    // Create a proper CompileError instance by creating an Error and setting
    // its prototype to WebAssembly.CompileError.prototype
    let error = context.intrinsics().constructors().error().constructor()
        .call(&JsValue::undefined(), &[JsValue::from(js_string!(message.as_str()))], context)?;

    if let Some(ce_proto) = get_compile_error_prototype(context) {
        if let Some(err_obj) = error.as_object() {
            err_obj.set_prototype(Some(ce_proto));
        }
    }

    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)?;
    Ok(())
}

// ── Type registration helpers ──

/// Helper: create a constructor function with the given prototype.
fn register_constructor(
    namespace: &JsObject,
    name: &str,
    ctor_fn: NativeFunction,
    ctor_length: usize,
    proto: JsObject,
    context: &mut Context,
) -> JsResult<()> {
    let realm = context.realm().clone();
    let ctor = boa_engine::object::FunctionObjectBuilder::new(&realm, ctor_fn)
        .name(name)
        .length(ctor_length)
        .constructor(true)
        .build();
    let ctor_obj: JsObject = ctor.into();

    ctor_obj.define_property_or_throw(
        js_string!("prototype"),
        boa_engine::property::PropertyDescriptor::builder()
            .value(proto)
            .writable(false)
            .enumerable(false)
            .configurable(false)
            .build(),
        context,
    )?;

    namespace.define_property_or_throw(
        js_string!(name),
        boa_engine::property::PropertyDescriptor::builder()
            .value(ctor_obj)
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )?;

    Ok(())
}

/// Register WebAssembly.Module on the namespace.
fn register_module_type(namespace: &JsObject, context: &mut Context) -> JsResult<()> {
    // Prototype with static methods
    let proto = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        (),
    );

    // Module.exports(moduleObject)
    let exports_fn = NativeFunction::from_fn_ptr(module_exports_fn);
    let realm = context.realm().clone();
    let exports_func = boa_engine::object::FunctionObjectBuilder::new(&realm, exports_fn)
        .name("exports")
        .length(1)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("exports"),
        boa_engine::property::PropertyDescriptor::builder()
            .value(exports_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // Module.imports(moduleObject)
    let imports_fn = NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
        Err(JsNativeError::error()
            .with_message("WebAssembly.Module.imports: not yet implemented")
            .into())
    });
    let imports_func = boa_engine::object::FunctionObjectBuilder::new(&realm, imports_fn)
        .name("imports")
        .length(1)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("imports"),
        boa_engine::property::PropertyDescriptor::builder()
            .value(imports_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // Module.customSections(moduleObject, sectionName)
    let cs_fn = NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
        Err(JsNativeError::error()
            .with_message("WebAssembly.Module.customSections: not yet implemented")
            .into())
    });
    let cs_func = boa_engine::object::FunctionObjectBuilder::new(&realm, cs_fn)
        .name("customSections")
        .length(2)
        .constructor(false)
        .build();
    proto.define_property_or_throw(
        js_string!("customSections"),
        boa_engine::property::PropertyDescriptor::builder()
            .value(cs_func)
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build(),
        context,
    )?;

    // Constructor: new Module(bytes)
    let ctor_fn = NativeFunction::from_fn_ptr(module_ctor);
    register_constructor(namespace, "Module", ctor_fn, 1, proto, context)
}

/// WebAssembly.Module(bytes)
fn module_ctor(
    _new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("Module constructor: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

    let engine = wasmtime::Engine::default();
    wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
        // <https://www.w3.org/TR/wasm-js-api/#dom-module-module>
        // Step 3: "If module is error, throw a CompileError exception."
        JsNativeError::typ()
            .with_message(format!("CompileError: {}", error))
    })?;

    // Step 7-10: Set internal slots
    let _new_target = _new_target.clone();

    // Cannot use 'this' in NativeFunction constructors directly in boa.
    // Instead, we create a new object with the right data.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &stable_bytes).map_err(|error| {
        JsNativeError::typ()
            .with_message(format!("CompileError: {}", error))
    })?;

    let module_object = JsObject::from_proto_and_data(
        Some(context.intrinsics().constructors().object().prototype()),
        WasmModule::new(module, stable_bytes),
    );
    Ok(module_object.into())
}

/// WebAssembly.Module.exports(moduleObject)
fn module_exports_fn(
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

    let module_ref = module_object.downcast_ref::<WasmModule>().ok_or_else(|| {
        JsNativeError::typ().with_message("Module.exports: argument is not a WebAssembly.Module")
    })?;
    let wasm_module = &module_ref.module;

    let exports_array = boa_engine::object::builtins::JsArray::new(context)?;

    for export in wasm_module.exports() {
        let name = export.name();
        let kind_str = match export.ty() {
            wasmtime::ExternType::Func(_) => "function",
            wasmtime::ExternType::Table(_) => "table",
            wasmtime::ExternType::Memory(_) => "memory",
            wasmtime::ExternType::Global(_) => "global",
            wasmtime::ExternType::Tag(_) => "tag",
        };

        let entry = context.intrinsics().constructors().object().constructor()
            .call(&JsValue::undefined(), &[], context)?;
        let entry_obj = entry.as_object().ok_or_else(|| {
            JsNativeError::typ().with_message("failed to create export descriptor")
        })?;
        entry_obj.set(js_string!("name"), js_string!(name), false, context)?;
        entry_obj.set(js_string!("kind"), js_string!(kind_str), false, context)?;

        exports_array.push(entry, context)?;
    }

    Ok(JsValue::from(exports_array))
}


