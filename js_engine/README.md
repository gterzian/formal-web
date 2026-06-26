# `js_engine` — generic JS engine trait

<https://tc39.es/ecma262/>

Bridges between ECMAScript engines (Boa, JSC) and formal-web's
HTML/DOM/WebIDL layers.  Two categories of abstraction:

### 1. Standard: `JsEngine<T>` mirrors ECMA-262 operations

Web standards already define their behavior in terms of ECMA-262 operations:
`Call`, `Get`, `ToNumber`, `NewPromiseCapability`, `PerformPromiseThen`,
`CreateRealm`, etc.  The trait exposes them generically.  No new abstractions.

### 2. Weird: `gc.rs` abstracts engine-specific GC

GC has no ECMA-262 equivalent.  This module is deliberately the one
engine-specific part of the crate.

## Layout

```
src/
  lib.rs        Crate root
  types.rs      JsTypes — language types (§6.1) and object subtypes
  engine.rs     JsEngine<T>, EcmascriptHost<T>, Completion, HostHooks
  enums.rs      Numeric, PreferredType, IntegrityLevel, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor
  gc.rs         Trace, Finalize, GcRootHandle (engine-specific)
  boa/          Boa backend (feature = "boa")
  jsc/          JSC backend (feature = "jsc")
```

## Feature flags

| Feature | Engine | Default |
|---|---|---|
| `boa` | Boa (git dep) | **default** |
| `jsc` | JavaScriptCore (macOS) | opt-in |

Mutually exclusive — only one engine at a time.

```bash
cargo check -p js_engine                          # Boa (default)
cargo check -p js_engine --no-default-features --features jsc  # JSC
```

## Spec documentation convention

Every method on `JsEngine<T>` has **only** the spec anchor URL as its doc
comment.  Example: `/// <https://tc39.es/ecma262/#sec-toboolean>`.
No prose, no summaries.  The spec IS the documentation.

Infrastructure traits (`Trace`, `Finalize`, etc.) carry no spec links —
they are not spec-defined operations.

## Design

| Component | File | Role |
|---|---|---|
| `JsTypes` | `types.rs` | Associated-type bundle: all ECMAScript language types |
| `JsEngine<T>` | `engine.rs` | All ECMA-262 operations callable from outside the engine |
| `EcmascriptHost<T>` | `engine.rs` | Narrower interface: ops Web IDL callback algorithms need |
| `Completion<T, Ty>` | `engine.rs` | `Result<T, Ty::JsValue>` — isomorphic to spec Completion Record |
| `HostHooks<T>` | `engine.rs` | HTML-specified host hooks (promise rejection, etc.) |

### What does NOT get abstracted

| Operation | Reason |
|---|---|
| GC heap traversal (`downcast_ref`) | Engine-specific — no ECMA-262 equivalent |
| Native function registration (`NativeFunction`) | Engine-specific API shape |
| Platform object construction | Uses Boa `ObjectInitializer` |
| Proxy creation | Boa's proxy builder not publicly creatable |

These are handled by `#[repr(transparent)]` casts in the `CreateBuiltinFunction`
shim (see `boa/engine.rs` module docs).

## Per-backend details

See module docs for implementation status and quirks:

| Backend | Module | Docs |
|---|---|---|
| Boa | `src/boa/mod.rs` | Hard problems, known quirks |
| JSC | `src/jsc/mod.rs` | FFI coverage, `todo!()` items |
| GC | `src/gc.rs` | The one engine-specific abstraction |

## Migration status

| Phase | What | Status |
|---|---|---|
| 1. Foundation | `js_engine` dep, content alias | ✅ |
| 2. ESO storage | `EnvironmentSettingsObject` stores `Engine` | ✅ |
| 3. WebIDL infra | `EcmascriptHost<T>` generic; `Callback` still Boa-concrete | ⏳ |
| 4. Domain layer | Replace `context()` calls with `JsEngine<T>` | ❌ |
| 5. JS bindings | `CreateBuiltinFunction` on trait + Boa impl | ⏳ |
| 6. Full generics | Parameterize content/ over `T: JsTypes` | ❌ |

Open problems and their fixes are documented in `src/engine.rs` module docs
and the per-backend module docs.
