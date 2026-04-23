mod console;
pub(crate) mod dom;
pub(crate) mod html;
pub(crate) mod streams;

pub(crate) use console::install_console_namespace;
pub(crate) use dom::install_document_property;
