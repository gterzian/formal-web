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
use crate::webidl::binding::{
    get_registry_prototype, initialize_registry, register_interface_spec,
    wire_registry_prototype,
};

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

pub(crate) fn build_boa_context(document: Rc<RefCell<BaseDocument>>) -> Result<Context, String> {
    let mut context = ContextBuilder::new()
        .host_hooks(Rc::new(WindowHostHooks::new(document)))
        .job_executor(Rc::new(SimpleJobExecutor::new()))
        .build()
        .map_err(|error| error.to_string())?;

    initialize_registry(&mut context);

    macro_rules! reg {
        ($ty:ty) => {
            register_interface_spec::<$ty>(&mut context)
                .map_err(|error| error.to_string())?;
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
        de_proto.set_prototype(Some(context.intrinsics().constructors().error().prototype()));
    }

    wire_registry_prototype::<UIEvent, Event>(&mut context);
    wire_registry_prototype::<AbortSignal, EventTarget>(&mut context);
    wire_registry_prototype::<Node, EventTarget>(&mut context);
    wire_registry_prototype::<Document, Node>(&mut context);
    wire_registry_prototype::<Element, Node>(&mut context);
    wire_registry_prototype::<HTMLElement, Element>(&mut context);
    wire_registry_prototype::<HTMLAnchorElement, HTMLElement>(&mut context);
    wire_registry_prototype::<HTMLIFrameElement, HTMLElement>(&mut context);
    wire_registry_prototype::<Window, EventTarget>(&mut context);

    Ok(context)
}
