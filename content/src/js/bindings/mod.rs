pub(crate) mod dom;
pub(crate) mod html;
pub(crate) mod streams;
pub(crate) mod wasm;

pub(crate) use dom::install_document_property;
pub(crate) use wasm::install_wasm_namespace;
