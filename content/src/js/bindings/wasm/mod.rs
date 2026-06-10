//! WebAssembly namespace binding.
//!
//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>
//!
//! Defines the `WebAssembly` namespace's Web IDL members (which operations
//! and attributes exist) and installs the namespace, its [LegacyNamespace]
//! interfaces (Module, Instance), and error types (CompileError, etc.) on
//! the global object.
//!
//! This is the bindings layer — all JS-object creation, promise management,
//! and WebIdlInterface impls live here.  Domain logic (validate, compile)
//! is in `content/src/wasm/`.

mod interfaces;
pub(crate) use interfaces::{
    reject_compile_promise, resolve_compile_promise, resolve_instantiate_promise,
};

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue, js_string,
    object::{JsObject, builtins::JsPromise},
};

use crate::html::{PendingRequest, PendingState, Window};
use crate::wasm::{validate_wasm_module, WasmInstance, WasmModule};
use crate::webidl::{get_stable_bytes, is_buffer_source};
use crate::webidl::bindings::{
    register_interface_spec,
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlNamespace, register_namespace_spec,
};

/// Create a TypeError and pass it to the promise's reject function.
fn create_and_reject_type_error(
    resolvers: &boa_engine::builtins::promise::ResolvingFunctions,
    message: &str,
    context: &mut Context,
) -> JsResult<()> {
    let ctor = context
        .intrinsics()
        .constructors()
        .type_error()
        .constructor();
    let error = ctor.call(
        &JsValue::undefined(),
        &[js_string!(message).into()],
        context,
    )?;
    resolvers
        .reject
        .call(&JsValue::undefined(), &[error], context)?;
    Ok(())
}

// ── Namespace type ──

/// Marker type for the `WebAssembly` namespace.
struct WasmNamespace;

/// <https://www.w3.org/TR/wasm-js-api/#webassembly-namespace>
///
/// The `WebAssembly` namespace object exposes validate, compile, instantiate,
/// and the JSTag attribute.
impl WebIdlNamespace for WasmNamespace {
    const NAME: &'static str = "WebAssembly";

    fn define_members(def: &mut InterfaceDefinition) {
        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-validate>
        def.add_operation(OperationDef {
            id: "validate",
            length: 1,
            method: validate_fn,
            static_: false,
            unforgeable: false,
            promise_type: false,
        });

        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-compile>
        def.add_operation(OperationDef {
            id: "compile",
            length: 1,
            method: compile_fn,
            static_: false,
            unforgeable: false,
            promise_type: true,
        });

        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate>
        def.add_operation(OperationDef {
            id: "instantiate",
            length: 1,
            method: instantiate_fn,
            static_: false,
            unforgeable: false,
            promise_type: true,
        });

        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-jstag>
        def.add_attribute(AttributeDef {
            id: "JSTag",
            getter: interfaces::get_wasm_jstag,
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

// ── Installation entry point ──

/// <https://webassembly.github.io/spec/js-api/#webassembly-namespace>
pub(crate) fn install_wasm_namespace(context: &mut Context) -> JsResult<()> {
    // Step 1: "Let namespaceObject be OrdinaryObjectCreate(...)."
    // Step 2-3: Define regular attributes and operations.
    register_namespace_spec::<WasmNamespace>(context)?;

    // §3.13.1 step 5: Define [LegacyNamespace] interfaces on the namespace.
    // Module and Instance are registered via register_interface_spec which
    // checks legacy_namespace() and places them on the WebAssembly namespace.
    register_interface_spec::<WasmModule>(context)?;
    register_interface_spec::<WasmInstance>(context)?;

    // Register error types (CompileError, LinkError, RuntimeError) —
    // these are Error subclasses that need special prototype chain setup.
    // https://webassembly.github.io/spec/js-api/#error-objects
    interfaces::register_wasm_error_types(
        &resolve_wasm_namespace(context)?,
        context,
    )?;

    Ok(())
}

/// Resolve the `WebAssembly` namespace object from the global object.
fn resolve_wasm_namespace(context: &mut Context) -> JsResult<JsObject> {
    let ns_value = context
        .global_object()
        .get(js_string!("WebAssembly"), context)?;
    let Some(namespace) = ns_value.as_object() else {
        return Err(JsNativeError::error()
            .with_message("WebAssembly namespace not found after registration")
            .into());
    };
    Ok(namespace.clone())
}

// ── Namespace operation bindings ──

fn validate_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.validate: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;
    Ok(JsValue::new(validate_wasm_module(&stable_bytes)))
}

fn compile_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    let promise_obj: JsObject = promise.clone().into();

    let bytes_value = match args.first() {
        Some(v) => v,
        None => {
            if let Err(error) = create_and_reject_type_error(
                &resolvers,
                "WebAssembly.compile: missing argument",
                context,
            ) {
                eprintln!("wasm: failed to reject compile promise: {error}");
            }
            return Ok(promise.into());
        }
    };

    let stable_bytes = match get_stable_bytes(bytes_value, context) {
        Ok(b) => b,
        Err(_) => {
            if let Err(error) = create_and_reject_type_error(
                &resolvers,
                "WebAssembly.compile: invalid argument",
                context,
            ) {
                eprintln!("wasm: failed to reject compile promise: {error}");
            }
            return Ok(promise.into());
        }
    };

    // Note: The async compilation is kicked off by pushing a pending request
    // onto the GlobalScope.  The content process drains pending requests on
    // the next event-loop iteration, submits the bytes to the background
    // compilation worker, and resolves/rejects the promise when the result
    // arrives.
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let global_scope = &window.global_scope;

    let request_id = global_scope.next_wasm_request_id();

    global_scope.push_pending_request(PendingRequest::WasmCompile {
        bytes: stable_bytes,
        request_id,
        is_instantiate: false,
        promise: promise_obj,
        resolvers,
        state: PendingState::Pending,
    });

    Ok(promise.into())
}

fn instantiate_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let first = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.instantiate: missing argument")
    })?;

    // Check if first argument is a buffer source (bytes overload)
    // or a Module object (module-object overload).
    if is_buffer_source(first, context) {
        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate-bytes>
        return instantiate_bytes_overload(first, args.get(1), context);
    }

    let module_object = first.as_object().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebAssembly.instantiate: first argument must be a buffer source or Module")
    })?;

    let wasm_module = module_object.downcast_ref::<WasmModule>().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("WebAssembly.instantiate: first argument does not implement the Module interface")
    })?;

    // The importObject is the second argument (optional).
    let import_object = args.get(1).cloned().unwrap_or(JsValue::undefined());

    // Create a promise and push a pending instantiate request.
    let global = context.global_object();
    let window = global.downcast_ref::<Window>().ok_or_else(|| {
        JsNativeError::error().with_message("wasm: global object is not a Window")
    })?;
    let global_scope = &window.global_scope;

    let request_id = global_scope.next_wasm_request_id();
    let (promise, resolvers) = JsPromise::new_pending(context);
    let promise_obj: JsObject = promise.clone().into();

    global_scope.push_pending_request(PendingRequest::WasmInstantiate {
        module: wasm_module.module.clone(),
        import_object,
        request_id,
        promise: promise_obj,
        resolvers,
        state: PendingState::Pending,
    });

    Ok(promise.into())
}

fn instantiate_bytes_overload(
    bytes_value: &JsValue,
    _import_object: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

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

    Ok(promise.into())
}
