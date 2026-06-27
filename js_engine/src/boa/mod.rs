//! # `boa` — Boa Engine Backend
//!
//! This module provides the Boa implementation of the `js_engine` traits
//! (`JsTypes`, `JsEngine`).  It is gated behind the `boa` Cargo feature.
//!
//! ## Submodules
//!
//! | Module | Contents |
//! |---|---|
//! | [`types`] | `BoaTypes` — the `JsTypes` / `JsTypesWithRealm` marker type |
//! | [`engine`] | `BoaEngine` — the `JsEngine<BoaTypes>` implementation |
//!
//! # Hard problems (not yet implemented — marked with `todo!()`)
//!
//! - **Jobs/microtasks** — `Context::run_jobs` exists but `enqueue_job` needs
//!   to work with the job executor model.
//! - **Generator operations** — Boa has `JsGenerator` but the `GeneratorStart`
//!   operation is closely tied to the VM internals.
//! - **Module evaluation** — `Context::parse_module` + `Context::evaluate_module`
//!   exist but require module loader setup.
//! - **SharedArrayBuffer** — `JsSharedArrayBuffer` exists but `allocate_shared_array_buffer`
//!   needs the constructor reference.
//! - **AsyncGenerator** — Not fully wired through the trait yet.

mod engine;
mod types;

pub use engine::BoaEngine;
pub use types::BoaTypes;
