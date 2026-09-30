use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use log::error;

use crate::js::Engine;

/// Build a JavaScript engine context with all native bindings installed.
///
/// Creates the selected engine with the generic Web IDL bindings installed.
///
/// Returns a type implementing both [`js_engine::JsEngine<crate::js::Types>`] and
/// [`js_engine::ExecutionContext<crate::js::Types>`].
pub(crate) fn build_context(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    build_context_inner(document)
}

/// Create a new realm associated with an existing engine.
///
/// V8 shares its isolate. Boa and JSC currently create a fresh engine.
pub(crate) fn build_realm(
    engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    build_realm_inner(engine, document)
}

/// Build a fresh JavaScript engine for a dedicated worker agent's realm,
/// whose global object is a worker global scope.
///
/// A dedicated worker runs in its own agent, so its realm is always built
/// in a fresh engine (its own JS heap).
pub(crate) fn build_worker_realm(
    document: Rc<RefCell<BaseDocument>>,
    name: String,
    worker_type: crate::html::WorkerType,
) -> Result<Engine, String> {
    // The Boa backend builds the realm's global object through its host hooks
    // and needs a factory that constructs the DedicatedWorkerGlobalScope
    // platform object once the execution context exists. JSC/V8 create it in
    // `setup_worker_realm` and associate it with the global object there.
    #[cfg(boa_backend)]
    let mut engine = {
        use crate::html::{DedicatedWorkerGlobalScope, GlobalScope};

        let factory_name = name.clone();
        let document = Rc::clone(&document);
        let factory = move |ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>| {
            let global_scope = GlobalScope::new(
                crate::html::GlobalScopeKind::Worker,
                Rc::clone(&document),
                ec,
            );
            DedicatedWorkerGlobalScope::new(global_scope, factory_name.clone(), worker_type, ec)
        };
        js_engine::create_engine(factory)?
    };
    #[cfg(not(boa_backend))]
    let mut engine = js_engine::create_engine()?;
    // Create the worker global object (JSC/V8) and run the realm bootstrap
    // on every backend: the interface registry, console, interface
    // registration, and the worker global scope's prototype wiring
    // (see `setup_worker_realm`).  On Boa the global object was already
    // built by the realm-creation host hooks from the factory above.
    setup_worker_realm(&mut engine, document, name, worker_type)?;
    Ok(engine)
}

fn build_context_inner(document: Rc<RefCell<BaseDocument>>) -> Result<Engine, String> {
    // The Boa backend builds the realm's global object through its host hooks
    // and needs a factory that constructs the Window platform object once the
    // execution context exists. JSC/V8 create the Window in `setup_realm` and
    // associate it with the global object there.
    #[cfg(boa_backend)]
    let mut engine = {
        use crate::html::{GlobalScope, Window};

        let document = Rc::clone(&document);
        let factory = move |ec: &mut dyn js_engine::ExecutionContext<crate::js::Types>| {
            let global_scope = GlobalScope::new(
                crate::html::GlobalScopeKind::Window,
                Rc::clone(&document),
                ec,
            );
            Window::new(global_scope, ec)
        };
        js_engine::create_engine(factory)?
    };
    #[cfg(not(boa_backend))]
    let mut engine = js_engine::create_engine()?;
    setup_realm(&mut engine, document)?;
    Ok(engine)
}

#[cfg(jsc_backend)]
fn build_realm_inner(
    _engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    build_context_inner(document)
}

#[cfg(v8_backend)]
fn build_realm_inner(
    engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    let mut child = engine.new_child_realm();
    setup_realm(&mut child, document)?;
    Ok(child)
}

