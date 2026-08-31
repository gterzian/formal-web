pub(crate) mod global_event_handlers;
mod html_anchor_element;
mod html_element;
mod html_iframe_element;
pub(crate) mod html_input_element;
pub(crate) mod html_media_element;
pub(crate) mod html_video_element;
pub(crate) mod hyperlink_element_utils;
mod location;
pub(crate) mod message_event;
pub(crate) mod messageport;
pub(crate) mod window;
pub(crate) mod windowproxy;
pub(crate) mod worker;
pub(crate) mod worker_global_scope;
pub(crate) mod worker_location;
pub(crate) mod worker_navigator;

pub(crate) use html_element::style_declaration_object;
