mod abort;
mod dispatch;
pub mod document;
mod dom_exception;
pub mod element;
pub mod event;
pub mod node;
mod ui_event_dispatch;

pub(crate) use abort::{
    AbortAlgorithm, create_abort_signal, initialize_dependent_abort_signal, signal_abort,
};
pub use abort::{AbortController, AbortSignal};
pub(crate) use dispatch::{
    EventPathItem, dispatch_event, dispatch_with_path, fire_event, simple_path,
};
pub use document::Document;
pub use dom_exception::DOMException;
pub use element::Element;
pub(crate) use event::EventTargetAccess;
pub use event::{AT_TARGET, BUBBLING_PHASE, CAPTURING_PHASE, Event, EventTarget, UIEvent};
pub use node::Node;
pub(crate) use ui_event_dispatch::{
    build_path_from_target_js_object, dispatch_trusted_click_event, dispatch_ui_event,
};
