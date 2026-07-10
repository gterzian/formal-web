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
use boa_engine::{JsError, JsNativeError, JsResult, JsValue, object::JsObject};
use js_engine::boa::BoaContext;
use js_engine::{Completion, ExecutionContext, JsTypes};

/// Bridge for Boa-gated wasm callers that pass `JsError`.
/// Converts `boa_engine::JsError` into a rejected promise via the generic
/// `rejected_promise_from_error` API.
fn rejected_promise_from_error_boa(
    error: boa_engine::JsError,
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> <crate::js::Types as JsTypes>::JsObject {
    let reason = match error.as_opaque() {
        Some(value) => value.clone(),
        None => {
            // Native error (e.g. TypeError) — convert to opaque JsValue.
            let ec_any = ec.as_any_mut();
            let boa_ctx = ec_any
                .downcast_mut::<js_engine::boa::BoaContext>()
                .expect("rejected_promise_from_error_boa only works on Boa backend");
            let ctx: &mut boa_engine::Context = boa_ctx.context();
            match error.into_opaque(ctx) {
                Ok(value) => value,
                Err(_) => ec.new_type_error("rejected_promise_from_error: cannot convert error"),
            }
        }
    };
    rejected_promise_from_error(reason, ec)
}

// ── Namespace type ──

/// Marker type for the `WebAssembly` namespace.
struct WasmNamespace;

/// <https://www.w3.org/TR/wasm-js-api/#webassembly-namespace>
///
/// The `WebAssembly` namespace object exposes validate, compile, instantiate,
/// and the JSTag attribute.
impl WebIdlNamespace<crate::js::Types> for WasmNamespace {
    const NAME: &'static str = "WebAssembly";

    fn define_members(def: &mut InterfaceDefinition<crate::js::Types>) {
        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-validate>
        def.add_operation(OperationDef {
            id: "validate",
            length: 1,
            method: validate_fn,
            static_: false,
            unforgeable: false,
            promise_type: false,
            exposed: None,
        });

        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-compile>
        def.add_operation(OperationDef {
            id: "compile",
            length: 1,
            method: compile_fn,
            static_: false,
            unforgeable: false,
            promise_type: true,
            exposed: None,
        });

        // <https://www.w3.org/TR/wasm-js-api/#dom-webassembly-instantiate>
        def.add_operation(OperationDef {
            id: "instantiate",
            length: 1,
            method: instantiate_fn,
            static_: false,
            unforgeable: false,
            promise_type: true,
            exposed: None,
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
            exposed: None,
        });
    }
}

// ── Installation entry point ──

/// <https://webassembly.github.io/spec/js-api/#webassembly-namespace>
pub(crate) fn install_wasm_namespace(engine: &mut BoaContext) -> JsResult<()> {
    // Step 1: "Let namespaceObject be OrdinaryObjectCreate(...)."
    // Step 2-3: Define regular attributes and operations.
    register_namespace_spec::<crate::js::Types, WasmNamespace, BoaContext>(engine)
        .map_err(JsError::from_opaque)?;

    // §3.13.1 step 5: Define [LegacyNamespace] interfaces on the namespace.
    register_interface_spec::<crate::js::Types, WasmModule, BoaContext>(engine)
        .map_err(JsError::from_opaque)?;
    register_interface_spec::<crate::js::Types, WasmInstance, BoaContext>(engine)
        .map_err(JsError::from_opaque)?;

    // Register error types (CompileError, LinkError, RuntimeError).
    // https://webassembly.github.io/spec/js-api/#error-objects
    let ec: &mut dyn ExecutionContext<crate::js::Types> = engine;
    let namespace_obj = resolve_wasm_namespace(ec).map_err(JsError::from_opaque)?;
    interfaces::register_wasm_error_types(&namespace_obj, engine.context())?;

    Ok(())
}

/// Resolve the `WebAssembly` namespace object from the global object.
fn resolve_wasm_namespace(
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsObject, crate::js::Types> {
    let global = ec.realm_global_object();
    let ns_value = ExecutionContext::get(ec, global, ec.property_key_from_str("WebAssembly"))?;
    let Some(namespace) = <crate::js::Types as JsTypes>::value_as_object(&ns_value) else {
        return Err(ec.new_type_error("WebAssembly namespace not found after registration"));
    };
    Ok(namespace)
}

// ── Namespace operation bindings ──
//
// Each binding function is a thin wrapper: extract JS arguments,
// call the corresponding domain function in `content/src/wasm/namespace.rs`,
// and wrap the result.

fn validate_fn(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let bytes_value = match args.first() {
        Some(val) => val,
        None => return Err(ec.new_type_error("WebAssembly.validate: missing argument")),
    };
    let stable_bytes = get_a_copy_of_the_buffer_source(bytes_value, ec)?;
    Ok(JsValue::new(validate_wasm_module(&stable_bytes)))
}

fn compile_fn(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let bytes_value = match args.first() {
        Some(val) => val,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.compile: missing argument")
                .into();
            return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
        }
    };
    let stable_bytes = match get_a_copy_of_the_buffer_source(bytes_value, ec) {
        Ok(bytes) => bytes,
        Err(opaque) => {
            let error: JsError = JsError::from_opaque(opaque);
            return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
        }
    };
    asynchronously_compile_a_webassembly_module(stable_bytes, ec)
}

fn instantiate_fn(
    _this: &JsValue,
    args: &[JsValue],
    ec: &mut dyn ExecutionContext<crate::js::Types>,
) -> Completion<JsValue, crate::js::Types> {
    let first = match args.first() {
        Some(val) => val,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.instantiate: missing argument")
                .into();
            return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
        }
    };

    // Dispatch to the right overload based on the first argument's type.
    if is_buffer_source(first, ec) {
        let stable_bytes = match get_a_copy_of_the_buffer_source(first, ec) {
            Ok(bytes) => bytes,
            Err(opaque) => {
                let error: JsError = JsError::from_opaque(opaque);
                return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
            }
        };
        return instantiate_bytes(stable_bytes, ec).or_else(|opaque| {
            let error: JsError = JsError::from_opaque(opaque);
            Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)))
        });
    }

    let module_object = match first.as_object() {
        Some(obj) => obj,
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message(
                    "WebAssembly.instantiate: first argument must be a buffer source or Module",
                )
                .into();
            return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
        }
    };
    // Extract the WasmModule data through with_object_any, cloning the
    // inner wasmtime::Module (a handle) to avoid borrowing ec.
    let wasm_module_clone = ec
        .with_object_any(&module_object)
        .and_then(|data| data.downcast_ref::<WasmModule>())
        .map(|m| m.module.clone());
    let wasm_module_inner = match wasm_module_clone {
        Some(module) => WasmModule::new(module, Vec::new()),
        None => {
            let error: JsError = JsNativeError::typ()
                .with_message("WebAssembly.instantiate: first argument does not implement the Module interface")
                .into();
            return Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)));
        }
    };
    asynchronously_instantiate_a_webassembly_module(&wasm_module_inner, ec).or_else(|opaque| {
        let error: JsError = JsError::from_opaque(opaque);
        Ok(JsValue::from(rejected_promise_from_error_boa(error, ec)))
    })
}
