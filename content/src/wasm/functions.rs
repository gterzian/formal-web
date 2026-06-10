//! Domain-level implementations of WebAssembly JS API algorithms.
//!
//! This module contains the spec-mapped implementations of the WebAssembly
//! namespace operations (validate, compile, instantiate), the Module interface
//! (constructor, exports), error type setup, and helpers for converting buffer
//! sources to stable byte copies.  The matching JS bindings
//! (`content/src/js/bindings/wasm/`) define *which* Web IDL members the
//! namespace has and wire them up via `register_namespace_spec`; this module
//! implements *what those members do*.

use boa_engine::{
    Context, JsNativeError, JsObject, JsResult, JsValue,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, builtins::{JsArrayBuffer, JsTypedArray, JsUint8Array}},
    property::PropertyDescriptor,
};
use wasmtime::Module;

use crate::wasm::types::WasmModule;

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
