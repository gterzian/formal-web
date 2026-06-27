use log::error;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    context::{intrinsics::Intrinsics, ContextBuilder, HostHooks},
    job::SimpleJobExecutor,
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, JsObject},
    property::PropertyDescriptor,
    symbol::JsSymbol,
    Context, Source,
};

use super::hyperlink_element_utils;
use crate::dom::{
    AbortController, AbortSignal, DOMException, Document, Element, Event, EventTarget, Node,
    UIEvent,
};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, HTMLInputElement,
    HTMLMediaElement, HTMLVideoElement, Location, Window,
};
use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, ReadableByteStreamController, ReadableStream,
    ReadableStreamBYOBReader, ReadableStreamBYOBRequest, ReadableStreamDefaultController,
    ReadableStreamDefaultReader, TransformStream, TransformStreamDefaultController, WritableStream,
    WritableStreamDefaultController, WritableStreamDefaultWriter,
};
use crate::webidl::bindings::{
    get_registry_prototype, initialize_registry, register_interface_spec, wire_registry_prototype,
};
// Note: AbortSignal static methods (abort, timeout, any) are registered via
// static operations in `AbortSignal::define_members`.
use super::super::streams::readablestream::{pipe_to_native_method, values_method};

pub(crate) struct WindowHostHooks {
    document: Rc<RefCell<BaseDocument>>,
}

impl WindowHostHooks {
    pub(crate) fn new(document: Rc<RefCell<BaseDocument>>) -> Self {
        Self { document }
    }
}

impl HostHooks for WindowHostHooks {
    fn create_global_object(&self, intrinsics: &Intrinsics) -> JsObject {
        JsObject::from_proto_and_data(
            intrinsics.constructors().object().prototype(),
            Window::new(GlobalScope::new(
                crate::html::GlobalScopeKind::Window,
                Rc::clone(&self.document),
            )),
        )
    }
}

/// Build the Boa engine, registering all native bindings.
///
/// Returns a fully-initialized `Engine` with all interfaces, prototypes,
/// and native functions registered.  Access the underlying `Context` via
/// `engine.context()` for Boa-specific operations not yet abstracted.
pub(crate) fn build_boa_engine(document: Rc<RefCell<BaseDocument>>) -> Result<crate::js::Engine, String> {
    let context = build_boa_context(document)?;
    Ok(crate::js::Engine::from_context(context))
}

fn build_boa_context(document: Rc<RefCell<BaseDocument>>) -> Result<Context, String> {
    let mut context = ContextBuilder::new()
        .host_hooks(Rc::new(WindowHostHooks::new(document)))
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .build()
        .map_err(|error| error.to_string())?;

    initialize_registry::<js_engine::boa::BoaTypes>(crate::js::context_as_ec(&mut context));

    // ── Install WebAssembly namespace ──
    if let Err(error) = crate::js::bindings::install_wasm_namespace(&mut context) {
        error!("[content] failed to install WebAssembly namespace: {error}");
    }

    macro_rules! reg {
        ($ty:ty) => {
            register_interface_spec::<$ty>(&mut context).map_err(|error| error.to_string())?;
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

    if let Some(de_proto) = get_registry_prototype::<DOMException>(&context) {
        de_proto.set_prototype(Some(
            context.intrinsics().constructors().error().prototype(),
        ));
    }

    wire_registry_prototype::<UIEvent, Event>(&mut context);
    wire_registry_prototype::<AbortSignal, EventTarget>(&mut context);
    wire_registry_prototype::<Node, EventTarget>(&mut context);
    wire_registry_prototype::<Document, Node>(&mut context);
    wire_registry_prototype::<Element, Node>(&mut context);
    wire_registry_prototype::<HTMLElement, Element>(&mut context);
    wire_registry_prototype::<HTMLAnchorElement, HTMLElement>(&mut context);
    wire_registry_prototype::<HTMLIFrameElement, HTMLElement>(&mut context);
    wire_registry_prototype::<HTMLMediaElement, HTMLElement>(&mut context);
    wire_registry_prototype::<HTMLVideoElement, HTMLMediaElement>(&mut context);
    wire_registry_prototype::<HTMLInputElement, HTMLElement>(&mut context);
    wire_registry_prototype::<Window, EventTarget>(&mut context);

    // ── Post-registration wiring ──

    // HTMLAnchorElement: HTMLHyperlinkElementUtils members (§HTMLHyperlinkElementUtils)
    if let Some(proto) = get_registry_prototype::<HTMLAnchorElement>(&context) {
        hyperlink_element_utils::register_hyperlink_element_utils_on_prototype(
            &proto,
            &mut context,
        )
        .map_err(|error| error.to_string())?;
    }

    // ReadableStream: async iterator, pipeTo (§ReadableStream)
    // Note: Static methods (ReadableStream.from(), AbortSignal.abort(), etc.) are
    // registered automatically by `register_interface_spec` via static operations.

    if let Some(rs_proto) = get_registry_prototype::<ReadableStream>(&context) {
        let realm = context.realm().clone();

        // values() and @@asyncIterator
        let values_fn =
            FunctionObjectBuilder::new(&realm, NativeFunction::from_fn_ptr(values_method))
                .name(js_string!("values"))
                .length(0)
                .constructor(false)
                .build();

        let values_desc = PropertyDescriptor::builder()
            .value(values_fn.clone())
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(js_string!("values"), values_desc, &mut context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        let symbol_desc = PropertyDescriptor::builder()
            .value(values_fn)
            .writable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(JsSymbol::async_iterator(), symbol_desc, &mut context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        // pipeTo with JS wrapper workaround
        let pipe_to_native_fn =
            FunctionObjectBuilder::new(&realm, NativeFunction::from_fn_ptr(pipe_to_native_method))
                .name(js_string!("pipeTo"))
                .length(2)
                .constructor(false)
                .build();

        let native_desc = PropertyDescriptor::builder()
            .value(pipe_to_native_fn)
            .writable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(
                js_string!("__formalWebReadableStreamPipeToNative"),
                native_desc,
                &mut context,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        let pipe_to_wrapper = context.eval(Source::from_bytes(
            "(function pipeTo() { return ReadableStream.prototype.__formalWebReadableStreamPipeToNative.call(this, arguments[0], arguments[1]); })",
        ))
        .map_err(|error| error.to_string())?
            .as_object()
            .ok_or_else(|| {
                String::from("ReadableStream.pipeTo wrapper initialization did not return a function")
            })?.clone();

        let pipe_to_desc = PropertyDescriptor::builder()
            .value(pipe_to_wrapper)
            .writable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(js_string!("pipeTo"), pipe_to_desc, &mut context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;
    }

    Ok(context)
}
