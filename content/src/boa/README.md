`content/src/boa` integrates Boa with the content-process DOM runtime.

- `execution_context.rs` owns the Boa `Context`, global-object construction, and the Rust state that corresponds to an HTML environment and environment settings object.

- `runtime_data.rs` caches the platform objects associated with one Boa environment so repeated wrapper lookups reuse the same `JsObject` identity.

- `task_queue.rs` holds queued work for that environment. `JsExecutionContext::drain_tasks` runs those tasks and then performs the microtask checkpoint through Boa's job queue.

- `html_parser.rs` connects html5ever parsing to Blitz mutation, records parser errors, and queues classic inline scripts in document order.

- `event_handler.rs` bridges Blitz UI events into JavaScript event dispatch.

- Bindings in `content/src/boa/bindings` should convert arguments, select the right carrier object, and delegate. If a JavaScript-visible algorithm needs DOM or runtime state, move that logic onto the DOM carrier or Boa runtime struct that owns the state.

- Document runtime structs against HTML concepts such as `#environment`, `#environment-settings-object`, `#global-object`, and `#task-queue` instead of documenting them as if they were DOM interfaces.