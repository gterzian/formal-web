# formal-web
### A formalization of a Web engine.
The idea of formal-web is to build a formally verified Web engine.
The below diagram gives an overview of the architecture.

```
┌─────────────────────────────────┐
│         TLA+ specs              │  highest-level design thinking
├─────────────────────────────────┤
│         Lean specs              │  TLA-like Lean, high-level state machines or TLS
├─────────────────────────────────┤
│      Lean implementations       │  concurrent logic, I/O
├─────────────────────────────────┤
│         Rust modules            │  algorithms, called via FFI ◀─┐
└─────────────────────────────────┘                               │
                                        Lean impl ──FFI──────────▶┘
```

- TLA+ specs for high-level design thinking.
- Lean "specs", written in a TLA-like style to model state transitions.
- Lean "implementations", using the Lean I/O capabilities, implementing the specs.
- Rust modules called into from Lean using FFI.

The Rust modules should be sequential logic only, with the exception of embarrassingly parallel workloads.
Each such Rust module should be a black box with an input and output — modeled as atomic functions from Lean's perspective.
These can be unverified, or verified separately (for a given input and output).

In the end, Lean will be used to implement all concurrent aspects of the engine, and Rust will be used for modular sequential and/or embarassingly parralel algorithms.

---

> **Example:** All Web engines today run some kind of content process, which you can think of as a tab.
> But in this design, the engine runs a single main process that manages all tabs, and for each tab there would be a process
> only running a Rust module managing the DOM, calling into JavaScript, and producing a display list when needed. 
> That means that the event-loop for the tab, and all other concurrent aspects,
> would be part of the main process and be written in Lean.
