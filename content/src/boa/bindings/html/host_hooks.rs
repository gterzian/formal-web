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
    install_console_namespace, install_document_property_with_object,
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

    register_web_api_classes(&mut context)?;

    Ok(context)
}

/// Register all Web API classes with the boa context.
fn register_web_api_classes(context: &mut Context) -> Result<(), String> {
    macro_rules! register {
        ($cls:ty) => {
            context
                .register_global_class::<$cls>()
                .map_err(|error| error.to_string())?;
        };
    }

    register!(EventTarget);
    register!(DOMException);
    register!(Event);
    register!(UIEvent);
    register!(AbortSignal);
    register!(AbortController);
    register!(Node);
    register!(Document);
    register!(Element);
    register!(HTMLElement);
    register!(HTMLAnchorElement);
    register!(HTMLIFrameElement);
    register!(Window);
    register!(Location);
    register!(ByteLengthQueuingStrategy);
    register!(CountQueuingStrategy);
    register!(ReadableStream);
    register!(ReadableStreamDefaultController);
    register!(ReadableByteStreamController);
    register!(ReadableStreamDefaultReader);
    register!(ReadableStreamBYOBReader);
    register!(ReadableStreamBYOBRequest);
    register!(WritableStream);
    register!(WritableStreamDefaultController);
    register!(WritableStreamDefaultWriter);
    register!(TransformStream);
    register!(TransformStreamDefaultController);

    Ok(())
}

/// Install the `document` and `window` properties on the global object.
pub(crate) fn install_global_properties(
    context: &mut Context,
    document_object: JsObject,
) -> Result<(), String> {
    // Wire interface prototypes BEFORE storing the document object, so the
    // prototype chain is fully established before any property accessor might
    // trigger a GC borrow on the global object's internal data.
    wire_interface_prototypes(context);

    store_document_object(context, document_object.clone())
        .map_err(|error| error.to_string())?;
    // Use the pre-resolved document object to avoid a with_global_scope call
    // that would borrow the global object's RefCell and conflict with the
    // subsequent register_global_property inside install_document_property.
    install_document_property_with_object(context, document_object.clone())
        .map_err(|error| error.to_string())?;
    install_console_namespace(context).map_err(|error| error.to_string())?;

    let global = context.global_object();
    if let Some(window_class) = context.get_global_class::<Window>() {
        global.set_prototype(Some(window_class.prototype()));
    }
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
