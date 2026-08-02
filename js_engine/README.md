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
| `boa/` | Boa backend implementation (default) |
| `jsc/` | JSC backend implementation (macOS only) |
| `v8/` | V8 backend implementation through `rusty_v8` (macOS arm64 only) |

Engine-specific documentation — status, GC integration, open issues, and
investigation logs — lives in the nested `README.md` files:
[`src/boa/README.md`](src/boa/README.md), [`src/jsc/README.md`](src/jsc/README.md),
[`src/v8/README.md`](src/v8/README.md).

## Feature flags

| Flag | Engine | Default |
|---|---|---|
| `boa` | Boa (git dep) | **default** |
| `jsc` | JavaScriptCore (macOS, experimental) | opt-in |
| `v8` | V8 150.1.0 through `rusty_v8` (macOS arm64) | opt-in |

Exactly one engine feature must be active. V8 and WebAssembly cannot be
enabled together.

## Build & test (default Boa backend)

```bash
# Build everything
rustup run 1.94.0 cargo build --release

# Run WPT suite
rustup run 1.94.0 cargo run --release -- wpt
```

The other backends (JSC, V8) have their own build/test invocations —
see the nested READMEs.  WebAssembly (`wasm` feature) is Boa-only.

## Generic engine tests

`content/src/generic_js_test.rs` exercises the generic layer (platform
objects, promises, GC rooting, iterators, typed arrays) on every backend:

```bash
rustup run 1.94.0 cargo test --no-default-features --features <jsc|v8> -p content generic_js_test
```
