//! WebAssembly namespace binding.
//!
//! <https://www.w3.org/TR/wasm-js-api/#webassembly-namespace>
//!
//! Defines the `WebAssembly` namespace's Web IDL members (which operations
//! and attributes exist) and installs the namespace plus post-registration
//! types (CompileError, Module) on the global object.
//!
//! **This is a thin binding layer only.**  Spec-mapped implementations of
//! the namespace operations, the Module interface, and error-type setup
//! live in `content/src/wasm/functions.rs`.  Functions here convert
//! JavaScript arguments, interact with content-process state (global
//! scope, pending requests), and delegate to the domain layer.

use boa_engine::{
    Context, JsNativeError, JsResult, JsValue, js_string,
    object::{JsObject, builtins::JsPromise},
};

use crate::html::{PendingRequest, PendingState, Window};
use crate::wasm::{
    get_stable_bytes, is_buffer_source, register_wasm_error_types,
    register_wasm_instance_type, register_wasm_module_type,
    validate_wasm_module, WasmModule,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlNamespace, register_namespace_spec,
};

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
            getter: crate::wasm::get_wasm_jstag,
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

/// Install the `WebAssembly` namespace on the global object.
///
/// <https://www.w3.org/TR/wasm-js-api/#webassembly-namespace>
///
/// 1. Register the namespace object with its operations via
///    `register_namespace_spec` (Web IDL binding infrastructure).
/// 2. Add error types (CompileError, LinkError, RuntimeError) as
///    `Error` subclasses on the namespace.
/// 3. Add the `Module` type constructor with static methods on the
///    namespace.
///
/// These are separate post-registration steps because
/// `[LegacyNamespace=WebAssembly]` is not yet supported by the Web IDL
/// interface registration infrastructure.  As each type (Module, Instance,
/// etc.) is migrated to `WebIdlInterface`, the constructor will be slotted
/// into the namespace by `register_namespace_spec` automatically.
pub(crate) fn install_wasm_namespace(context: &mut Context) -> JsResult<()> {
    // Step: Register the namespace via the Web IDL bindings infra.
    register_namespace_spec::<WasmNamespace>(context)?;

    let ns_value = context
        .global_object()
        .get(js_string!("WebAssembly"), context)?;
    let Some(namespace) = ns_value.as_object() else {
        return Err(JsNativeError::error()
            .with_message("WebAssembly namespace not found after registration")
            .into());
    };
    let namespace = namespace.clone();

    // Register error types: CompileError, LinkError, RuntimeError
    register_wasm_error_types(&namespace, context)?;

    // Register type constructors (Module with static methods)
    register_wasm_module_type(&namespace, context)?;

    // Register Instance type with exports attribute
    register_wasm_instance_type(&namespace, context)?;

    Ok(())
}

// ── Namespace operation bindings ──

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-validate>
///
/// Steps 1-6: Extract stable bytes, validate synchronously, return boolean.
fn validate_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.validate: missing argument")
    })?;
    let stable_bytes = get_stable_bytes(bytes_value, context)?;

    // Steps 2-6: Compile, check errors, return true/false.
    Ok(JsValue::new(validate_wasm_module(&stable_bytes)))
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-compile>
///
/// Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
/// Step 2: "Asynchronously compile a WebAssembly module from stableBytes using
///          options and return the result."
///
/// The async compilation is kicked off by pushing a pending request onto the
/// GlobalScope.  The content process drains pending requests on the next
/// event-loop iteration, submits the bytes to the background compilation
/// worker, and resolves/rejects the promise when the result arrives.
fn compile_fn(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
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

    // Step: "Let promise be a new promise."
    let (promise, resolvers) = JsPromise::new_pending(context);
    let promise_obj: JsObject = promise.clone().into();

    // Push the pending request onto the GlobalScope.  The content process
    // will drain this after JS execution and submit to the background worker.
    global_scope.push_pending_request(PendingRequest::WasmCompile {
        bytes: stable_bytes,
        request_id,
        is_instantiate: false,
        promise: promise_obj,
        resolvers,
        state: PendingState::Pending,
    });

    // Step: "Return promise."
    Ok(promise.into())
}

/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate>
///
/// Two overloads:
///   1. `instantiate(bytes, importObject)`:
///      Step 1: "Let stableBytes be a copy of the bytes held by the buffer bytes."
///      Step 2: "Asynchronously compile a WebAssembly module from stableBytes..."
///      Step 3: "Instantiate promiseOfModule with imports importObject..."
///
///   2. `instantiate(moduleObject, importObject)`:
///      <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate-moduleobject-importobject>
///      "Asynchronously instantiate the WebAssembly module moduleObject importing
///       importObject, and return the result."
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

    // <https://webassembly.github.io/spec/js-api/#dom-webassembly-instantiate-moduleobject-importobject>
    //
    // Step: "Let module be moduleObject.[[Module]]."
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

/// Handle the bytes overload of `instantiate`.
///
/// <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate-bytes>
///
/// Step 1: "Let stableBytes be a copy..."
/// Step 2: "Asynchronously compile a WebAssembly module..."
/// Step 3: "Instantiate promiseOfModule with imports importObject..."
///
/// For now, this follows the same compile-only path as `compile()` and
/// full instantiation is deferred until the compile-and-then-instantiate
/// flow is wired through the background worker.
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
