# `js_engine` crate

Implements the ECMA-262 abstract operation trait (`JsEngine<T>`) that bridges
between ECMAScript engines (Boa, JSC) and formal-web's HTML/DOM/WebIDL layers.

<https://tc39.es/ecma262/>

## Features

| Feature | Engine | Default | Link |
|---|---|---|---|
| `boa` | Boa (git dep) | **default** | Links boa_engine; WebAssembly via wasmtime in `content/src/wasm/` |
| `jsc` | JavaScriptCore (macOS) | opt-in | Links system framework; WebAssembly built-in (no wasmtime) |

At most one engine feature can be active.  `default = ["boa"]`.

### Backend selection

```bash
# Boa (default)
cargo check -p js_engine

# JSC
cargo check -p js_engine --no-default-features --features jsc
```

### WebAssembly

- **Boa backend**: WebAssembly is provided externally via `wasmtime` in
  `content/src/wasm/`.  Boa delegates to wasmtime for compilation and
  instantiation; the content crate manages the background worker.
- **JSC backend**: WebAssembly is built into JavaScriptCore.  The
  `content/src/wasm/` module is not used.  Wasm compile/instantiate
  go through `JSEvaluateScript` or JSC's internal Wasm API.

## Layout

```
src/
  types.rs      JsTypes + JsTypesWithRealm
  engine.rs     JsEngine<T>, Completion, HostHooks
  enums.rs      Numeric, PreferredType, IntegrityLevel, IteratorKind, etc.
  records.rs    IteratorRecord, PromiseCapability, PropertyDescriptor, etc.
  boa/
    mod.rs      module root
    types.rs    BoaTypes (behind feature = "boa")
    engine.rs   BoaEngine (behind feature = "boa")
  jsc/
    mod.rs      module root
    sys.rs      raw FFI bindings to JavaScriptCore
    types.rs    safe JSC wrapper types
    engine.rs   JscTypes + JscEngine (behind feature = "jsc")
```

## Spec Documentation

Every method on `JsEngine<T>` has **only** the ECMA-262 spec anchor URL in its
doc comment.  Zero prose.  Examples:

- `/// <https://tc39.es/ecma262/#sec-toboolean>`
- `/// <https://tc39.es/ecma262/#sec-tonumber>`
- `/// <https://tc39.es/ecma262/#sec-get-o-p>`

The spec IS the documentation.  The trait method name mirrors the spec operation
name exactly.  If the name is not enough context, the algorithm lives in the spec.

## Design

The `JsEngine<T>` trait is a faithful Rust projection of ECMA-262's abstract
operation surface — the operations HTML and WebIDL cite when they say "call the
abstract operation X".

- **`JsTypes`** — pure associated-type bundle: all ECMAScript language types (§6.1)
  and object subtypes by internal slot profile.
- **`JsEngine<T>`** — operations grouped by spec chapter (§7.1, §7.2, §7.3, §9.3,
  §9.6, §16, §25, §27).
- **`Completion<T, Ty>`** — `Result<T, Ty::JsValue>`; isomorphic to spec Completion Record.
- **HTML host hooks** (`HostHooks<T>`) — constructor-time configuration, not trait methods.