#[cfg(boa_backend)]
fn build_realm_inner(
    _engine: &mut Engine,
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Engine, String> {
    // Boa: create a new realm within the existing context.
    // Currently falls back to a full build since Boa's multi-realm support
    // rebuilds the whole context.
    build_context_inner(document)
}

/// Shared setup for engines using the generic interface-registration path.
/// Initializes the global object, Window, Document, prototypes, etc.
fn setup_realm(engine: &mut Engine, _document: Rc<RefCell<BaseDocument>>) -> Result<(), String> {
    #[cfg(not(boa_backend))]
    let document = _document;
    use crate::dom::{
        AbortController, AbortSignal, Attr, DOMException, DOMImplementation, Document, Element,
        Event, EventTarget, NamedNodeMap, Node,
    };
    use crate::encoding::{TextDecoder, TextEncoder};
    use crate::fetch::{FetchApiRequest, Headers, Response};
    use crate::file_api::{Blob, File};
    #[cfg(not(boa_backend))]
    use crate::html::GlobalScope;
    use crate::html::{
        CanvasRenderingContext2D, HTMLAnchorElement, HTMLCanvasElement, HTMLElement,
        HTMLIFrameElement, HTMLInputElement, HTMLLinkElement, HTMLMediaElement, HTMLScriptElement,
        HTMLVideoElement, Location, MessageChannel, MessageEvent, MessagePort, Navigator,
        OffscreenCanvas, OffscreenCanvasRenderingContext2D, PromiseRejectionEvent, Window,
        WindowProxy, Worker,
    };
    use crate::streams::{
        ByteLengthQueuingStrategy, CountQueuingStrategy, ReadableByteStreamController,
        ReadableStream, ReadableStreamBYOBReader, ReadableStreamBYOBRequest,
        ReadableStreamDefaultController, ReadableStreamDefaultReader, TransformStream,
        TransformStreamDefaultController, WritableStream, WritableStreamDefaultController,
        WritableStreamDefaultWriter,
    };
    use crate::ui_events::{MouseEvent, UIEvent};
    use crate::url_standard::{URL, URLSearchParams};
    use crate::webidl::bindings::{
        get_registry_prototype, initialize_registry, register_interface_spec,
        wire_registry_constructor_prototype, wire_registry_prototype,
    };
    use js_engine::ExecutionContext as _;

    // Step 1: Create the Window with GlobalScope and associate it with the
    // realm's global object so `global_scope_or_error` works. The Boa backend
    // constructs the Window through its host hooks during realm creation, so
    // only JSC/V8 create it here.
    #[cfg(not(boa_backend))]
    let global_obj = {
        let global_scope = GlobalScope::new(
            crate::html::GlobalScopeKind::Window,
            Rc::clone(&document),
            engine,
        );
        let window = Window::new(global_scope, engine);
        let global_obj = engine.realm_global_object();
        js_engine::associate_existing_object(engine, &global_obj, window);
        global_obj
    };
    #[cfg(boa_backend)]
    let global_obj = engine.realm_global_object();
    // Set the EventTarget reflector for the Window.
    let global_value =
        <crate::js::Types as js_engine::JsTypes>::value_from_object(global_obj.clone());
    crate::js::try_set_event_target_reflector(&global_value, engine);

    // Step 2: Store the global object in host_any.
    crate::js::platform_objects::init_global_object_slot(engine, global_obj.clone());

    #[cfg(feature = "wasm")]
    if let Err(error) = crate::js::bindings::install_wasm_namespace(engine) {
        error!("[content] failed to install WebAssembly namespace: {error}");
    }

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
    reg!(DOMImplementation);
    reg!(Event);
    reg!(MessageEvent);
    reg!(PromiseRejectionEvent);
    reg!(MessageChannel);
    reg!(MessagePort);
    reg!(UIEvent);
    reg!(MouseEvent);
    reg!(AbortSignal);
    reg!(AbortController);
    reg!(Node);
    reg!(Document);
    reg!(Element);
    reg!(Attr);
    reg!(NamedNodeMap);
    reg!(HTMLElement);
    reg!(HTMLAnchorElement);
    reg!(HTMLScriptElement);
    reg!(HTMLLinkElement);
    reg!(HTMLCanvasElement);
    reg!(CanvasRenderingContext2D);
    reg!(HTMLIFrameElement);
    reg!(HTMLInputElement);
    reg!(HTMLMediaElement);
    reg!(HTMLVideoElement);
    reg!(OffscreenCanvas);
    reg!(OffscreenCanvasRenderingContext2D);
    reg!(Window);
    reg!(WindowProxy);
    reg!(Worker);
    reg!(Location);
    reg!(Navigator);
    reg!(URL);
    reg!(URLSearchParams);
    install_pair_iterator::<URLSearchParams>(engine)?;
    reg!(Headers);
    install_pair_iterator::<Headers>(engine)?;
    reg!(FetchApiRequest);
    reg!(Response);
    reg!(Blob);
    reg!(File);
    reg!(TextEncoder);
    reg!(TextDecoder);
    wire_registry_prototype::<crate::js::Types, File, Blob>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, File, Blob>(engine);
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
    // WebRTC and Media Capture and Streams: [Exposed=Window], built with the
    // `webrtc` feature.
    #[cfg(feature = "webrtc")]
    {
        use crate::mediacapture_streams::{
            MediaDeviceInfo, MediaDevices, MediaStream, MediaStreamTrack, MediaStreamTrackEvent,
        };
        use crate::webrtc::{
            RTCDataChannel, RTCDataChannelEvent, RTCIceCandidate, RTCPeerConnection,
            RTCPeerConnectionIceEvent, RTCRtpReceiver, RTCRtpSender, RTCRtpTransceiver,
            RTCSessionDescription, RTCStatsReport, RTCTrackEvent,
        };
        use crate::websockets::{CloseEvent, WebSocket};
        reg!(RTCPeerConnection);
        reg!(RTCDataChannel);
        reg!(RTCSessionDescription);
        reg!(RTCIceCandidate);
        reg!(RTCPeerConnectionIceEvent);
        reg!(RTCDataChannelEvent);
        reg!(WebSocket);
        reg!(CloseEvent);
        reg!(MediaStreamTrack);
        reg!(MediaStream);
        reg!(MediaDevices);
        reg!(MediaDeviceInfo);
        reg!(MediaStreamTrackEvent);
        reg!(RTCRtpSender);
        reg!(RTCRtpReceiver);
        reg!(RTCRtpTransceiver);
        reg!(RTCTrackEvent);
        reg!(RTCStatsReport);
        install_pair_iterator::<RTCStatsReport>(engine)?;
        wire_registry_prototype::<crate::js::Types, MediaStreamTrack, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, MediaStream, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, MediaDevices, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, MediaStreamTrackEvent, Event>(engine);
        wire_registry_prototype::<crate::js::Types, RTCTrackEvent, Event>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, MediaStreamTrack, EventTarget>(
            engine,
        );
        wire_registry_constructor_prototype::<crate::js::Types, MediaStream, EventTarget>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, MediaDevices, EventTarget>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, MediaStreamTrackEvent, Event>(
            engine,
        );
        wire_registry_constructor_prototype::<crate::js::Types, RTCTrackEvent, Event>(engine);
        wire_registry_prototype::<crate::js::Types, WebSocket, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, CloseEvent, Event>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, WebSocket, EventTarget>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, CloseEvent, Event>(engine);
        wire_registry_prototype::<crate::js::Types, RTCPeerConnection, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, RTCDataChannel, EventTarget>(engine);
        wire_registry_prototype::<crate::js::Types, RTCPeerConnectionIceEvent, Event>(engine);
        wire_registry_prototype::<crate::js::Types, RTCDataChannelEvent, Event>(engine);
        wire_registry_constructor_prototype::<crate::js::Types, RTCPeerConnection, EventTarget>(
            engine,
        );
        wire_registry_constructor_prototype::<crate::js::Types, RTCDataChannel, EventTarget>(
            engine,
        );
        wire_registry_constructor_prototype::<crate::js::Types, RTCPeerConnectionIceEvent, Event>(
            engine,
        );
        wire_registry_constructor_prototype::<crate::js::Types, RTCDataChannelEvent, Event>(engine);
    }

    // Step 6: Wire prototype chains.
    wire_registry_prototype::<crate::js::Types, UIEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, MessageEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, PromiseRejectionEvent, Event>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, PromiseRejectionEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, MessagePort, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, MouseEvent, UIEvent>(engine);
    wire_registry_prototype::<crate::js::Types, AbortSignal, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Node, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Document, Node>(engine);
    wire_registry_prototype::<crate::js::Types, Element, Node>(engine);
    wire_registry_prototype::<crate::js::Types, Attr, Node>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLElement, Element>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLAnchorElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLScriptElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLLinkElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLCanvasElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLIFrameElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLMediaElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLVideoElement, HTMLMediaElement>(engine);
    wire_registry_prototype::<crate::js::Types, HTMLInputElement, HTMLElement>(engine);
    wire_registry_prototype::<crate::js::Types, OffscreenCanvas, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Window, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Worker, EventTarget>(engine);

    // Step 6b: Wire constructor prototype chains so subclass constructors
    // inherit from their parent interface object (WebIDL "create an interface
    // object", step 3).
    wire_registry_constructor_prototype::<crate::js::Types, UIEvent, Event>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, MessagePort, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, MouseEvent, UIEvent>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, AbortSignal, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Node, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Document, Node>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Element, Node>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Attr, Node>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLElement, Element>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLCanvasElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, OffscreenCanvas, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLAnchorElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLScriptElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLLinkElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLIFrameElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLMediaElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLVideoElement, HTMLMediaElement>(
        engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, HTMLInputElement, HTMLElement>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Window, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Worker, EventTarget>(engine);

    // Step 6c: DOMException inherits from the realm's Error constructor.
    if let Some(de_proto) = get_registry_prototype::<crate::js::Types, DOMException>(engine) {
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        if let Err(error) = engine.set_prototype(de_proto, Some(intrinsics.error_prototype.clone()))
        {
            error!("failed to wire DOMException to Error.prototype: {error:?}");
        }
    }

    // Step 7: Set the global object's prototype to Window.prototype so
    // `instanceof Window` etc. works.
    if let Some(window_proto) = get_registry_prototype::<crate::js::Types, Window>(engine) {
        let proto_set = engine.set_prototype(global_obj.clone(), Some(window_proto.clone()));
        let immutable_global_proto = match proto_set {
            Ok(true) => false,
            Ok(false) | Err(_) => true,
        };

        // Step 7b: Engines with an immutable global object [[Prototype]]
        // (e.g. JSC) fall back to copying Window/EventTarget properties onto
        // the global object.
        if immutable_global_proto {
            let prototypes = [
                get_registry_prototype::<crate::js::Types, EventTarget>(engine),
                Some(window_proto),
            ];
            for proto in prototypes.iter().flatten() {
                if let Ok(keys) = engine.own_property_keys(proto.clone()) {
                    for key in keys {
                        let key_str = engine.property_key_to_rust_string(&key);
                        if key_str == "constructor" || key_str == "__proto__" {
                            continue;
                        }
                        if let Ok(Some(descriptor)) =
                            engine.get_own_property(proto.clone(), key.clone())
                            && (descriptor.value.is_some() || descriptor.get.is_some())
                            && let Err(error) =
                                engine.define_property_or_throw(global_obj.clone(), key, descriptor)
                        {
                            error!(
                                "failed to copy a Window prototype property to the global object: {error:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    // Step 8: Install CSS namespace.
    crate::js::install_css_namespace(engine)
        .map_err(|error| format!("failed to install CSS namespace: {:?}", error))?;

    // Step X: Install TestUtils namespace (gc() method).
    crate::js::bindings::testutils::install_testutils_namespace(engine)
        .map_err(|error| format!("failed to install TestUtils namespace: {:?}", error))?;

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
    if let Some(rs_proto) = get_registry_prototype::<crate::js::Types, ReadableStream>(engine) {
        let values_fn: <crate::js::Types as js_engine::JsTypes>::JsObject = engine
            .create_builtin_fn_static(
                |args, this, ec| {
                    crate::js::bindings::streams::readablestream::values_method(&this, args, ec)
                },
                0,
                engine.property_key_from_str("values"),
                false,
            )
            .into();

        let pipe_to_native_fn: <crate::js::Types as js_engine::JsTypes>::JsObject = engine
            .create_builtin_fn_static(
                |args, this, ec| {
                    crate::js::bindings::streams::readablestream::pipe_to_native_method(
                        &this, args, ec,
                    )
                },
                2,
                engine.property_key_from_str("pipeTo"),
                false,
            )
            .into();

        // values descriptor
        let values_value =
            <crate::js::Types as js_engine::JsTypes>::value_from_object(values_fn.clone());
        let values_desc = js_engine::records::PropertyDescriptor::<crate::js::Types> {
            value: Some(values_value.clone()),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            get: None,
            set: None,
        };
        engine
            .define_property_or_throw(
                rs_proto.clone(),
                engine.property_key_from_str("values"),
                values_desc,
            )
            .map_err(|error| format!("failed to install ReadableStream.values: {error:?}"))?;

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
        engine
            .define_property_or_throw(rs_proto.clone(), async_iter_key, async_iter_desc)
            .map_err(|error| {
                format!("failed to install ReadableStream async iterator: {error:?}")
            })?;

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
        engine
            .define_property_or_throw(
                rs_proto.clone(),
                engine.property_key_from_str("__formalWebReadableStreamPipeToNative"),
                native_desc,
            )
            .map_err(|error| {
                format!("failed to install ReadableStream pipeTo native function: {error:?}")
            })?;

        // pipeTo: JS wrapper that calls the native backstop.
        let wrapper_source = "(function pipeTo(dest, opts) { return this.__formalWebReadableStreamPipeToNative(dest, opts); })";
        if let Ok(wrapper_val) = engine.evaluate_script(wrapper_source)
            && let Some(wrapper_obj) =
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
            engine
                .define_property_or_throw(
                    rs_proto.clone(),
                    engine.property_key_from_str("pipeTo"),
                    pipe_to_desc,
                )
                .map_err(|error| format!("failed to install ReadableStream.pipeTo: {error:?}"))?;
        }
    }

    Ok(())
}

/// Finish building a dedicated worker agent's realm: create the realm's
/// global object — the DedicatedWorkerGlobalScope that run-a-worker step 5
/// creates for the global object, carrying the worker's name and type — and
/// run the realm bootstrap `setup_realm` runs for window realms.  The
/// run-a-worker step annotations for the realm and its worker environment
/// settings object live on
/// `EnvironmentSettingsObject::new_worker_in_realm`
/// (<https://html.spec.whatwg.org/#set-up-a-worker-environment-settings-object>),
/// which calls `build_worker_realm`.
///
/// The numbered phases in the body are this helper's own bootstrap order
/// (mirroring `setup_realm`), not steps of a spec algorithm.
fn setup_worker_realm(
    engine: &mut Engine,
    _document: Rc<RefCell<BaseDocument>>,
    _name: String,
    _worker_type: crate::html::WorkerType,
) -> Result<(), String> {
    use crate::dom::{
        AbortController, AbortSignal, DOMException, Document, Element, Event, EventTarget, Node,
    };
    use crate::encoding::{TextDecoder, TextEncoder};
    use crate::fetch::{FetchApiRequest, Headers, Response};
    use crate::file_api::{Blob, File};
    #[cfg(not(boa_backend))]
    use crate::html::GlobalScope;
    use crate::html::{
        DedicatedWorkerGlobalScope, MessageChannel, MessageEvent, MessagePort, OffscreenCanvas,
        OffscreenCanvasRenderingContext2D, PromiseRejectionEvent, Worker, WorkerGlobalScope,
        WorkerLocation, WorkerNavigator,
    };
    use crate::streams::{
        ByteLengthQueuingStrategy, CountQueuingStrategy, ReadableByteStreamController,
        ReadableStream, ReadableStreamBYOBReader, ReadableStreamBYOBRequest,
        ReadableStreamDefaultController, ReadableStreamDefaultReader, TransformStream,
        TransformStreamDefaultController, WritableStream, WritableStreamDefaultController,
        WritableStreamDefaultWriter,
    };
    use crate::url_standard::{URL, URLSearchParams};
    use crate::webidl::bindings::{
        get_registry_prototype, initialize_registry, register_interface_spec,
        wire_registry_constructor_prototype, wire_registry_prototype,
    };
    use js_engine::ExecutionContext as _;

    // Step 5 of run a worker: "For the global object, if is shared is true,
    // create a new SharedWorkerGlobalScope object. Otherwise, create a new
    // DedicatedWorkerGlobalScope object."  Step 6: "Let worker global scope
    // be the global object of realm execution context's Realm component."
    // The Boa backend constructs the platform object through its host hooks
    // during realm creation, so it only returns the realm's global object
    // here; JSC/V8 create the worker global scope and associate it with the
    // realm's global object.
    let global_obj = {
        #[cfg(not(boa_backend))]
        {
            let global_scope = GlobalScope::new(
                crate::html::GlobalScopeKind::Worker,
                Rc::clone(&_document),
                engine,
            );
            let dedicated_worker_global_scope =
                DedicatedWorkerGlobalScope::new(global_scope, _name, _worker_type, engine);
            let global_obj = engine.realm_global_object();
            js_engine::associate_existing_object(
                engine,
                &global_obj,
                dedicated_worker_global_scope,
            );
            global_obj
        }
        #[cfg(boa_backend)]
        engine.realm_global_object()
    };
    // Set the EventTarget reflector for the worker global scope.
    let global_value =
        <crate::js::Types as js_engine::JsTypes>::value_from_object(global_obj.clone());
    crate::js::try_set_event_target_reflector(&global_value, engine);

    // Step 2: Store the global object in host_any.
    crate::js::platform_objects::init_global_object_slot(engine, global_obj.clone());

    #[cfg(feature = "wasm")]
    if let Err(error) = crate::js::bindings::install_wasm_namespace(engine) {
        error!("[content] failed to install WebAssembly namespace: {error}");
    }

    // Step 3: Initialize the interface registry.
    initialize_registry::<crate::js::Types>(engine);

    // Step 4: Install console namespace.
    crate::js::install_console_namespace(engine)
        .map_err(|error| format!("failed to install console: {:?}", error))?;

    // Step 5: Register the interfaces a worker realm exposes: the base
    // interfaces, the DOM/node interfaces the environment settings object
    // still builds a document platform object for, and the worker
    // interfaces.  Window-only interfaces (Window, Location, HTML elements,
    // UI events) are not registered.
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
    reg!(MessageEvent);
    reg!(PromiseRejectionEvent);
    reg!(MessageChannel);
    reg!(MessagePort);
    reg!(AbortSignal);
    reg!(AbortController);
    reg!(Node);
    reg!(Document);
    reg!(Element);
    reg!(Worker);
    reg!(WorkerGlobalScope);
    reg!(DedicatedWorkerGlobalScope);
    reg!(WorkerLocation);
    reg!(WorkerNavigator);
    reg!(URL);
    reg!(URLSearchParams);
    install_pair_iterator::<URLSearchParams>(engine)?;
    reg!(Headers);
    install_pair_iterator::<Headers>(engine)?;
    reg!(FetchApiRequest);
    reg!(Response);
    reg!(Blob);
    reg!(File);
    reg!(TextEncoder);
    reg!(TextDecoder);
    wire_registry_prototype::<crate::js::Types, File, Blob>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, File, Blob>(engine);
    reg!(OffscreenCanvas);
    reg!(OffscreenCanvasRenderingContext2D);
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
    wire_registry_prototype::<crate::js::Types, MessageEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, PromiseRejectionEvent, Event>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, PromiseRejectionEvent, Event>(engine);
    wire_registry_prototype::<crate::js::Types, MessagePort, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, AbortSignal, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Node, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Document, Node>(engine);
    wire_registry_prototype::<crate::js::Types, Element, Node>(engine);
    wire_registry_prototype::<crate::js::Types, OffscreenCanvas, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, Worker, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, WorkerGlobalScope, EventTarget>(engine);
    wire_registry_prototype::<crate::js::Types, DedicatedWorkerGlobalScope, WorkerGlobalScope>(
        engine,
    );

    // Step 6b: Wire constructor prototype chains.
    wire_registry_constructor_prototype::<crate::js::Types, MessagePort, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, AbortSignal, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Node, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, OffscreenCanvas, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Document, Node>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Element, Node>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, Worker, EventTarget>(engine);
    wire_registry_constructor_prototype::<crate::js::Types, WorkerGlobalScope, EventTarget>(engine);
    wire_registry_constructor_prototype::<
        crate::js::Types,
        DedicatedWorkerGlobalScope,
        WorkerGlobalScope,
    >(engine);

    // Step 6c: DOMException inherits from the realm's Error constructor.
    if let Some(de_proto) = get_registry_prototype::<crate::js::Types, DOMException>(engine) {
        let realm = engine.current_realm();
        let intrinsics = engine.realm_intrinsics(&realm);
        if let Err(error) = engine.set_prototype(de_proto, Some(intrinsics.error_prototype.clone()))
        {
            error!("failed to wire DOMException to Error.prototype: {error:?}");
        }
    }

    // Step 7: Set the global object's prototype to
    // DedicatedWorkerGlobalScope.prototype so `instanceof` and the global
    // members (self, postMessage, name, ...) resolve through the worker
    // prototype chain.
    if let Some(dedicated_proto) =
        get_registry_prototype::<crate::js::Types, DedicatedWorkerGlobalScope>(engine)
    {
        let proto_set = engine.set_prototype(global_obj.clone(), Some(dedicated_proto.clone()));
        let immutable_global_proto = match proto_set {
            Ok(true) => false,
            Ok(false) | Err(_) => true,
        };

        // Step 7b: Engines with an immutable global object [[Prototype]]
        // (e.g. JSC) fall back to copying the worker prototype properties
        // onto the global object.
        if immutable_global_proto {
            let prototypes = [
                get_registry_prototype::<crate::js::Types, EventTarget>(engine),
                get_registry_prototype::<crate::js::Types, WorkerGlobalScope>(engine),
                Some(dedicated_proto),
            ];
            for proto in prototypes.iter().flatten() {
                if let Ok(keys) = engine.own_property_keys(proto.clone()) {
                    for key in keys {
                        let key_str = engine.property_key_to_rust_string(&key);
                        if key_str == "constructor" || key_str == "__proto__" {
                            continue;
                        }
                        match engine.get_own_property(proto.clone(), key.clone()) {
                            Ok(Some(descriptor))
                                if descriptor.value.is_some() || descriptor.get.is_some() =>
                            {
                                if let Err(error) = engine.define_property_or_throw(
                                    global_obj.clone(),
                                    key,
                                    descriptor,
                                ) {
                                    error!(
                                        "failed to copy a worker prototype property to the global object: {error:?}"
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Step X: Install TestUtils namespace (gc() method).
    crate::js::bindings::testutils::install_testutils_namespace(engine)
        .map_err(|error| format!("failed to install TestUtils namespace: {:?}", error))?;

    Ok(())
}

/// <https://webidl.spec.whatwg.org/#js-iterable>
/// Install the @@iterator of a pair-iterable interface's prototype.
/// <https://webidl.spec.whatwg.org/#define-the-iteration-methods>
fn install_pair_iterator<T: crate::webidl::PairIterable>(
    engine: &mut Engine,
) -> Result<(), String> {
    use crate::webidl::bindings::get_registry_prototype;
    use js_engine::records::PropertyDescriptor;
    use js_engine::{EcmascriptHost, ExecutionContext as _};

    // The @@iterator property's value is the same function object as the
    // entries method's.
    let Some(prototype) = get_registry_prototype::<crate::js::Types, T>(engine) else {
        return Err(format!("{} is not registered", T::NAME));
    };
    let entries = EcmascriptHost::get(engine, &prototype, "entries")
        .map_err(|error| format!("failed to read {}.prototype.entries: {error:?}", T::NAME))?;
    let iterator_key = engine.property_key_from_well_known_symbol("iterator");
    engine
        .define_property_or_throw(
            prototype,
            iterator_key,
            PropertyDescriptor {
                value: Some(entries),
                writable: Some(true),
                enumerable: Some(false),
                configurable: Some(true),
                get: None,
                set: None,
            },
        )
        .map_err(|error| format!("failed to install {} @@iterator: {error:?}", T::NAME))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use blitz_dom::{BaseDocument, DocumentConfig};
    use js_engine::ExecutionContext;
    use url::Url;

    use crate::html::EnvironmentSettingsObject;

    fn new_document() -> Rc<RefCell<BaseDocument>> {
        Rc::new(RefCell::new(BaseDocument::new(DocumentConfig::default())))
    }

    #[test]
    fn realm_script_resolves_document_global() {
        let creation_url = Url::parse("about:blank").expect("parse creation URL");
        let mut parent_settings =
            EnvironmentSettingsObject::new(new_document(), creation_url.clone())
                .expect("build parent settings object");
        let mut child_settings = EnvironmentSettingsObject::new_in_realm(
            Some(&mut parent_settings.realm_execution_context),
            new_document(),
            creation_url,
            None,
            None,
        )
        .expect("build child settings object");

        let document_type = child_settings
            .realm_execution_context
            .evaluate_script("typeof document")
            .expect("evaluate document global");
        let document_type = child_settings
            .realm_execution_context
            .to_rust_string(document_type)
            .expect("convert document type");

        assert_eq!(document_type, "object");
    }
}
