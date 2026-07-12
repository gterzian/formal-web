#[cfg(boa_backend)]
mod host_hooks;
mod html_anchor_element;
mod html_element;
mod html_iframe_element;
pub(crate) mod html_input_element;
pub(crate) mod html_media_element;
pub(crate) mod html_video_element;
pub(crate) mod hyperlink_element_utils;
mod location;
mod window;

#[cfg(boa_backend)]
pub(crate) use host_hooks::build_context;
#[cfg(boa_backend)]
pub(crate) use host_hooks::set_boa_job_callback;
pub(crate) use html_element::style_declaration_object;
