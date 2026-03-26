use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use std::ffi::c_char;
use std::panic::{self, AssertUnwindSafe};

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lean_mk_string_from_bytes(value: *const c_char, size: usize) -> *mut lean_object;
}

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";
const LOADED_HTML_DOCUMENT: &str =
    "<html><head><title>Loaded</title></head><body><p>Loaded!</p></body></html>";

fn create_html_document_pointer(html: &str) -> usize {
    let document = HtmlDocument::from_html(html, DocumentConfig::default());
    Box::into_raw(Box::new(document)) as usize
}

fn lean_string_from_owned(value: String) -> *mut lean_object {
    unsafe { lean_mk_string_from_bytes(value.as_ptr() as *const c_char, value.len()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_empty_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(EMPTY_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_loaded_html_document(_: *mut lean_object) -> usize {
    panic::catch_unwind(AssertUnwindSafe(|| create_html_document_pointer(LOADED_HTML_DOCUMENT)))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_render_html_document(pointer: usize) -> *mut lean_object {
    let html = panic::catch_unwind(AssertUnwindSafe(|| {
        if pointer == 0 {
            String::from("<null rust document pointer>")
        } else {
            let document = unsafe { &*(pointer as *const HtmlDocument) };
            document.root_element().outer_html()
        }
    }))
    .unwrap_or_else(|_| String::from("<panic rendering rust document>"));

    lean_string_from_owned(html)
}