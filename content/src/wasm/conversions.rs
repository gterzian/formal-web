//! <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
//!
//! Value conversion between WebAssembly values and JavaScript values,
//! as defined in the WebAssembly Core Embedding specification.

use boa_engine::JsValue;
use js_engine::boa::BoaTypes;
use js_engine::{Completion, ExecutionContext};

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
pub(crate) fn js_val_to_wasm_val(
    value: &JsValue,
    wasm_type: &wasmtime::ValType,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<wasmtime::Val, BoaTypes> {
    match wasm_type {
        wasmtime::ValType::I32 => {
            let n = ec.to_number(value.clone())?;
            Ok(wasmtime::Val::I32(n as i32))
        }
        wasmtime::ValType::I64 => Err(ec.new_type_error("i64 wasm values not yet supported")),
        wasmtime::ValType::F32 => {
            let n = ec.to_number(value.clone())?;
            Ok(wasmtime::Val::F32(n as u32))
        }
        wasmtime::ValType::F64 => {
            let n = ec.to_number(value.clone())?;
            Ok(wasmtime::Val::F64(n.to_bits()))
        }
        _ => Err(ec.new_type_error("unsupported wasm value type")),
    }
}

/// <https://webassembly.github.io/spec/core/appendix/embedding.html#embed-func-type>
pub(crate) fn wasm_val_to_js_value(
    val: &wasmtime::Val,
    ec: &mut dyn ExecutionContext<BoaTypes>,
) -> Completion<JsValue, BoaTypes> {
    match val {
        wasmtime::Val::I32(n) => Ok(JsValue::from(*n)),
        wasmtime::Val::I64(_) => Err(ec.new_type_error("i64 wasm values not yet supported")),
        wasmtime::Val::F32(n) => Ok(JsValue::from(f32::from_bits(*n) as f64)),
        wasmtime::Val::F64(n) => Ok(JsValue::from(f64::from_bits(*n))),
        _ => Err(ec.new_type_error("unsupported wasm result type")),
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
