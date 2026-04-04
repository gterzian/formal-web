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
- **Rust embedder and content processes** handle window-system integration, DOM/runtime execution, and recorded paint-scene production. Lean starts them through a thin FFI layer and coordinates them with explicit messages.

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
lake build                                                        # full build
lake build FormalWeb.UserAgent                                    # user-agent module only
rustup run 1.92.0 cargo check --manifest-path ffi/Cargo.toml      # Lean-facing Rust staticlib
rustup run 1.92.0 cargo check --manifest-path embedder/Cargo.toml # main-thread embedder runtime library
rustup run 1.92.0 cargo check --manifest-path content/Cargo.toml  # child content-process binary
```

`lake build` builds the Lean code, the Rust static library under `ffi/`, and the `content` child executable, then copies the child binary into `.lake/build/bin/` so the embedder can spawn it at runtime.

## Run

```bash
lake exe formal-web
```

Starts the Rust embedder event loop plus the Lean runtime workers. As event loops come up, the embedder spawns the `content` child executable and communicates with it over `ipc-channel`. The startup flow loads the demo page from `artifacts/StartupExample.html`.

You can also run the built executable directly:

```bash
./.lake/build/bin/formal-web
```

This expects the sibling child binary `./.lake/build/bin/content` produced by `lake build` to still be present.