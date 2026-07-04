pub(crate) mod dom;
pub(crate) mod html;
pub(crate) mod streams;
#[cfg(boa_backend)]
pub(crate) mod wasm;

pub(crate) use dom::install_document_property;
#[cfg(boa_backend)]
pub(crate) use wasm::install_wasm_namespace;
