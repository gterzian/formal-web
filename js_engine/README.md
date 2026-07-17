# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JavaScriptCore, and V8) and formal-web's
HTML/DOM/WebIDL layers.  Migration to a fully generic `JsEngine<T>` /
`ExecutionContext<T>` trait architecture is complete — content code
never depends on backend-specific APIs.

## Architecture

Two categories of abstraction:

1. **Standard** — `JsEngine<T>` and `ExecutionContext<T>` mirror ECMA-262
   abstract operations (§7–§27). `ExecutionContext<T>` is threaded through
   every binding function and domain method as the HTML specification's realm
   execution context.
2. **Engine-specific** — `gc.rs` abstracts GC (`Trace`, `Finalize`,
   `GcRootHandle`, `GcCell`) which has no ECMA-262 equivalent.

### Key traits

| Trait | Role |
|---|---|
| `JsTypes` | Associated types for a backend's value/object/string/realm/etc. |
| `JsEngine<T>` | Factory operations: realm creation, script evaluation, builtin functions |
| `ExecutionContext<T>` | Interface for ECMA-262 operations that reference the surrounding agent's running execution context |
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
| `v8/` | V8 backend implementation through `rusty_v8` (macOS arm64 only) |

## Feature flags

| Flag | Engine | Default |
|---|---|---|
| `boa` | Boa (git dep) | **default** |
| `jsc` | JavaScriptCore (macOS, experimental) | opt-in |
| `v8` | V8 150.1.0 through `rusty_v8` (macOS arm64) | opt-in |

Exactly one engine feature must be active. V8 and WebAssembly cannot be
enabled together.

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

### V8 (macOS arm64, opt-in)

```bash
# Build every process with V8 and media support
rustup run 1.94.0 cargo build --release \
  --no-default-features --features v8,media

# Run the browser after the complete build
rustup run 1.94.0 cargo run --release \
  --no-default-features --features v8,media

# Run the generic engine tests
rustup run 1.94.0 cargo test --no-default-features \
  --features v8 -p content generic_js_test
```

The first build downloads the pinned V8 150.1.0 archive. Set
`RUSTY_V8_ARCHIVE=/absolute/path/to/librusty_v8_release_aarch64-apple-darwin.a.gz`
to use a local archive, or set `RUSTY_V8_MIRROR` to an alternate releases base
URL. Cargo also caches downloaded archives under `.cargo/.rusty_v8` in the
Cargo home directory.

WebAssembly support is deferred for V8; use Boa with the `wasm` feature.

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
