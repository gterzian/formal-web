mod bindings;
mod downcast;
pub(crate) mod platform_objects;
pub(crate) use bindings::{install_console_namespace, install_document_property};
pub(crate) use downcast::{
	is_abort_signal_object, with_abort_controller_ref, with_abort_signal_mut,
	with_abort_signal_ref, with_event_mut, with_event_target_mut, with_event_target_ref,
};
