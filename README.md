# formal-web
### A formalization of the Web.

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

## Example

All Web engines today run some kind of *content process* — roughly, one per tab. These processes perform a significant amount of cross-process coordination, which is a major source of complexity and bugs.

In formal-web, the engine would manage all tabs on the main process. For each tab, a separate process would run the event loop in Rust — managing the DOM, calling into JavaScript, and producing a display list. All coordination between the event loop and the rest of the engine would be handled in Lean on the main process.

This architecture keeps Rust's role narrow and well-defined, while Lean owns the coordination logic that is hardest to get right.

> **Note:** One could imagine going further and running the event loop, DOM management, and layout entirely in Lean — the DOM's recursive structure maps naturally onto inductive types, making it a strong candidate for verified Lean code. The use of Rust here is a practical choice: it allows existing code from [Servo](https://servo.org) to be reused, rather than reimplementing a rendering engine from scratch.