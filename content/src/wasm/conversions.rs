//! <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
//!
//! Value conversion between WebAssembly values and JavaScript values,
//! as defined in the WebAssembly Core Embedding specification.

use boa_engine::{Context, JsNativeError, JsResult, JsValue};

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
pub(crate) fn js_val_to_wasm_val(
    value: &JsValue,
    wasm_type: &wasmtime::ValType,
    context: &mut Context,
) -> Result<wasmtime::Val, JsNativeError> {
    match wasm_type {
        wasmtime::ValType::I32 => {
            let n = value
                .to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for i32"))?;
            Ok(wasmtime::Val::I32(n as i32))
        }
        wasmtime::ValType::I64 => {
            Err(JsNativeError::typ().with_message("i64 wasm values not yet supported"))
        }
        wasmtime::ValType::F32 => {
            let n = value
                .to_number(context)
                .map_err(|_| JsNativeError::typ().with_message("expected number for f32"))?;
            Ok(wasmtime::Val::F32(n as u32))
        }
        wasmtime::ValType::F64 => {
            let n = value
                .to_number(context)
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

/// Create a default `wasmtime::Val` for a given `ValType`, used to initialize
/// result buffers before calling an exported wasm function.
pub(crate) fn default_val_for_type(val_type: &wasmtime::ValType) -> wasmtime::Val {
    match val_type {
        wasmtime::ValType::I32 => wasmtime::Val::I32(0),
        wasmtime::ValType::I64 => wasmtime::Val::I64(0),
        wasmtime::ValType::F32 => wasmtime::Val::F32(0),
        wasmtime::ValType::F64 => wasmtime::Val::F64(0),
        _ => wasmtime::Val::I32(0),
    }
}
