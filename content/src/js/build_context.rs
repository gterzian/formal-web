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
fn build_context_inner(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    use js_engine::ExecutionContext as _;
    use js_engine::JsEngine as _;
    use js_engine::JsTypes as _;
    use js_engine::jsc::JscEngine;

    use crate::dom::{
        AbortController, AbortSignal, DOMException, Document, Element, Event, EventTarget, Node,
        UIEvent,
    };
    use crate::html::{
        GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
        HTMLMediaElement, HTMLVideoElement, Location, Window,
    };
    use crate::streams::{
        ByteLengthQueuingStrategy, CountQueuingStrategy, ReadableByteStreamController,
        ReadableStream, ReadableStreamBYOBReader, ReadableStreamBYOBRequest,
        ReadableStreamDefaultController, ReadableStreamDefaultReader, TransformStream,
        TransformStreamDefaultController, WritableStream, WritableStreamDefaultController,
        WritableStreamDefaultWriter,
    };
    use crate::webidl::bindings::{
        get_registry_prototype, initialize_registry, register_interface_spec,
        wire_registry_prototype,
    };

    let mut engine = JscEngine::new();

    // Step 1: Create the Window with GlobalScope and associate it with the
    // realm's global object so `global_scope_or_error` works.
    let global_scope = GlobalScope::new(crate::html::GlobalScopeKind::Window, Rc::clone(&document));
    let window = Window::new(global_scope);
    let global_obj = engine.realm_global_object();
    engine.associate_existing_object(&global_obj, Box::new(window));

    // Step 2: Store the global object in host_any.
    crate::js::platform_objects::init_global_object_slot(&mut engine, global_obj);

    // Step 3: Initialize the interface registry.
    initialize_registry::<crate::js::Types>(&mut engine);

    // Step 4: Install console namespace.
    crate::js::install_console_namespace(&mut engine)
        .map_err(|error| format!("failed to install console: {:?}", error))?;

    // Step 5: Register all interface specs.
    macro_rules! reg {
        ($ty:ty) => {
            register_interface_spec::<crate::js::Types, $ty, _>(&mut engine).map_err(|error| {
                format!(
                    "failed to register {}: {:?}",
                    stringify!($ty),
                    error.display()
                )
            })?;
        };
    }

    reg!(EventTarget);
    reg!(DOMException);
    reg!(Event);
    reg!(UIEvent);
    reg!(AbortSignal);
    reg!(AbortController);
    reg!(Node);
    reg!(Document);
    reg!(Element);
    reg!(HTMLElement);
    reg!(HTMLAnchorElement);
    reg!(HTMLIFrameElement);
    reg!(HTMLInputElement);
    reg!(HTMLMediaElement);
    reg!(HTMLVideoElement);
    reg!(Window);
    reg!(Location);
    reg!(ByteLengthQueuingStrategy);
    reg!(CountQueuingStrategy);
    reg!(ReadableStream);
    reg!(ReadableStreamDefaultController);
    reg!(ReadableByteStreamController);
    reg!(ReadableStreamDefaultReader);
    reg!(ReadableStreamBYOBReader);
    reg!(ReadableStreamBYOBRequest);
    reg!(WritableStream);
    reg!(WritableStreamDefaultController);
    reg!(WritableStreamDefaultWriter);
    reg!(TransformStream);
    reg!(TransformStreamDefaultController);

    // Step 6: Wire prototype chains.
    wire_registry_prototype::<crate::js::Types, UIEvent, Event>(&mut engine);
    wire_registry_prototype::<crate::js::Types, AbortSignal, EventTarget>(&mut engine);
    wire_registry_prototype::<crate::js::Types, Node, EventTarget>(&mut engine);
    wire_registry_prototype::<crate::js::Types, Document, Node>(&mut engine);
    wire_registry_prototype::<crate::js::Types, Element, Node>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLElement, Element>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLAnchorElement, HTMLElement>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLIFrameElement, HTMLElement>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLMediaElement, HTMLElement>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLVideoElement, HTMLMediaElement>(&mut engine);
    wire_registry_prototype::<crate::js::Types, HTMLInputElement, HTMLElement>(&mut engine);
    wire_registry_prototype::<crate::js::Types, Window, EventTarget>(&mut engine);

    // Step 7: Set the global object's prototype to Window.prototype so
    // `instanceof Window` etc. works.
    if let Some(window_proto) = get_registry_prototype::<crate::js::Types, Window>(&engine) {
        let _ = engine.set_prototype(global_obj, Some(window_proto));
    }

    // Step 8: Install CSS namespace.
    crate::js::install_css_namespace(&mut engine)
        .map_err(|error| format!("failed to install CSS namespace: {:?}", error))?;

    // Step 9: HTMLAnchorElement: HTMLHyperlinkElementUtils members.
    if let Some(anchor_proto) =
        get_registry_prototype::<crate::js::Types, HTMLAnchorElement>(&engine)
    {
        crate::js::bindings::html::hyperlink_element_utils::
            register_hyperlink_element_utils_on_prototype(
                &anchor_proto, &mut engine,
            )
            .map_err(|error| error.display().to_string())?;
    }

    Ok(engine)
}
