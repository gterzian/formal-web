use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;

use crate::js::Engine;

/// Build a JavaScript engine context with all native bindings installed.
///
/// On the Boa backend, creates a `BoaContext` with full Web IDL bindings
/// (DOM, HTML, Streams, WebAssembly).  On the JSC backend, creates a
/// `JscEngine` with only the generic console namespace.
///
/// Returns a type implementing both [`js_engine::JsEngine<crate::js::Types>`] and
/// [`js_engine::ExecutionContext<crate::js::Types>`].
pub(crate) fn build_context(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    build_context_inner(document)
}

#[cfg(boa_backend)]
fn build_context_inner(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    crate::js::bindings::html::build_context(document)
}

#[cfg(not(boa_backend))]
fn build_context_inner(_document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    use js_engine::jsc::JscEngine;

    let mut engine = JscEngine::new();
    crate::js::install_console_namespace(&mut engine)
        .map_err(|error| format!("failed to install console: {:?}", error))?;
    crate::js::install_css_namespace(&mut engine)
        .map_err(|error| format!("failed to install CSS namespace: {:?}", error))?;
    Ok(engine)
}
