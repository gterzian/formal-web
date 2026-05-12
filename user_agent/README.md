# User Agent

The Rust browser runtime lives in this crate.

Keep new runtime ownership here, split by responsibility:
- `user_agent.rs` for top-level runtime commands and shared state.
- `event_loop.rs` for content event-loop threads, content child-process ownership, and task routing.
- `timer.rs` for the timer worker.
- `fetch.rs` for the fetch worker and dedicated network-process IPC.

Model long-running workers as stateful structs with `run(&mut self)` so thread-local state stays on the owning component instead of being spread across helper functions. The event loop owns its content subprocess directly; do not reintroduce a separate content bridge layer.

Track traversable target names in this crate when runtime messages announce new top-level traversables so iframe-host cleanup can stop the right event loop without routing that bookkeeping back through Lean.
Route Rust- and webview-originated navigation entrypoints through this crate, even when a temporary Lean hook still performs the underlying navigation semantics.

Do not grow `ffi` or `content_bridge` into the permanent home of the runtime.