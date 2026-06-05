mod host_hooks;
mod html_anchor_element;
mod html_element;
mod html_iframe_element;
mod hyperlink_element_utils;
mod location;
mod window;

pub(crate) use host_hooks::build_boa_context;
pub(crate) use host_hooks::wire_interface_prototypes;
pub(crate) use html_element::style_declaration_object;
pub(crate) use window::create_window_proxy;
