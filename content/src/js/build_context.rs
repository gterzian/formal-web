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

/// Create a new realm within an existing engine, sharing the same underlying
/// JS context (same GC heap) but with its own global object, Window, Document,
/// and prototype chain.
///
/// On JSC, uses `JscEngine::new_shared_realm()` to create a child engine
/// sharing the same `JSGlobalContextRef`.  On Boa, uses `create_realm()`.
pub(crate) fn build_realm(
    engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    build_realm_inner(engine, document)
}

#[cfg(boa_backend)]
fn build_context_inner(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    crate::js::bindings::html::build_context(document)
}

#[cfg(not(boa_backend))]
fn build_context_inner(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    use js_engine::jsc::JscEngine;
    let mut engine = JscEngine::new();
    setup_realm(&mut engine, document)?;
    Ok(engine)
}

#[cfg(not(boa_backend))]
fn build_realm_inner(
    engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    let mut child = engine.new_shared_realm();
    setup_realm(&mut child, document)?;
    Ok(child)
}

/// Shared setup for both fresh engines and child realms (JSC backend).
/// Initializes the global object, Window, Document, prototypes, etc.
#[cfg(not(boa_backend))]
fn setup_realm(engine: &mut Engine, document: Rc<RefCell<BaseDocument>>) -> Result<(), String> {
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
    use js_engine::ExecutionContext as _;

    // Step 1: Create the Window with GlobalScope and associate it with the
    // realm's global object so `global_scope_or_error` works.
    let global_scope = GlobalScope::new(crate::html::GlobalScopeKind::Window, Rc::clone(&document));
    let mut window = Window::new(global_scope);
    // Store the engine context so `create_document_in_realm` can create shared
    // realms for `window.open` (same GC heap on JSC).
    window
        .global_scope
        .set_engine_context(Box::new(engine.context().clone()));
    let global_obj = engine.realm_global_object();
    engine.associate_existing_object(&global_obj, Box::new(window));

    // Step 2: Store the global object in host_any.
    crate::js::platform_objects::init_global_object_slot(engine, global_obj);

    // Step 3: Initialize the interface registry.
    initialize_registry::<crate::js::Types>(engine);

    // Step 4: Install console namespace.
    crate::js::install_console_namespace(engine)
        .map_err(|error| format!("failed to install console: {:?}", error))?;

    // Step 5: Register all interface specs.
    macro_rules! reg {
        ($ty:ty) => {
            register_interface_spec::<crate::js::Types, $ty, _>(engine).map_err(|error| {
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
    wire_registry_prototype::<crate::js::Types, UIEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, AbortSignal, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Node, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Document, Node>(engine);
    wire_registry_prototype::<crate::js::Types, Element, Node>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLElement, Element>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLAnchorElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLIFrameElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLMediaElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLVideoElement, HTMLMediaElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLInputElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, Window, EventTarget>(engine);

    // Step 7: Set the global object's prototype to Window.prototype so
    // `instanceof Window` etc. works.
    if let Some(window_proto) = get_registry_prototype::<crate::js::Types, Window>(engine) {
        let _ = engine.set_prototype(global_obj, Some(window_proto));

        // Step 7b: JSC's global object prototype is immutable.
        // Copy Window/EventTarget properties to the global object.
        let prototypes = [
            get_registry_prototype::<crate::js::Types, EventTarget>(engine),
            Some(window_proto),
        ];
        for proto in prototypes.iter().flatten() {
            if let Ok(keys) = engine.own_property_keys(*proto) {
                for key in keys {
                    let key_str = engine.property_key_to_rust_string(&key);
                    if key_str == "constructor" || key_str == "__proto__" {
                        continue;
                    }
                    if let Ok(Some(descriptor)) = engine.get_own_property(*proto, key.clone()) {
                        if descriptor.value.is_some() || descriptor.get.is_some() {
                            let _ = engine.define_property_or_throw(global_obj, key, descriptor);
                        }
                    }
                }
            }
        }
    }

    // Step 8: Install CSS namespace.
    crate::js::install_css_namespace(engine)
        .map_err(|error| format!("failed to install CSS namespace: {:?}", error))?;

    // Step 9: HTMLAnchorElement: HTMLHyperlinkElementUtils members.
    if let Some(anchor_proto) =
        get_registry_prototype::<crate::js::Types, HTMLAnchorElement>(engine)
    {
        crate::js::bindings::html::hyperlink_element_utils::
            register_hyperlink_element_utils_on_prototype(
                &anchor_proto, engine,
            )
            .map_err(|error| error.display().to_string())?;
    }

    // Step 10: ReadableStream methods: values, @@asyncIterator, pipeTo.
    // These are registered in host_hooks.rs for Boa; here for JSC.
    if let Some(rs_proto) = get_registry_prototype::<crate::js::Types, ReadableStream>(engine) {
        let values_fn: <crate::js::Types as js_engine::JsTypes>::JsObject = engine
            .create_builtin_fn(
                Box::new(|args, this, ec| {
                    crate::js::bindings::streams::readablestream::values_method(&this, args, ec)
                }),
                0,
                engine.property_key_from_str("values"),
            )
            .into();

        let pipe_to_native_fn: <crate::js::Types as js_engine::JsTypes>::JsObject = engine
            .create_builtin_fn(
                Box::new(|args, this, ec| {
                    crate::js::bindings::streams::readablestream::pipe_to_native_method(
                        &this, args, ec,
                    )
                }),
                2,
                engine.property_key_from_str("pipeTo"),
            )
            .into();

        // values descriptor
        let values_value =
            <crate::js::Types as js_engine::JsTypes>::value_from_object(values_fn.clone());
        let values_desc = js_engine::records::PropertyDescriptor::<crate::js::Types> {
            value: Some(values_value),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            get: None,
            set: None,
        };
        let _ = engine.define_property_or_throw(
            rs_proto,
            engine.property_key_from_str("values"),
            values_desc,
        );

        // @@asyncIterator: same function as values (per spec
        // ReadableStream.prototype[@@asyncIterator] = ReadableStream.prototype.values)
        let async_iter_key = engine.property_key_from_well_known_symbol("asyncIterator");
        let async_iter_desc = js_engine::records::PropertyDescriptor::<crate::js::Types> {
            value: Some(values_value.clone()),
            writable: Some(true),
            configurable: Some(true),
            enumerable: None,
            get: None,
            set: None,
        };
        let _ = engine.define_property_or_throw(rs_proto, async_iter_key, async_iter_desc);

        // __formalWebReadableStreamPipeToNative (native backstop)
        let native_value =
            <crate::js::Types as js_engine::JsTypes>::value_from_object(pipe_to_native_fn.clone());
        let native_desc = js_engine::records::PropertyDescriptor::<crate::js::Types> {
            value: Some(native_value),
            writable: Some(true),
            configurable: Some(true),
            enumerable: None,
            get: None,
            set: None,
        };
        let _ = engine.define_property_or_throw(
            rs_proto,
            engine.property_key_from_str("__formalWebReadableStreamPipeToNative"),
            native_desc,
        );

        // pipeTo: JS wrapper that calls the native backstop.
        let wrapper_source = "(function pipeTo(dest, opts) { return this.__formalWebReadableStreamPipeToNative(dest, opts); })";
        if let Ok(wrapper_val) = engine.evaluate_script(wrapper_source) {
            if let Some(wrapper_obj) =
                <crate::js::Types as js_engine::JsTypes>::value_as_object(&wrapper_val)
            {
                let pipe_value =
                    <crate::js::Types as js_engine::JsTypes>::value_from_object(wrapper_obj);
                let pipe_to_desc = js_engine::records::PropertyDescriptor::<crate::js::Types> {
                    value: Some(pipe_value),
                    writable: Some(true),
                    configurable: Some(true),
                    enumerable: None,
                    get: None,
                    set: None,
                };
                let _ = engine.define_property_or_throw(
                    rs_proto,
                    engine.property_key_from_str("pipeTo"),
                    pipe_to_desc,
                );
            }
        }
    }

    Ok(())
}

#[cfg(boa_backend)]
fn build_realm_inner(
    _engine: &mut Engine,
    _document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    // Boa: create a new realm within the existing context.
    // Currently falls back to full build_context since Boa's
    // multi-realm support needs the host_hooks path.
    crate::js::bindings::html::build_context(_document)
}
