# Boa backend (`js_engine/src/boa`)

Opt-in engine: a git dependency on `boa-dev/boa`. Boa has no native
WebAssembly; the `wasm` feature wires the Wasmtime-based WebAssembly
implementation for this backend. (V8 implements WebAssembly
natively — see `../v8/README.md`.)

## Build

```bash
# Build js_engine crate
rustup run 1.94.0 cargo build --release --no-default-features --features boa -p js_engine

# Build content binary with Boa
rustup run 1.94.0 cargo build --release --no-default-features --features boa,media -p content --bin formal-web-content

# Run a single WPT test via Boa
rustup run 1.94.0 cargo run --release --no-default-features --features boa,media -- wpt dom/nodes/Element-hasAttribute.html
```

## Known issues

**A cross-realm native call is entered in the caller's realm.**  When script
in realm A calls a native function belonging to realm B (e.g.
`popup.open(...)` through a WindowProxy), the platform objects the call
creates come from realm A's interface registry: a SecurityError thrown by
`popup.open` has realm A's `DOMException` as its constructor, where the V8
backend (and browsers) give realm B's.  Observed through
`e.constructor === window.DOMException` on the caller side; the callee-realm
switch performed by the V8 callback machinery has no Boa counterpart.

## Known gaps

Backend methods that return a plausible value instead of implementing the
full spec algorithm, plus the one generic operation Boa cannot express:

- `is_array` does not follow a Proxy's `[[ProxyTarget]]`; Boa's public object
  API exposes no target accessor.  `review_findings::is_array_sees_through_proxy`
  is ignored on Boa for this reason (V8 passes).
- `set_value_in_buffer` with `Float16` throws: the `float16` feature is off.
- `species_constructor` always returns the default constructor.
- `is_fixed_length_array_buffer` is always true (resizable ArrayBuffers are
  not supported).
- `set_host_hooks` is a no-op (host hooks are set on the `ContextBuilder`).

`JsTypesGcExt::Reflector` for Boa is a strong `JsObject` because Boa exposes
no weak reflector here.  A cache built on it keeps its wrapper alive; the
node-wrapper cache (`content/src/html/global_scope.rs`) is therefore weak on
V8 and strong on Boa.

## WPT results

Last recorded: `executed=277 unexpected=0`.

Wasm tests are excluded from the default WPT run (opt-in `--features wasm`).
