use std::sync::{Arc, Mutex};

use boa_engine::{JsObject, object::JsData};
use boa_gc::{Finalize, Trace};
use wasmtime::Store;

/// <https://www.w3.org/TR/wasm-js-api/#module-objects>
///
/// A compiled WebAssembly module, stored as data on a JS Module object.
/// The JS-visible properties (exports, imports, customSections) are defined
/// as static methods on the Module constructor when the namespace is set up.
#[derive(Trace, Finalize)]
pub(crate) struct WasmModule {
    /// The compiled wasmtime module (Send + Sync).
    #[unsafe_ignore_trace]
    pub(crate) module: wasmtime::Module,
    /// The source bytes from which the module was compiled.
    /// TODO: bytes is the [[Bytes]] internal slot (spec §4.2). Currently stored
    /// during construction but never read back — needed once customSections is
    /// implemented (spec §4.2.8).
    #[unsafe_ignore_trace]
    #[allow(dead_code)]
    pub(crate) bytes: Vec<u8>,
}

impl JsData for WasmModule {}

impl WasmModule {
    pub(crate) fn new(module: wasmtime::Module, bytes: Vec<u8>) -> Self {
        Self { module, bytes }
    }

    /// <https://webassembly.github.io/spec/js-api/#dom-module-exports>
    pub(crate) fn export_descriptors(&self) -> Vec<(String, &'static str)> {
        // Step 1: "Let module be moduleObject.[[Module]]."
        // Step 2: "Let exports be « »."
        // Step 3: "For each (name, type) of module_exports(module),"
        // Note: Steps 3.2-3.3 (building JsArray entries and appending) are done
        // by the JS bindings glue — this domain method returns the raw Vec.
        self.module
            .exports()
            .map(|export| {
                let kind = match export.ty() {
                    wasmtime::ExternType::Func(_) => "function",
                    wasmtime::ExternType::Table(_) => "table",
                    wasmtime::ExternType::Memory(_) => "memory",
                    wasmtime::ExternType::Global(_) => "global",
                    wasmtime::ExternType::Tag(_) => "tag",
                };
                // Step 3.1: "Let kind be the string value of the extern type type."
                (export.name().to_string(), kind)
            })
            .collect()
        // Step 4: "Return exports."
        // Note: The binding wraps this Vec in a JsArray before returning to JS.
    }
}

/// <https://www.w3.org/TR/wasm-js-api/#instance-objects>
///
/// A WebAssembly instance, stored as data on a JS Instance object.
///
/// The `store` field holds the wasmtime store that the instance was created
/// from, wrapped in `Arc<Mutex<...>>` so that exported-function closures
/// on the main thread and the background worker can safely access it.
///
/// Note: The `wasmtime::Instance` field is redundant with `store.data()`
/// but kept for ergonomic access to the handle.
#[derive(Trace, Finalize)]
pub(crate) struct WasmInstance {
    /// The exports object created from the instance's exports.
    pub(crate) exports: JsObject,
    /// Shared (main + worker), mutex-protected store.
    /// TODO: store is needed by exported-function closures that access
    /// the wasmtime instance after creation. The field on the struct is
    /// never dereferenced directly — only the Arc is cloned into closures.
    #[unsafe_ignore_trace]
    #[allow(dead_code)]
    pub(crate) store: Arc<Mutex<Store<()>>>,
    /// The wasmtime instance handle.
    /// TODO: instance is kept for ergonomic access when the content process
    /// needs the handle (e.g. get_export). Currently unused after construction.
    #[unsafe_ignore_trace]
    #[allow(dead_code)]
    pub(crate) instance: wasmtime::Instance,
}

impl JsData for WasmInstance {}

impl WasmInstance {
    pub(crate) fn new(
        exports: JsObject,
        store: Arc<Mutex<Store<()>>>,
        instance: wasmtime::Instance,
    ) -> Self {
        Self {
            exports,
            store,
            instance,
        }
    }
}

/// <https://www.w3.org/TR/wasm-js-api/#memory-objects>
/// TODO: WebAssembly.Memory Web IDL interface not yet exposed. Struct is
/// defined so the type is ready once the JS bindings glue is implemented.
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmMemory {
    #[unsafe_ignore_trace]
    pub(crate) memory: wasmtime::Memory,
    pub(crate) buffer_object: Option<JsObject>,
}

impl JsData for WasmMemory {}

/// <https://www.w3.org/TR/wasm-js-api/#table-objects>
/// TODO: WebAssembly.Table Web IDL interface not yet exposed.
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmTable {
    #[unsafe_ignore_trace]
    pub(crate) table: wasmtime::Table,
}

impl JsData for WasmTable {}

/// <https://www.w3.org/TR/wasm-js-api/#global-objects>
/// TODO: WebAssembly.Global Web IDL interface not yet exposed.
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmGlobal {
    #[unsafe_ignore_trace]
    pub(crate) global: wasmtime::Global,
}

impl JsData for WasmGlobal {}

/// <https://www.w3.org/TR/wasm-js-api/#tag-section>
/// TODO: WebAssembly.Tag Web IDL interface not yet exposed.
#[allow(dead_code)]
#[derive(Trace, Finalize)]
pub(crate) struct WasmTag {
    #[unsafe_ignore_trace]
    pub(crate) tag: wasmtime::Tag,
}

impl JsData for WasmTag {}
