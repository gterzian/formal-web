use log::error;
use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context, Source,
    context::{ContextBuilder, HostHooks, intrinsics::Intrinsics},
    job::SimpleJobExecutor,
    js_string,
    object::JsObject,
    property::PropertyDescriptor,
    symbol::JsSymbol,
};
use js_engine::ExecutionContext;
use js_engine::boa::BoaContext;

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
    get_registry_prototype, initialize_registry, register_interface_spec,
    wire_registry_constructor_prototype, wire_registry_prototype,
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
        let data = Window::new(GlobalScope::new(
            crate::html::GlobalScopeKind::Window,
            Rc::clone(&self.document),
        ));
        // Wrap in TraceableBox so the Window's GC-traced fields (GcCell<>
        // references to Document, Event, etc.) remain visible to Boa's GC.
        JsObject::from_proto_and_data(
            intrinsics.constructors().object().prototype(),
            js_engine::boa::NativeDataWrapper(js_engine::boa::TraceableBox::new(data)),
        )
    }
}

/// Build a Boa context, registering all native bindings.
///
/// Returns a fully-initialized `BoaContext` with all interfaces, prototypes,
/// and native functions registered.  Access the underlying `Context` via
/// `engine.context()` for Boa-specific operations not yet abstracted.
pub(crate) fn build_context(document: Rc<RefCell<BaseDocument>>) -> Result<BoaContext, String> {
    let context = build_boa_context(document)?;
    Ok(BoaContext::from_context(context))
}

fn build_boa_context(document: Rc<RefCell<BaseDocument>>) -> Result<Context, String> {
    let context = ContextBuilder::new()
        .host_hooks(Rc::new(WindowHostHooks::new(document)))
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .build()
        .map_err(|error| error.to_string())?;

    let mut engine = js_engine::boa::BoaContext::from_context(context);

    initialize_registry::<crate::js::Types>(&mut engine);

    // Store the global object in host_any so that platform_objects.rs can
    // reach GlobalScope through the generic ExecutionContext trait.
    {
        let global_obj = engine.context().global_object();
        crate::js::platform_objects::init_global_object_slot(&mut engine, global_obj);
    }

    if let Err(error) = crate::js::bindings::install_wasm_namespace(&mut engine) {
        error!("[content] failed to install WebAssembly namespace: {error}");
    }

    macro_rules! reg {
        ($ty:ty) => {
            register_interface_spec::<crate::js::Types, $ty, _>(&mut engine)
                .map_err(|error| error.display().to_string())?;
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

    {
        let context = engine.context_ref();
        let de_proto = get_registry_prototype::<crate::js::Types, DOMException>(&engine);
        if let Some(ref de_proto) = de_proto {
            de_proto.set_prototype(Some(
                context.intrinsics().constructors().error().prototype(),
            ));
        }
    }

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

    wire_registry_constructor_prototype::<crate::js::Types, UIEvent, Event>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, AbortSignal, EventTarget>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, Node, EventTarget>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, Document, Node>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, Element, Node>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLElement, Element>(&mut engine);
    wire_registry_constructor_prototype::<crate::js::Types, HTMLAnchorElement, HTMLElement>(
        &mut engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, HTMLIFrameElement, HTMLElement>(
        &mut engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, HTMLMediaElement, HTMLElement>(
        &mut engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, HTMLVideoElement, HTMLMediaElement>(
        &mut engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, HTMLInputElement, HTMLElement>(
        &mut engine,
    );
    wire_registry_constructor_prototype::<crate::js::Types, Window, EventTarget>(&mut engine);


    // HTMLAnchorElement: HTMLHyperlinkElementUtils members (§HTMLHyperlinkElementUtils)
    if let Some(proto) = get_registry_prototype::<crate::js::Types, HTMLAnchorElement>(&engine) {
        hyperlink_element_utils::register_hyperlink_element_utils_on_prototype(&proto, &mut engine)
            .map_err(|error| error.display().to_string())?;
    }

    // ReadableStream: async iterator, pipeTo (§ReadableStream)
    // Note: Static methods (ReadableStream.from(), AbortSignal.abort(), etc.) are
    // registered automatically by `register_interface_spec` via static operations.

    if let Some(rs_proto) = get_registry_prototype::<crate::js::Types, ReadableStream>(&engine) {
        // Create builtin functions via generic EC operations (no Boa context needed)
        let values_fn: JsObject = engine
            .create_builtin_fn(
                Box::new(|args, this, ec| values_method(&this, args, ec)),
                0,
                engine.property_key_from_str("values"),
            )
            .into();

        let pipe_to_native_fn: JsObject = engine
            .create_builtin_fn(
                Box::new(|args, this, ec| pipe_to_native_method(&this, args, ec)),
                2,
                engine.property_key_from_str("pipeTo"),
            )
            .into();

        // Property definitions require the Boa Context directly
        let context = engine.context();

        // values() descriptor
        let values_desc = PropertyDescriptor::builder()
            .value(values_fn.clone())
            .writable(true)
            .enumerable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(js_string!("values"), values_desc, context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        // @@asyncIterator
        let symbol_desc = PropertyDescriptor::builder()
            .value(values_fn)
            .writable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(JsSymbol::async_iterator(), symbol_desc, context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        // __formalWebReadableStreamPipeToNative (native backstop)
        let native_desc = PropertyDescriptor::builder()
            .value(pipe_to_native_fn)
            .writable(true)
            .configurable(true)
            .build();
        rs_proto
            .define_property_or_throw(
                js_string!("__formalWebReadableStreamPipeToNative"),
                native_desc,
                context,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())?;

        // pipeTo with JS wrapper workaround
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
            .define_property_or_throw(js_string!("pipeTo"), pipe_to_desc, context)
            .map(|_| ())
            .map_err(|error| error.to_string())?;
    }

    if let Err(error) = crate::js::bindings::testutils::install_testutils_namespace(&mut engine) {
        error!(
            "[content] failed to install TestUtils namespace: {:?}",
            error
        );
    }

    Ok(engine.into_context())
}
