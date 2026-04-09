# formal-web

Web engines require complicated concurrent coordination, which formal methods can help address. formal-web formalizes the Web and builds a verified engine on top of that foundation.

```
┌─────────────────────────────────┐
│         TLA+ specs              │  highest-level design thinking
├─────────────────────────────────┤
│         Lean specs              │  labeled transition systems
├─────────────────────────────────┤
│      Lean implementations       │  concurrent logic, I/O
├─────────────────────────────────┤
│  Rust embedder + child procs   │  rendering, DOM/runtime I/O ◀─┐
└─────────────────────────────────┘                               │
                                        Lean impl ──FFI / IPC────▶┘
```

- **TLA+ specs** for high-level design.
- **Lean specs** model labeled transition systems — *what* the system does, as state transitions, without committing to an implementation.
- **Lean implementations** use Lean's IO monad to implement the specs — *how* the system does it, with all concurrent logic handled here.
- **Rust embedder and content processes** handle window-system integration, DOM/runtime execution, and recorded paint-scene production. The Rust entry point starts the Lean kernel through a thin FFI layer and coordinates with it through explicit messages.

Lean handles the engine-wide concurrent coordination; Rust handles embedder integration plus the per-event-loop content-process work.

## Motivation

Formal specs pair well with AI-assisted coding: given a Lean spec, an AI can write an implementation and prove it respects the spec. Given a natural language description, an AI can draft the spec itself, shifting the human's role to reviewing rather than writing.

## Example

In formal-web, the top-level windowing runtime lives in an embedder process. Each event loop then starts its own Rust content process, which owns the DOM/runtime work for that event loop and returns recorded paint scenes over IPC. All coordination between event loops, fetch, navigation, and the rest of the engine is handled in Lean.

> **Note:** The DOM's recursive structure maps naturally onto inductive types, so running the event loop and layout entirely in Lean is feasible. Rust is used here as a practical choice to reuse existing code from [Servo](https://servo.org).

## Build

Prerequisites:

- `elan` installed (picks up the toolchain from `lean-toolchain` automatically)
- `rustup` installed
- Rust toolchain `1.92.0`: `rustup toolchain install 1.92.0`
- On macOS, Xcode and the macOS SDK at the path referenced in `lakefile.lean`

```bash
rustup run 1.92.0 cargo run -- --help                            # root runtime entry point
rustup run 1.92.0 cargo check                                     # root Rust workspace
lake build                                                        # full Lean workspace
lake build FormalWeb.Runtime                                      # runtime Lean modules only
lake build FormalWeb.UserAgent                                    # user-agent module only
rustup run 1.92.0 cargo check --manifest-path ffi/Cargo.toml      # Lean-facing Rust staticlib
rustup run 1.92.0 cargo check --manifest-path embedder/Cargo.toml # main-thread embedder runtime library
rustup run 1.92.0 cargo check --manifest-path content/Cargo.toml  # child content-process binary
```

`cargo run` and `cargo check` at the repository root build the Rust entry point, the Lean runtime artifacts needed by `ffi`, and the `content` child executable used by the embedder. `lake build` remains available when you want the broader Lean workspace build.

## Run

```bash
rustup run 1.92.0 cargo run
```

Starts the Rust embedder event loop, initializes the Lean runtime, and starts the Lean kernel workers. As event loops come up, the embedder spawns the `content` child executable and communicates with it over `ipc-channel`. The startup flow loads the demo page from `artifacts/StartupExample.html`.

You can also run the built executable directly:

```bash
./target/debug/formal-web
```

This expects the sibling child binary `./target/debug/content` produced by the root build to still be present.

## WPT

```bash
./mach test-wpt --list html/document-isolation-policy/credentialless-cross-origin-isolated.tentative.window.js
```

`./mach test-wpt` is a thin wrapper around `cargo run -- test-wpt ...`. Use `tests/wpt/include.ini` to opt suites into a run, and place expectations under `tests/wpt/meta` using Servo-style `.ini` metadata files.