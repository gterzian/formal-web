# TLS Design Ideas

This note captures the current design direction for the formal web-engine model.

## Goals

- Model the HTML-standard state and algorithms directly in Lean.
- Add a higher-level labeled transition system later, for concurrent and externally interesting state transitions.
- Build an actual executable implementation using tasks, channels, or a similar concurrency substrate.
- Prove that the executable implementation refines, simulates, or otherwise faithfully implements the LTS.

## Layering

The model should be split into at least three layers.

### 1. Data layer

This layer defines the core state carried by the browser model.

Examples:

- `UserAgent`
- `TopLevelTraversable`
- document state
- navigables
- task queues
- message queues

This layer should stay close to the spec terminology.

### 2. Algorithm layer

This layer models spec algorithms as pure Lean functions.

Typical shape:

```lean
State -> Input1 -> Input2 -> State × Result
```

or, when no value is produced:

```lean
State -> Input -> State
```

The important point is that these helpers are not automatically LTS steps. They are deterministic or partially specified state transformers used by larger semantics.

### 3. Concurrency/LTS layer

This layer introduces a labeled transition relation over a larger operational state.

The LTS should model only transitions that are semantically interesting at the concurrency boundary, such as:

- dequeuing and running a task
- delivering a message
- handling a navigation request
- processing a timer firing
- processing a network completion
- external user-driven events

Internal helper calls can occur inside one such transition without each becoming its own LTS step.

## Why Not Make Every Helper an LTS Step

Making every helper call a separate LTS transition would be too low-level and would expose implementation structure instead of the intended concurrent semantics.

That would create several problems:

- the transition system becomes noisy
- proofs become harder because each semantic action is fragmented into many tiny steps
- harmless refactorings of helper functions would change the LTS unnecessarily
- the model would be tied to a particular decomposition of algorithms instead of the intended behavior

So the LTS should sit above helper functions, not coincide with them.

## Recommended State Split

It is likely that `UserAgent` will remain one important component of state, but the eventual LTS state should probably be larger than `UserAgent` alone.

A plausible shape is:

```lean
structure BrowserState where
  userAgent : UserAgent
  schedulers : ...
  taskQueues : ...
  channels : ...
  pendingIO : ...
```

`UserAgent` then represents spec-level browser state, while `BrowserState` represents the whole executable machine state.

## Labels and Messages

The action labels of the LTS should correspond to meaningful externally visible or scheduler-visible events.

Possible examples:

- `RequestCreateTraversable ...`
- `RunNavigationTask ...`
- `DeliverMessage ...`
- `CompleteFetch ...`
- `TimerFired ...`
- `Tau` for internal, intentionally hidden steps if needed

The runtime implementation will likely have concrete message types flowing over channels. Those message types are good candidates for inductive enums in Lean.

That suggests a useful discipline:

- define message/action types explicitly as Lean inductive types
- use them both in the implementation model and in the LTS labels where appropriate
- prove that consuming or producing those messages preserves the intended relation between runtime and spec state

## Refinement Direction

The long-term proof story should be something like:

1. Define pure spec algorithms over spec state.
2. Define an LTS over higher-level concurrent state.
3. Define an executable task/channel-based machine.
4. Define a refinement or simulation relation between runtime state and LTS/spec state.
5. Prove that each runtime step corresponds to one or more allowed LTS steps.

Depending on how much runtime detail is exposed, the proof may use:

- forward simulation
- backward simulation
- stuttering refinement
- trace inclusion

Stuttering refinement is likely to be useful if one externally visible browser action requires multiple internal runtime scheduling steps.

## Determinism and Concurrency

The eventual system should not assume determinism.

Concurrency, scheduling choice, message arrival order, timers, and network completion will naturally introduce nondeterminism. The LTS should allow that by using a relational transition definition rather than a single-step next-state function.

Helper algorithms can still be deterministic in isolation. That is not in conflict with a nondeterministic global LTS.

## Suggested Development Order

1. Continue modeling spec data structures.
2. Continue modeling spec algorithms as pure helpers.
3. Introduce explicit identifiers for major entities such as traversables, documents, tasks, and channels.
4. Add a separate transition-system module.
5. Define a first browser-level LTS with a small number of important labels.
6. Add a runtime machine model based on tasks/channels.
7. Prove a simulation or refinement theorem relating the runtime machine to the LTS.

## Short-Term Guidance

For now, optimize for:

- clean spec-facing data definitions
- clean algorithm definitions with close spec traceability
- signatures that thread state in a way that will be easy to wrap inside an LTS later

Avoid overcommitting too early to:

- a specific scheduler design
- a specific channel API
- making every helper function directly visible as a transition label

The key design bet is that the algorithm layer should be reusable by both the future LTS semantics and the future executable runtime.