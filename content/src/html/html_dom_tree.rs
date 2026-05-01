use crate::ContentRuntime;

use super::html_iframe_element::{
    run_iframe_post_connection_steps_for_document, run_iframe_removing_steps_for_document,
};

/// <https://html.spec.whatwg.org/#dom-trees>
pub(crate) fn run_dom_post_connection_steps_for_document(
    runtime: &mut ContentRuntime,
    document_id: u64,
) -> Result<(), String> {
    run_iframe_post_connection_steps_for_document(runtime, document_id)
}

/// <https://dom.spec.whatwg.org/#concept-node-remove>
pub(crate) fn run_dom_removing_steps_for_document(
    runtime: &mut ContentRuntime,
    document_id: u64,
) -> Result<(), String> {
    run_iframe_removing_steps_for_document(runtime, document_id)
}
