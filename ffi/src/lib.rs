use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use std::panic::{self, AssertUnwindSafe};

#[repr(C)]
pub struct lean_object {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn lean_box_usize(value: usize) -> *mut lean_object;
}

const EMPTY_HTML_DOCUMENT: &str = "<html><head></head><body></body></html>";

#[unsafe(no_mangle)]
pub extern "C" fn formal_web_create_empty_html_document() -> *mut lean_object {
    let pointer = panic::catch_unwind(AssertUnwindSafe(|| {
        let document = HtmlDocument::from_html(EMPTY_HTML_DOCUMENT, DocumentConfig::default());
        Box::into_raw(Box::new(document)) as usize
    }))
    .unwrap_or(0);

    unsafe { lean_box_usize(pointer) }
}