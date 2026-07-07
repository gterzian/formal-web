pub(crate) mod bindings;
/// Generic platform-object downcast helpers:
/// `try_with_*` functions using [`ExecutionContext::with_object_any`] / `with_object_any_mut`.
pub(crate) mod downcast;
/// Generic platform-object resolution helpers.
/// Uses only [`ExecutionContext`] trait methods.
pub(crate) mod platform_objects;

/// Generic engine builder — the single entry point for creating a JS engine
/// context.  Uses `#[cfg]` internally to switch between Boa and JSC backends.
pub(crate) mod build_context;

/// Generic bootstrap modules — use only [`ExecutionContext<T>`] trait methods.
/// Not engine-specific; compiled on all backends.
pub(crate) mod console_generic;
pub(crate) mod css_generic;

pub(crate) use console_generic::install_console_namespace;
pub(crate) use css_generic::install_css_namespace;

pub(crate) use bindings::install_document_property;
pub(crate) use downcast::{
    try_with_abort_controller_ref, try_with_abort_signal_mut, try_with_abort_signal_ref,
    try_with_event_mut, try_with_event_target_mut, try_with_event_target_ref,
    with_abort_signal_ref,
};

/// Content-level type alias for the concrete JS types in use.
/// Set by the build script based on the target platform:
/// `jsc_backend` on Apple platforms, `boa_backend` on others.
#[cfg(jsc_backend)]
pub(crate) type Types = js_engine::jsc::JscTypes;

#[cfg(not(jsc_backend))]
pub(crate) type Types = js_engine::boa::BoaTypes;

/// Content-level type alias for the concrete JS engine in use.
/// `BoaContext` on Boa, `JscEngine` on JSC.
#[cfg(jsc_backend)]
pub(crate) type Engine = js_engine::jsc::JscEngine;

#[cfg(not(jsc_backend))]
pub(crate) type Engine = js_engine::boa::BoaContext;
