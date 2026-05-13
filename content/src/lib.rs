#[path = "main.rs"]
mod sidecar;

pub use sidecar::maybe_run_content_process;
pub use sidecar::{boa, dom, html, streams, webidl};
pub(crate) use sidecar::{ContentRuntime, EMPTY_HTML_DOCUMENT, NavigableContainerState, ui_event};
