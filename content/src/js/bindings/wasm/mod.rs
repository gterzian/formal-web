//! WebAssembly namespace binding.
//!
//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>
//!
//! Defines the `WebAssembly` namespace's Web IDL members (which operations
//! and attributes exist) and installs the namespace, its [LegacyNamespace]
//! interfaces (Module, Instance), and error types (CompileError, etc.) on
//! the global object.
//!
//! This is the bindings layer — the argument-extraction and result-wrapping
//! glue.  All implementation logic lives in `content/src/wasm/namespace.rs`.

mod interfaces;

use crate::wasm::{
    WasmInstance, WasmModule,
    namespace::{
        asynchronously_compile_a_webassembly_module,
        asynchronously_instantiate_a_webassembly_module, instantiate_bytes,
    },
    validate_wasm_module,
};
use crate::webidl::bindings::{
    AttributeDef, InterfaceDefinition, OperationDef, WebIdlNamespace, register_interface_spec,
    register_namespace_spec,
};
use crate::webidl::{
    get_a_copy_of_the_buffer_source, is_buffer_source, rejected_promise_from_error,
};
use boa_engine::{Context, JsError, JsNativeError, JsResult, JsValue, js_string, object::JsObject};

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
    register_interface_spec::<WasmModule>(context)?;
    register_interface_spec::<WasmInstance>(context)?;

    // Register error types (CompileError, LinkError, RuntimeError).
    // https://webassembly.github.io/spec/js-api/#error-objects
    interfaces::register_wasm_error_types(&resolve_wasm_namespace(context)?, context)?;

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
//
// Each binding function is a thin wrapper: extract JS arguments,
// call the corresponding domain function in `content/src/wasm/namespace.rs`,
// and wrap the result.

fn validate_fn(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let bytes_value = args.first().ok_or_else(|| {
        JsNativeError::typ().with_message("WebAssembly.validate: missing argument")
    })?;
    let stable_bytes = get_a_copy_of_the_buffer_source(bytes_value, context)?;
    Ok(JsValue::new(validate_wasm_module(&stable_bytes)))
}

fn compile_fn(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let bytes_value = match args.first() {
        Some(val) => val,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.compile: missing argument")
                .into();
            return Ok(rejected_promise_from_error(error, context).into());
        }
    };
    let stable_bytes = match get_a_copy_of_the_buffer_source(bytes_value, context) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(rejected_promise_from_error(error.into(), context).into());
        }
    };
    asynchronously_compile_a_webassembly_module(stable_bytes, context)
}

fn instantiate_fn(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let first = match args.first() {
        Some(val) => val,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.instantiate: missing argument")
                .into();
            return Ok(rejected_promise_from_error(error, context).into());
        }
    };

    // Dispatch to the right overload based on the first argument's type.
    if is_buffer_source(first, context) {
        let stable_bytes = match get_a_copy_of_the_buffer_source(first, context) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(rejected_promise_from_error(error.into(), context).into());
            }
        };
        return instantiate_bytes(stable_bytes, context);
    }

    let module_object = match first.as_object() {
        Some(obj) => obj,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message(
                    "WebAssembly.instantiate: first argument must be a buffer source or Module",
                )
                .into();
            return Ok(rejected_promise_from_error(error, context).into());
        }
    };
    let wasm_module = match module_object.downcast_ref::<WasmModule>() {
        Some(m) => m,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.instantiate: first argument does not implement the Module interface")
                .into();
            return Ok(rejected_promise_from_error(error, context).into());
        }
    };
    asynchronously_instantiate_a_webassembly_module(&*wasm_module, context)
}
