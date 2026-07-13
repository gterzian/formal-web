# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.  Migration to a fully generic `JsEngine<T>` /
`ExecutionContext<T>` trait architecture is complete — content code
never depends on backend-specific APIs.

## Architecture

Two categories of abstraction:

1. **Standard** — `JsEngine<T>` and `ExecutionContext<T>` mirror ECMA-262
   abstract operations (§7–§27).  `ExecutionContext<T>` is the runtime
   handle threaded through every binding function and domain method — it
   IS the HTML spec's realm execution context.
2. **Engine-specific** — `gc.rs` abstracts GC (`Trace`, `Finalize`,
   `GcRootHandle`, `GcCell`) which has no ECMA-262 equivalent.

### Key traits

| Trait | Role |
|---|---|
| `JsTypes` | Associated types for a backend's value/object/string/realm/etc. |
| `JsEngine<T>` | Factory operations: realm creation, script evaluation, builtin functions |
| `ExecutionContext<T>` | Runtime handle for all ECMA-262 operations that reference the surrounding agent's running execution context |
| `JsTypesGcExt` | Cycle-safe reflector link between Rust domain objects and their JS wrappers |

### Module layout

| Module | Contents |
|---|---|
| `types` | `JsTypes`, `JsTypesWithRealm` |
| `engine` | `JsEngine`, `ExecutionContext`, `Completion`, `HostHooks` |
| `enums` | `Numeric`, `PreferredType`, `IntegrityLevel`, `PromiseState`, etc. |
| `records` | `IteratorRecord`, `PromiseCapability`, `PromiseResolvers`, `PropertyDescriptor`, `RealmIntrinsics` |
| `gc` | `Trace`, `Finalize`, `GcRootHandle`, `GcCell` (backend-abstracted) |
| `boa/` | Boa backend implementation |
| `jsc/` | JSC backend implementation (macOS only) |

## Feature flags

| Flag | Engine | Default |
|---|---|---|
| `boa` | Boa (git dep) | **default** |
| `jsc` | JavaScriptCore (macOS, experimental) | opt-in |

At most one engine feature can be active.

## Build commands

### Boa (default, runs WPT)

```bash
# Build everything
rustup run 1.94.0 cargo build --release

# Run WPT suite
rustup run 1.94.0 cargo run --release -- wpt
```

### Boa + WebAssembly

```bash
rustup run 1.94.0 cargo build --release --features wasm
```

### JSC (macOS only, experimental)

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p js_engine

# Build content binary with JSC
rustup run 1.94.0 cargo build --release --no-default-features --features jsc -p content --bin formal-web-content

# Run a single WPT test via JSC
target/release/formal-web wpt dom/nodes/Element-hasAttribute.html
```

## WPT test results

### Boa backend (primary — run full suite)

Latest: `executed=79 unexpected=0`

Wasm tests are excluded from the default WPT run (opt-in `--features wasm`).

### JSC backend (experimental)

**PASS:** CSS.supports, DOM Element tests, Node-constants, document.title,
document-dir, iframe, anchor, basic streams (constructor, default-reader,
strategies, transform, writable), formal gc-protection.

**TIMEOUT:**  Most piping tests, cancel, read-task-handling.

**FAIL:** structured-clone (Blob not implemented), wasm compile (timeout).

## Remaining work

### JSC microtask drain during nested C API calls

`promise_state()` uses `eval_script_raw("void 0")` to drain microtasks, but
JSC only drains its microtask queue when control returns from the outermost
C API call.  Inside nested calls (common — stream algorithm code runs inside
a JS call), the eval does not trigger drainage and `.then()` handlers never
fire.

**Dead end:** No public C API forces JSC microtask drainage.  Tracked
promise states fail because stream algorithms poll CHAINED promises (via
`.then()`), not the original tracked promise.

### Other unfixed issues

- **`setTimeout` not pumped during piping tests** — `delay()` timeouts.
- **`instanceof Window` returns false (JSC)** — Global object's `[[Prototype]]`
  is immutable through the public C API.
- **`WindowTimer.arguments`** — `Vec<JsValue>` elements unprotected from GC.
  Needs `GcRootHandle` wrapping.
- **`detach_array_buffer` (JSC)** — No-op (`Ok(())`).
- **`species_constructor`** — Always returns `default_constructor`.
- **Cross-realm `new.target`** — `get_function_realm` always returns current realm.
- **WASM compile/instantiate timeout (JSC)** — Background compilation requires
  the creating thread's run loop to be pumped.
