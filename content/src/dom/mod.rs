mod dispatch;
pub mod document;
pub mod element;
pub mod event;
pub mod node;
mod ui_event_dispatch;

pub(crate) use dispatch::{
	EventDispatchHost, dispatch, dispatch_with_chain, dispatch_window_event, fire_event,
};
pub use document::Document;
pub use element::Element;
pub use event::{AT_TARGET, BUBBLING_PHASE, CAPTURING_PHASE, Event, EventTarget, UIEvent};
pub(crate) use event::{with_event_mut, with_event_target_mut, with_event_target_ref};
pub use node::Node;
pub(crate) use ui_event_dispatch::dispatch_ui_event;
