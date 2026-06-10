//! <https://webassembly.github.io/spec/js-api/>
//!
//! Pure domain logic — operates on Rust/wasmtime types only.
//! JS-bridge code (WebIdlInterface impls, promise helpers, JS-object creation)
//! lives in `content/src/js/bindings/wasm/`.

use boa_engine::{Context, JsNativeError, JsResult, JsValue};
use wasmtime::Module;

use crate::wasm::types::WasmModule;

/// <https://webassembly.github.io/spec/js-api/#dom-webassembly-validate>
pub(crate) fn validate_wasm_module(stable_bytes: &[u8]) -> bool {
    // Step 2: "Compile stableBytes as a WebAssembly module and store the results as module."
    // Step 3: "If module is error, return false."
    // Note: Steps 4-6 (validating builtins and imported strings) are not yet implemented.
    let engine = wasmtime::Engine::default();
    matches!(Module::new(&engine, stable_bytes), Ok(_))
}

impl WasmModule {
    /// <https://webassembly.github.io/spec/js-api/#dom-module-exports>
    pub(crate) fn export_descriptors(&self) -> Vec<(String, &'static str)> {
        // Step 1: "Let module be moduleObject.[[Module]]."
        // Step 2: "Let exports be « »."
        // Step 3: "For each (name, type) of module_exports(module),"
        // Note: Steps 3.2-3.3 (building JsArray entries and appending) are done
        // by the JS bindings glue — this domain method returns the raw Vec.
        self.module.exports()
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

/// Return `(name, externval)` pairs for all exports of an instantiated module.
pub(crate) fn instance_export_list(
    module: &wasmtime::Module,
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<()>,
) -> Vec<(String, wasmtime::Extern)> {
    module.exports()
        .filter_map(|export| {
            let name = export.name();
            instance.get_export(&mut *store, name).map(|val| (name.to_string(), val))
        })
        .collect()
}

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
pub(crate) fn js_val_to_wasm_val(
    value: &JsValue,
    wasm_type: &wasmtime::ValType,
    context: &mut Context,
) -> Result<wasmtime::Val, JsNativeError> {
    match wasm_type {
        wasmtime::ValType::I32 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for i32"))?;
            Ok(wasmtime::Val::I32(n as i32))
        }
        wasmtime::ValType::I64 => {
            Err(JsNativeError::typ().with_message("i64 wasm values not yet supported"))
        }
        wasmtime::ValType::F32 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for f32"))?;
            Ok(wasmtime::Val::F32(n as u32))
        }
        wasmtime::ValType::F64 => {
            let n = value.to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for f64"))?;
            Ok(wasmtime::Val::F64(n.to_bits()))
        }
        _ => Err(JsNativeError::typ().with_message("unsupported wasm value type")),
    }
}

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
pub(crate) fn wasm_val_to_js_value(
    val: &wasmtime::Val,
    _context: &mut Context,
) -> JsResult<JsValue> {
    match val {
        wasmtime::Val::I32(n) => Ok(JsValue::from(*n)),
        wasmtime::Val::I64(_) => Err(JsNativeError::typ()
            .with_message("i64 wasm values not yet supported")
            .into()),
        wasmtime::Val::F32(n) => Ok(JsValue::from(f32::from_bits(*n) as f64)),
        wasmtime::Val::F64(n) => Ok(JsValue::from(f64::from_bits(*n))),
        _ => Err(JsNativeError::typ()
            .with_message("unsupported wasm result type")
            .into()),
    }
}
