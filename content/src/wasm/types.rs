use boa_engine::{JsObject, object::JsData};
use boa_gc::{Finalize, Trace};

/// <https://www.w3.org/TR/wasm-js-api/#module-objects>
///
/// A compiled WebAssembly module, stored as data on a JS Module object.
/// The JS-visible properties (exports, imports, customSections) are defined
/// as static methods on the Module constructor when the namespace is set up.
#[derive(Trace, Finalize)]
#[allow(dead_code)]
pub(crate) struct WasmModule {
    /// The compiled wasmtime module (Send + Sync).
    #[unsafe_ignore_trace]
    pub(crate) module: wasmtime::Module,
    /// The source bytes from which the module was compiled.
    #[unsafe_ignore_trace]
    pub(crate) bytes: Vec<u8>,
}

impl JsData for WasmModule {}

impl WasmModule {
    #[allow(dead_code)]
    pub(crate) fn new(module: wasmtime::Module, bytes: Vec<u8>) -> Self {
        Self { module, bytes }
    }
}

/// <https://www.w3.org/TR/wasm-js-api/#instance-objects>
///
/// A WebAssembly instance, stored as data on a JS Instance object.
#[derive(Trace, Finalize)]
#[allow(dead_code)]
pub(crate) struct WasmInstance {
    /// The exports object created from the instance's exports.
    pub(crate) exports: JsObject,
}

impl JsData for WasmInstance {}

impl WasmInstance {
    #[allow(dead_code)]
    pub(crate) fn new(exports: JsObject) -> Self {
        Self { exports }
    }
}

/// <https://www.w3.org/TR/wasm-js-api/#memory-objects>
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmMemory {
    #[unsafe_ignore_trace]
    pub(crate) memory: wasmtime::Memory,
    pub(crate) buffer_object: Option<JsObject>,
}

impl JsData for WasmMemory {}

/// <https://www.w3.org/TR/wasm-js-api/#table-objects>
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmTable {
    #[unsafe_ignore_trace]
    pub(crate) table: wasmtime::Table,
}

impl JsData for WasmTable {}

/// <https://www.w3.org/TR/wasm-js-api/#global-objects>
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmGlobal {
    #[unsafe_ignore_trace]
    pub(crate) global: wasmtime::Global,
}

impl JsData for WasmGlobal {}

/// <https://www.w3.org/TR/wasm-js-api/#tag-section>
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmTag {
    #[unsafe_ignore_trace]
    pub(crate) tag: wasmtime::Tag,
}

impl JsData for WasmTag {}
