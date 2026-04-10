mod bindings;
mod html_parser;
pub(crate) mod platform_objects;
pub(crate) use bindings::{install_console_namespace, install_document_property};
pub use html_parser::{JsHtmlParserProvider, execute_parser_scripts, parse_html_into_document};
