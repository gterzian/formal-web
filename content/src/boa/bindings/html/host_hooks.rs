use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context,
    context::{ContextBuilder, HostHooks, intrinsics::Intrinsics},
    job::SimpleJobExecutor,
    js_string,
    object::JsObject,
    property::Attribute,
};
use boa_runtime::extensions::{RuntimeExtension, StructuredCloneExtension};

use crate::boa::{
    install_console_namespace, install_document_property,
    platform_objects::store_document_object,
};
use crate::dom::{
    AbortController, AbortSignal, DOMException, Document, Element, Event, EventTarget, Node, UIEvent,
};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLIFrameElement, HTMLElement, Location, Window,
};
use crate::streams::{
    ByteLengthQueuingStrategy, CountQueuingStrategy, ReadableByteStreamController, ReadableStream,
    ReadableStreamBYOBReader, ReadableStreamBYOBRequest, ReadableStreamDefaultController,
    ReadableStreamDefaultReader, TransformStream, TransformStreamDefaultController, WritableStream,
    WritableStreamDefaultController, WritableStreamDefaultWriter,
};

/// <https://html.spec.whatwg.org/#global-object>
/// Boa host hooks that create the Window as the realm's global object.
pub(crate) struct WindowHostHooks {
    /// The base document that the Window's GlobalScope wraps.
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

/// Build a boa `Context` pre-configured with a Window global object and all
/// registered Web API classes. Returns the context so the caller can capture
/// a pointer to the GlobalScope inside the Window for direct access.
pub(crate) fn build_boa_context(
    document: Rc<RefCell<BaseDocument>>,
) -> Result<Context, String> {
    let mut context = ContextBuilder::new()
        .host_hooks(Rc::new(WindowHostHooks::new(document)))
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .build()
        .map_err(|error| error.to_string())?;

    StructuredCloneExtension
        .register(None, &mut context)
        .map_err(|error| error.to_string())?;

    context
        .register_global_class::<EventTarget>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<DOMException>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Event>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<UIEvent>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<AbortSignal>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<AbortController>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Node>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Document>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Element>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<HTMLElement>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<HTMLAnchorElement>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<HTMLIFrameElement>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Window>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<Location>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ByteLengthQueuingStrategy>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<CountQueuingStrategy>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableStream>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableStreamDefaultController>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableByteStreamController>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableStreamDefaultReader>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableStreamBYOBReader>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<ReadableStreamBYOBRequest>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<WritableStream>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<WritableStreamDefaultController>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<WritableStreamDefaultWriter>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<TransformStream>()
        .map_err(|error| error.to_string())?;
    context
        .register_global_class::<TransformStreamDefaultController>()
        .map_err(|error| error.to_string())?;

    wire_interface_prototypes(&mut context);

    Ok(context)
}

/// Install the `document` and `window` properties on the global object.
pub(crate) fn install_global_properties(
    context: &mut Context,
    document_object: JsObject,
) -> Result<(), String> {
    let global = context.global_object();
    if let Some(window_class) = context.get_global_class::<Window>() {
        global.set_prototype(Some(window_class.prototype()));
    }

    store_document_object(context, document_object.clone())
        .map_err(|error| error.to_string())?;
    install_document_property(context).map_err(|error| error.to_string())?;
    install_console_namespace(context).map_err(|error| error.to_string())?;
    context
        .register_global_property(js_string!("window"), global.clone(), Attribute::all())
        .map_err(|error| error.to_string())?;
    context
        .register_global_property(js_string!("self"), global, Attribute::all())
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn wire_interface_prototypes(context: &mut Context) {
    use boa_engine::class::Class;

    fn set_registered_interface_prototype<Child: Class, Parent: Class>(context: &mut Context) {
        let Some(child) = context.get_global_class::<Child>() else {
            return;
        };
        let Some(parent) = context.get_global_class::<Parent>() else {
            return;
        };
        child.prototype().set_prototype(Some(parent.prototype()));
        child
            .constructor()
            .set_prototype(Some(parent.constructor()));
    }

    if let Some(dom_exception) = context.get_global_class::<DOMException>() {
        dom_exception.prototype().set_prototype(Some(
            context.intrinsics().constructors().error().prototype(),
        ));
    }

    set_registered_interface_prototype::<UIEvent, Event>(context);
    set_registered_interface_prototype::<AbortSignal, EventTarget>(context);
    set_registered_interface_prototype::<Window, EventTarget>(context);
    set_registered_interface_prototype::<Node, EventTarget>(context);
    set_registered_interface_prototype::<Document, Node>(context);
    set_registered_interface_prototype::<Element, Node>(context);
    set_registered_interface_prototype::<HTMLElement, Element>(context);
    set_registered_interface_prototype::<HTMLAnchorElement, HTMLElement>(context);
    set_registered_interface_prototype::<HTMLIFrameElement, HTMLElement>(context);
}
