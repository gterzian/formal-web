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
│         Rust modules            │  algorithms, called via FFI ◀─┐
└─────────────────────────────────┘                               │
                                        Lean impl ──FFI──────────▶┘
```

- **TLA+ specs** for high-level design.
- **Lean specs** model labeled transition systems — *what* the system does, as state transitions, without committing to an implementation.
- **Lean implementations** use Lean's IO monad to implement the specs — *how* the system does it, with all concurrent logic handled here.
- **Rust modules**, invoked from Lean via FFI, handle sequential and embarrassingly parallel algorithms. Each module is a referentially transparent function treated as atomic from Lean's perspective, and can be verified separately against its input/output contract.

Lean handles all concurrent aspects of the engine; Rust handles modular sequential algorithms.

## Motivation

Formal specs pair well with AI-assisted coding: given a Lean spec, an AI can write an implementation and prove it respects the spec. Given a natural language description, an AI can draft the spec itself, shifting the human's role to reviewing rather than writing.

## Example

In formal-web, all tabs run on the main process. Each tab gets a separate process running the event loop in Rust — managing the DOM, calling into JavaScript, and producing a display list. All coordination between the event loop and the rest of the engine is handled in Lean on the main process.

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
rustup run 1.92.0 cargo check --manifest-path ffi/Cargo.toml     # Rust FFI crate
```

## Run

```bash
lake exe formal-web
```

Starts the Rust `winit` event loop and the Lean runtime workers. Loads the demo page from `artifacts/StartupExample.html`.