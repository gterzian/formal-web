//! <https://webassembly.github.io/spec/js-api/#webassembly-namespace>
//!
//! Spec-mapped implementations of the `WebAssembly` namespace operations
//! (validate, compile, instantiate).  Each function receives already-converted
//! Rust types (Vec<u8>, &WasmModule) — the JsValue→Rust-type conversion happens
//! in the bindings layer via `content/src/webidl/` helpers.

use boa_engine::{Context, JsNativeError, JsResult, JsValue};
use wasmtime::Module;

use crate::html::{GlobalScope, PendingRequest, PendingState, Window};
use crate::wasm::types::WasmModule;
use crate::webidl::new_pending_promise;

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-validate>
pub(crate) fn validate_wasm_module(stable_bytes: &[u8]) -> bool {
    // Step 2: "Compile stableBytes as a WebAssembly module and store the results as module."
    // Step 3: "If module is error, return false."
    // Note: Steps 4-6 (validating builtins and imported strings) are not yet implemented.
    let engine = wasmtime::Engine::default();
    matches!(Module::new(&engine, stable_bytes), Ok(_))
}

/// <https://webassembly.github.io/spec/js-api/#asynchronously-compile-a-webassembly-module>
///
/// Receives `stable_bytes` already extracted from the JS buffer source
/// by the bindings layer (via `crate::webidl::get_a_copy_of_the_buffer_source`).
pub(crate) fn asynchronously_compile_a_webassembly_module(
    stable_bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Note: Step 1 "Let stableBytes be a copy of the bytes held by the buffer bytes"
    // was already executed by the bindings layer via get_a_copy_of_the_buffer_source.

    // "Asynchronously compile a WebAssembly module from stableBytes using options and return the result."
    // Step 1: "Let promise be a new promise."
    let (promise, resolvers) = new_pending_promise(context);

    // Step 2: "Run the following steps in parallel:"
    // Note: The content process drains pending requests on the next
    // event-loop iteration and submits them to the background worker,
    // which performs the actual compilation "in parallel".
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
///
/// Receives `wasm_module` already extracted from the JS Module object
/// by the bindings layer (via `downcast_ref::<WasmModule>()`).
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

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-compile>
/// Instantiate bytes overload: compiles first, then instantiates.
///
/// Receives `stable_bytes` already extracted from the JS buffer source
/// by the bindings layer (via `crate::webidl::get_a_copy_of_the_buffer_source`).
/// Pushes a compile request flagged for
/// instantiation — the content process handles the second phase.
pub(crate) fn instantiate_bytes(
    stable_bytes: Vec<u8>,
    context: &mut Context,
) -> JsResult<JsValue> {
    // Note: "Let stableBytes be a copy of the bytes held by the buffer bytes"
    // was already executed by the bindings layer.

    // "Asynchronously compile a WebAssembly module from stableBytes using
    //  options and let promiseOfModule be the result."
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
        is_instantiate: true,
        state: PendingState::Pending,
    });
    window.global_scope.store_wasm_resolver(request_id, promise.clone(), resolvers);

    // "Instantiate promiseOfModule with imports importObject and return the result."
    // Note: The is_instantiate flag tells the content-process result handler
    // to treat the compiled module as the first step of instantiation.
    Ok(JsValue::from(promise))
}
