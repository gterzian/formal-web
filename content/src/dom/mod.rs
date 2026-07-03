mod abort;
#[cfg(boa_backend)]
mod dispatch;
pub mod document;
mod dom_exception;
pub mod element;
pub mod event;
pub mod node;
#[cfg(boa_backend)]
mod ui_event_dispatch;

#[cfg(boa_backend)]
pub(crate) use abort::signal_abort;
pub(crate) use abort::{AbortAlgorithm, create_abort_signal, initialize_dependent_abort_signal};
pub use abort::{AbortController, AbortSignal};
#[cfg(boa_backend)]
pub(crate) use dispatch::{
    EventDispatchHost, dispatch, dispatch_window_event, dispatch_with_chain, fire_event,
};
pub use document::Document;
pub use dom_exception::DOMException;
pub use element::Element;
pub use event::{AT_TARGET, BUBBLING_PHASE, CAPTURING_PHASE, Event, EventTarget, UIEvent};
pub use node::Node;
#[cfg(boa_backend)]
pub(crate) use ui_event_dispatch::{dispatch_trusted_click_event, dispatch_ui_event};
