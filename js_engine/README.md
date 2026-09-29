# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JavaScriptCore, and V8) and formal-web's
HTML/DOM/WebIDL layers. Content code never depends on backend-specific APIs —
it sees only the generic traits below.

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

### Platform object proxies

`ExecutionContext::create_platform_object_proxy(target, handler)` builds a
Proxy whose `with_object_any`/`with_object_any_mut` resolve to the target's
platform data, so a Web IDL operation called with the proxy as `this`
downcasts to the platform object (Web IDL legacy platform objects are built
this way).  A proxy made with plain `create_proxy`, or by script, never
resolves.  V8 marks the target with a private symbol and reads it through
`Proxy::GetTarget` when wrapping a value.  Boa has no public accessor for a
proxy's target, and JSC no public API for it either, so both keep the
(proxy, target) pairs in the realm's host-defined store; those handles live
as long as the realm.

### Module layout

| Module | Contents |
|---|---|
| `types` | `JsTypes`, `JsTypesWithRealm` |
| `engine` | `JsEngine`, `ExecutionContext`, `Completion`, `HostHooks` |
| `enums` | `Numeric`, `PreferredType`, `IntegrityLevel`, `PromiseState`, etc. |
| `records` | `IteratorRecord`, `PromiseCapability`, `PromiseResolvers`, `PropertyDescriptor`, `RealmIntrinsics` |
| `gc` | `Trace`, `Finalize`, `GcRootHandle`, `GcCell` (backend-abstracted) |
| `boa/` | Boa backend — see `src/boa/README.md` |
| `jsc/` | JSC backend (macOS, experimental) — see `src/jsc/README.md` |
| `v8/` | V8 backend through `rusty_v8` (macOS arm64) — see `src/v8/README.md` |

## Feature flags

| Flag | Engine | Default |
|---|---|---|
| `v8` | V8 150.1.0 through `rusty_v8` (macOS arm64) | **default** |
| `boa` | Boa (git dep) | opt-in |
| `jsc` | JavaScriptCore (macOS, experimental) | opt-in |

Exactly one engine feature must be active. The `wasm` feature (the
Wasmtime-based WebAssembly implementation) only applies to the Boa backend
— Boa has no native WebAssembly, while V8 and JSC implement WebAssembly
natively.

## Per-engine documentation

- [`src/v8/README.md`](src/v8/README.md) — V8 backend design (cppgc tracing), build commands, WPT results, remaining work
- [`src/boa/README.md`](src/boa/README.md) — Boa backend build commands, WPT results
- [`src/jsc/README.md`](src/jsc/README.md) — JSC backend build commands, WPT results, remaining work

## Known cross-engine failures

### BYOB byte-stream WPT failures (both backends)

`streams/readable-byte-streams/patched-global.any.js` and
`streams/readable-byte-streams/respond-after-enqueue.any.js` fail on both Boa
and V8: the tests read back zeroed bytes instead of the values written via
`byobRequest.respond()`. The BYOB failures have appeared and disappeared
between runs; `enqueue-with-detached-buffer.any.js` FAIL
(`controller.enqueue` after detaching `byobRequest.view.buffer` does not
throw) is the same class of issue. Not yet investigated. See
`content/src/streams/README.md` for the detailed failure notes.
