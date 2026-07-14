pub(crate) mod dom;
pub(crate) mod html;
pub(crate) mod streams;
pub(crate) mod testutils;
#[cfg(all(boa_backend, feature = "wasm"))]
pub(crate) mod wasm;

pub(crate) use dom::install_document_property;
#[cfg(all(boa_backend, feature = "wasm"))]
pub(crate) use wasm::install_wasm_namespace;
