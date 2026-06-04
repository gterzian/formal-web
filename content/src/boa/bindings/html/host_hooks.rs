use std::{cell::RefCell, rc::Rc};

use blitz_dom::BaseDocument;
use boa_engine::{
    Context,
    context::{ContextBuilder, HostHooks, intrinsics::Intrinsics},
    job::SimpleJobExecutor,
    object::JsObject,
};

use crate::dom::{
    AbortController, AbortSignal, DOMException, Document, Element, Event, EventTarget, Node,
    UIEvent,
};
use crate::html::{
    GlobalScope, HTMLAnchorElement, HTMLElement, HTMLIFrameElement, Location, Window,
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
pub(crate) fn build_boa_context(document: Rc<RefCell<BaseDocument>>) -> Result<Context, String> {
    let mut context = ContextBuilder::new()
        .host_hooks(Rc::new(WindowHostHooks::new(document)))
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .build()
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
/// Wire up the prototype chain for all registered Web API classes.
/// Must be called after the context is built but before any objects are created,
/// so that inheritance (e.g. Window → EventTarget) works correctly.
pub(crate) fn wire_interface_prototypes(context: &mut Context) {
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
