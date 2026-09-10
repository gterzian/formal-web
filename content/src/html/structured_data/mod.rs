//! The HTML spec's "safe passing of structured data"
//! (<https://html.spec.whatwg.org/#safe-passing-of-structured-data>): the
//! generic structured serialization algorithms in
//! [`safe_passing_of_structured_data`], with the per-platform-object parts
//! of those algorithms split into their own modules (see
//! [`messageport`]).  The wire-format data lives in
//! `ipc_messages::safe_passing_of_structured_data` so it can cross IPC.
//! See `README.md` in this directory for the module structure.

pub(crate) mod messageport;
pub(crate) mod offscreen_canvas;
pub(crate) mod safe_passing_of_structured_data;
