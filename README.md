# formal-web
### The Formalized Web Engine

Modern Web engines are among the most complex concurrent systems in existence. Cross-process coordination, race conditions, and the sheer scale of the Web platform have made browser engines a persistent source of security vulnerabilities and correctness bugs. formal-web is an attempt to address this at the root: by formalizing the Web and building a verified Web engine on top of that foundation.

The diagram below gives an overview of the architecture.

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

- **TLA+ specs** for high-level design thinking.
- **Lean specs**, written in a TLA-like style to model labeled transition systems — capturing *what* the system does as state transitions, without committing to an implementation.
- **Lean implementations**, using Lean's IO monad to implement the specs — capturing *how* the system does it, with all concurrent logic handled here.
- **Rust modules**, invoked from Lean via FFI.

The Rust modules are restricted to sequential logic, with the exception of embarrassingly parallel workloads. Each module is a black box — a referentially transparent function from a given input to a given output — and is treated as atomic from Lean's perspective. These modules can be left unverified or verified separately against their input/output contract.

The division of responsibility is principled: Lean handles all concurrent aspects of the engine, while Rust handles modular sequential and embarrassingly parallel algorithms. This keeps the concurrency model entirely within a language and proof system designed to reason about it.

## Motivation
The immediate historical context for this project is the rise of AI-assisted coding. A central conviction of formal-web is that formal methods are what allow these tools to be used to their full potential. Given a formal Lean spec, an AI can write the implementation and produce a proof that it respects the constraints of the higher-level spec — work that would otherwise require significant human effort. 

Given vague natural language input, an AI can also draft the spec itself. In that case, the human's role shifts from writing to reviewing — checking that the spec faithfully captures the intended behavior. This is a much more tractable task, and a robust one: a reviewed formal spec is a precise, machine-checkable record of intent.

## Example

All Web engines today run some kind of *content process* — roughly, one per tab. These processes perform a significant amount of cross-process coordination, which is a major source of complexity and bugs.

In formal-web, the engine would manage all tabs on the main process. For each tab, a separate process would run the event loop in Rust — managing the DOM, calling into JavaScript, and producing a display list. All coordination between the event loop and the rest of the engine would be handled in Lean on the main process.

This architecture keeps Rust's role narrow and well-defined, while Lean owns the coordination logic that is hardest to get right.

> **Note:** One could imagine going further and running the event loop, DOM management, and layout entirely in Lean — the DOM's recursive structure maps naturally onto inductive types, making it a strong candidate for verified Lean code. The use of Rust here is a practical choice: it allows existing code from [Servo](https://servo.org) to be reused, rather than reimplementing a rendering engine from scratch.

## Build

The project uses Lean via Lake for the main build, and a pinned Rust toolchain for the FFI crate.

Prerequisites:

- `elan` installed so the Lean toolchain in `lean-toolchain` is picked up automatically.
- `rustup` installed.
- Rust toolchain `1.92.0` installed: `rustup toolchain install 1.92.0`
- On macOS, Xcode and the macOS SDK available at the path referenced in `lakefile.lean`.

Build the full project:

```bash
lake build
```

Build just the user-agent Lean module:

```bash
lake build FormalWeb.UserAgent
```

Check the Rust FFI crate directly with the pinned toolchain:

```bash
rustup run 1.92.0 cargo check --manifest-path ffi/Cargo.toml
```

## Run

Run the demo executable through Lake:

```bash
lake exe formal-web
```

This starts the Rust `winit` event loop and the Lean runtime workers together. The current startup path loads the checked-in demo page from `artifacts/StartupExample.html`.