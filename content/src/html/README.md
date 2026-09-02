# content/src/html

`content/src/html` owns HTML parser integration, document lifecycle work, navigation helpers, and HTML global-object [platform objects](https://webidl.spec.whatwg.org/#dfn-platform-object) such as `Window` and `GlobalScope`.

- Keep DOM-tree entry points under `content/src/html/html_dom_tree.rs`, and route per-element hooks from there into element modules.
- Keep iframe bindings and iframe processing algorithms together in `content/src/html/html_iframe_element.rs` as free functions over content-process state (`ContentProcess`).
  `contentDocument` is unimplemented: the getter returns null even for a
  same-origin child navigable whose document lives in this content process
  (`contentWindow` resolves and the iframe `load` event does fire).
- Keep helper names aligned with the corresponding HTML algorithm anchors, and prefer explicit error returns or `debug_assert!` plus safe early returns over sentinel ids.
- Trigger parser-discovered iframe work from document-load parsing completion.
- Use the `web_standards` extension (`spec_lookup`) with `https://html.spec.whatwg.org/` to read the HTML spec.

## Structured clone (`structured_data/`)

`structured_data/` splits the safe passing of structured data between the
generic algorithms (`safe_passing_of_structured_data.rs`) and the
per-platform-object parts (`messageport.rs`); see its `README.md`.  The
gotchas below apply to the generic algorithms.

### String round-tripping — use UTF-16 units, never a display-escaped string

Strings are serialized as raw UTF-16 code units. Any display/escaping
conversion (one that replaces unpaired surrogates with literal `\uXXXX`
escape sequences) corrupts strings like lone surrogates (`\uD800`, `\uDC00`).

**Correct serialization:**
```rust
let utf16_units: Vec<u16> = ec.js_string_to_rust_string(&s).encode_utf16().collect();
```

**Correct deserialization:**
```rust
let js_string = ec.js_string_from_str(&String::from_utf16_lossy(&utf16_units[..]));
```

### RegExp source — `[[OriginalSource]]` vs the escaped getter

The `source` accessor on RegExp applies `EscapeRegExpPattern` (spec 22.2.3.2.5),
which escapes `/`, `\n`, `\r`, `\u2028`, and `\u2029`. Passing the escaped form
back to the RegExp constructor produces a different pattern. Always store the
raw `[[OriginalSource]]`: read `ec.get_regexp_source` and reverse the escaping
with `unescape_regexp_source()`.

### Error "message" — `[[GetOwnProperty]]`, not `[[Get]]`

The spec step for Error serialization (step 17.4) uses `[[GetOwnProperty]]` for
the "message" property — this checks only own data descriptors, ignores the
prototype chain, and does not invoke accessors. Using `EcmascriptHost::get`
(which is `[[Get]]`) is wrong. Use `ec.get_own_property` and read the value
from the data descriptor:
```rust
let msg_key = ec.property_key_from_str("message");
let msg_desc = ec.get_own_property(object.clone(), msg_key)?;
let message: Option<String> = match msg_desc {
    Some(ref desc) if desc.value.is_some() => desc
        .value
        .clone()
        .map(|v| ec.to_rust_string(v))
        .transpose()?,
    _ => None,
};
```

### EnumerableOwnProperties — filter by enumerability

The spec uses `EnumerableOwnProperties(value, "key")`, which returns only
enumerable own property keys. `ec.own_property_keys` returns ALL own keys
(including non-enumerable ones like `length` on arrays). Always check
enumerability through `ec.get_own_property`:
```rust
let keys = ec.own_property_keys(object.clone())?;
// ...for each key:
let desc = ec.get_own_property(object.clone(), key.clone())?;
let enumerable = desc.as_ref().and_then(|d| d.enumerable).unwrap_or(false);
```

### Wrapper objects — Boolean/Number/String/BigInt

When serializing, check for `[[BooleanData]]` / `[[NumberData]]` / etc.
internal slots (steps 7–10). When deserializing, create wrapper *objects*
with the correct prototype (steps 6–9), not primitive values — construct
through the realm's intrinsic constructor:
```rust
let num_val = ec.value_from_number(*n);
let obj = ec.construct(intrinsics.number.clone(), &[num_val], None)?;
value = Types::value_from_object(obj);
```

### Error cause — serialize custom data

The spec says "User agents should attach a serialized representation of any
interesting accompanying data." The `cause` property (ES2022) was added as
an optional `Box<SerializedRecord>` to the `Error` variant.

## Algorithm split: content process vs user agent

Many HTML algorithms (navigation, window.open, iframe creation) span both the
content process (which runs JS and owns DOM state) and the user agent (which
owns the navigable tree, browsing contexts, and event-loop dispatch). The
split is:

| Side | Owns | Runs |
|------|------|------|
| **Content** | Document, Window, JS `Context`, `GlobalScope` | Document-owning algorithm steps: URL parsing, feature tokenization, noopener computation, rules-for-choosing-a-navigable (local subset), document creation |
| **User agent** | Navigable tree, browsing contexts, browsing context groups, agents, event loops, session history | Navigable-owning algorithm steps: find-by-target-name (cross-process), new-traversable creation (non-window.open), opener tracking, beforeunload, navigation fetching |

When an algorithm crosses this boundary, the side that hits its limit sends an
IPC message and the other side continues. The IPC ordering guarantee (per
content process, messages arrive in order) makes this safe.

### Opener tracking for auxiliary browsing contexts

The content process does not track opener relationships — those are purely
UA-side state (`BrowsingContext.opener_browsing_context`, set by
`setup_opener_for_window_open`). The opener is only used for:
- Navigation policy (e.g., `target=_blank` with `rel=opener`)
- `window.opener` JS property (not yet implemented)
- Popup blocking

## Window IDL members (`window.rs`)

Every Window interface member is implemented as a `Window` method in
`content/src/html/window.rs`, following the spec's getter/method steps with
verbatim `// Step N:` comments (`self_value` implements the `self` getter
steps, `top_value` the top getter steps, `close` the `close()` method steps,
…).  The getters that read realm state (`window`/`frames`/`self` — "return
this's relevant realm.[[GlobalEnv]].[[GlobalThisValue]]") route through
`content/src/webidl/realm.rs::relevant_realm_global_this_value`, which owns
the JS-side read.  Members whose state is user-agent-only (navigable target
name, opener, closed, document-tree child navigable count) return placeholder
values from the domain methods with a `// Note:`.

Both bindings files are thin glue over these methods:

- `content/src/js/bindings/html/window.rs` — the Window interface (exposed
  on the global object and reached by the proxy's `[[Get]]` trap for
  same-content-process windows via OrdinaryGet on the Window).
- `content/src/js/bindings/html/windowproxy.rs` — the WindowProxy platform
  object's member set, which is only reached for cross-content-process
  windows (no local Window); the same member names on the Window interface
  shadow them for same-content-process windows.

Each binding function downcasts the receiver, resolves the local Window
(`local_window_domain` / `window_domain_from`), calls the domain method, and
wraps the result.  The cross-content fallbacks in the WindowProxy bindings
return placeholder values for state that lives in another content process.

### `window.open` cannot reach a cross-origin destination

The navigate algorithm's "allowed by sandboxing to navigate" check is
approximated in `window_open_steps` by comparing the destination's origin
with the source document's, throwing a "SecurityError" DOMException when they
differ, so a cross-origin popup (`window.open("https://other.example/")`)
throws instead of opening.  Lifting this needs the sandboxing flag set and
the target snapshot params the check is defined over.

## WindowProxy (`windowproxy.rs`)

<https://html.spec.whatwg.org/#the-windowproxy-exotic-object>

### Current implementation: one WindowProxy mechanism

### Current implementation

The identity handed to JavaScript is an ECMAScript Proxy whose target is a
[`WindowProxy`](windowproxy.rs) platform object tied to the navigable (one
per (realm, navigable), cached on the realm's GlobalScope), so
`event.source === iframe.contentWindow` holds.  The domain `WindowProxy`
holds the navigable's active Window — the domain struct, not a JS object —
in a `backing` cell when it lives in this content process (same agent
cluster); the proxy traps then delegate property access to that Window —
the local behavior (`window.open` results, `iframe.contentWindow`, and the
message event's `source` all resolve property gets/sets against the local
Window, e.g. `w.location = url` reaches the target's Location binding).
When the navigable's document was created in another content process, the
backing is `WindowProxyBacking::CrossContentProcess` and the traps branch on
`is_platform_object_same_origin`, delegating to the cross-origin abstract
operations (`CrossOriginGet`, `CrossOriginSet`, `CrossOriginGetOwnPropertyHelper`,
`CrossOriginPropertyFallback`, `CrossOriginOwnPropertyKeys`) — `postMessage`
routes through the user agent (steps 1–7 locally, user-agent routing for
step 8), and the remaining members resolve off the platform object's
prototype.

The `SameContentProcess` variant carries the domain `Window` and the
Window's JS object handle.  The handle is deliberately rooted (not a
cppgc-traced edge): the WindowProxy's backing must stay usable across the
navigation-commit garbage collection that runs when the old document is
destroyed, and a cppgc-traced edge read back from the cell after that
collection is not reliably usable on the V8 backend (the materialized
handle can point at a swept object — reproducible in `js_engine`'s own
`associated_platform_cells_survive_forced_gc` when the cell value is read
back and used after the forced gc).  The root keeps the window alive for
exactly as long as the proxy references it, and the navigation-commit
re-pointing clears it (releasing the root) once the navigable's document is
created in another content process.  The `backing` cell is shared by every
clone of the `WindowProxy` (the realm's cached copy and the platform object
created from it), so navigation commit re-points the cell in place and the
traps read the new backing without a per-access cache lookup; the cached
entry stores the domain `WindowProxy` and the JS object (the ECMAScript
Proxy) handed to JavaScript, created lazily on first access.

Callable results of the [[Get]] trap are wrapped so they are invoked with
`this` set to the resolved receiver — the Window for a same-content-process
window, the proxy's platform object for a cross-content-process window —
because the Call expression uses the Proxy itself as `this` and the member
functions downcast their receiver.  Constructors are handed back unwrapped:
interface objects reached through the proxy (`w.DOMException`) must keep
their identity, and a script function called through the proxy gets the
proxy as `this` like in browsers.

Cross-realm property access in V8 is gated by the context security token;
the engine installs a shared token on every context so same-origin windows
can reach each other's globals, and native callbacks run in their creation
realm (the callback machinery switches the engine's realm state), so
invoking the target window's methods through the proxy — `w.location = url`
via the `[PutForwards=href]` Location attribute, `w.open(...)`, timers, and
gets/sets on the target's globals — runs in the target realm and works.

### Lifecycle: navigation commit is the proxy transition

Per the spec, a browsing context (navigable) has **one** WindowProxy
identity, and *"when the browsing context is navigated, the Window object
wrapped by the browsing context's associated WindowProxy object is
changed"* (§7.2.3).  The proxy's `backing` cell is exactly that wrapped
Window, and navigation commit updates it:

1. **Same-content-process backing** — while the navigable's active document
   is in this content process (same agent cluster), the proxy is backed by
   that document's Window: `backing` is `SameContentProcess` and the traps
   delegate property access to it.
2. **Navigation commit (the old document unloads)** — when the old
   document is destroyed in this process (`destroy_document`), every cached
   WindowProxy for that navigable is re-pointed:
   - if a new document for the navigable is already active in this process
     (same-process navigation), the backing is re-pointed at the new
     document's Window (cross-realm proxy — the spec's §7.5.1 step 6
     reuses the initial about:blank Window itself for same-origin
     navigations: `ContentProcess::initialise_the_document_object` keeps
     the initial about:blank realm/Window and re-points it at the new
     document, so the backing is unchanged and only the proxy identity
     stays; the cross-realm re-point below is for navigations that do not
     qualify for step 6, where the new document gets a fresh realm/Window);
   - if the navigable's document was created in another content process
     (cross-origin navigation), `backing` becomes `CrossContentProcess`
     (cross-process forwarding via the user agent) while keeping the
     proxy's identity.

The user agent routes `DestroyDocument` to the event loop that owns the
document (`command_sender_for_document`), not the traversable's current
event loop: after a cross-process navigation the traversable has moved to a
new event loop, and routing by traversable would send the destroy to the
wrong content process — leaving the old document (and the WindowProxy's
same-content backing) alive in the old process forever.

Note: the user agent keeps a traversable whose active document is initial
about:blank on the opener's event loop for its first URL navigation
(`initialise_the_document_object`), so the first navigation of a
`window.open`'d popup stays in the same process and re-points the backing;
the window is created in another content process (and the backing becomes
cross-content) on later cross-origin navigations and for child navigables
whose parent document is cross-origin.

### Agents, processes, and realms

The taxonomy comes from the spec's agent model, not from our process
layout.  Windows are placed into agents by <https://html.spec.whatwg.org/#obtain-a-similar-origin-window-agent>
(defined in §8.1.2.2, used at §7.3.2.1 "creating browsing contexts" step 9
and §7.5.1 "shared document creation infrastructure" step 7.4):

- **Agent cluster** — the spec's idealized "process boundary" (§8.1.2.2:
  *"the agent cluster concept is an architecture-independent, idealized
  process boundary"*).  An agent cluster holds one similar-origin window
  agent.  Windows in the same cluster: a Window and a same-origin-domain
  iframe it created, and a Window and a same-origin-domain Window that
  opened it (opener/opened).  Windows with no opener or ancestor
  relationship are in **different** clusters *even when same-origin*.
- **Similar-origin window agent** — the spec unit our content process is
  the concrete realization of: one content process hosts one agent cluster
  with one similar-origin window agent (`AgentCluster.similar_origin_window_agent`
  and `AgentClusterKey` in `user_agent/src/user_agent.rs`).  Same-cluster
  windows are same-process; different-cluster windows are cross-process.
  In particular, cross-origin windows are always cross-process, and an
  auxiliary browsing context (`window.open`) is same-origin-domain-related
  to its opener, so it shares the opener's cluster and process.
- **Realm** (V8 context) — an engine detail, orthogonal to the agent
  model: every Window is its own realm even within the same agent, and V8
  gates property access on another context's global object on
  security-token equality.  The engine installs a shared security token on
  every context, so same-cluster (same-process) property gets/sets through
  the WindowProxy resolve against the local Window.  Native callbacks run
  in their creation realm (the callback machinery switches the engine's
  realm state), so method calls through the proxy (e.g. `w.location = url`
  reaching the Location binding's navigation) run in the target realm and
  work.

What a WindowProxy access involves therefore splits as:

- **Same cluster (same process)**: the proxy resolves the target window's
  realm locally (property gets/sets work with the shared security token;
  method invocation runs the target realm's native bindings).
- **Cross cluster (cross process)**: the proxy's backing is
  `CrossContentProcess` and the traps delegate to the cross-origin abstract
  operations, which resolve the platform object's member set; `postMessage`
  routes through the user agent (the remaining members need selective-access
  forwarding, gap 1 below).

### Remaining gaps

**1. Cross-cluster selective access is not wired.**  When the target
navigable lives in another agent cluster (another content process), the
WindowProxy must give selective access to the remote window: `postMessage`
already routes through the user agent, and the remaining members
(`document`, `location`, `name`, …) must be forwarded
to the target process the same way.  The domain `WindowProxy` is what makes
this possible — the proxy is a navigable id plus a backing and a forwarding
policy, so it can hand any member off to the user agent.

**2. The cross-content WindowProxy exposes a fixed member set.**  The
cross-content WindowProxy exposes the Window members the current features
need rather than delegating every property access; members not in its set
(e.g. `setTimeout`, `onmessage`, or script-defined globals on the target
window) are absent until the selective-access forwarding is wired.

**3. Child navigable properties (array-index and named).**  The spec requires
WindowProxy to expose child browsing contexts by numeric index (`window[0]`,
`window[1]`) and by name.  This requires tracking the document-tree child
navigables on the Document, which is not yet implemented.

**4. WindowProxy identity is per realm.**  `create_window_proxy` resolves
the current realm's own navigable to the realm's global object (the spec's
[[GlobalThisValue]] of a Window realm is that navigable's WindowProxy), so
`window.top === window`, `window.parent === window`, and
`window.open("", "_self") === window` hold.  Every other navigable gets a
proxy cached per (realm, navigable), which means the same navigable seen
from two realms is two objects: `iframe.contentWindow.top === window` does
not hold like in browsers.  `Window::top_value`/`parent_value` consult the
navigable hierarchy (`top_level_traversable_id`/`parent_traversable_id`) and
create the resolved navigable's WindowProxy with no local window, so the
proxy's backing is `CrossContentProcess` (its members resolve through the
platform object's member set, and its cross-origin `top`/`parent`/`self`
return the proxy itself) even when that navigable's window lives in this
process.

**5. `name`, `opener`, `closed` are stubs.**  The navigable target name is
tracked by the user agent (`traversable_target_names` in
`user_agent/src/user_agent.rs`), the opener relationship by
`BrowsingContext.opener_browsing_context`, and the is-closing flag by no
process yet; the domain methods (`Window::name_value`, `opener_value`,
`closed_value`, `close`) return placeholder values with `// Note:`
annotations until that state is sent to the content process or forwarded.

## Workers (`worker.rs`, `worker_global_scope.rs`, `dedicated_worker_global_scope.rs`, `dedicated_worker_agent.rs`)

Dedicated workers run on a native thread nested to the content process
hosting the owner realm, and talk to their owner over direct crossbeam
channels that bypass the MessagePort machinery (the spec's implicit
outside/inside port pair is documented per-field on `Worker` in `worker.rs`
and on `WorkerGlobalScope` in `worker_global_scope.rs`).  Worker
creation and termination never involve the user agent: the `Worker`
constructor reports to the content process's worker manager through its
realm's GlobalScope, which spawns the thread and stores its command channel
and join handle (joined on teardown and shutdown).  The spec-annotated
algorithms live in `worker.rs` (constructor steps, terminate a worker),
`worker_global_scope.rs` (the `WorkerGlobalScope` common interface, whose
methods also carry the dedicated members — name, postMessage, close, the
inbound message queue — since the dedicated realm's platform object is a
`WorkerGlobalScope` domain struct, and the close-a-worker and
import-scripts algorithms), `dedicated_worker_global_scope.rs` (the
`DedicatedWorkerGlobalScope` interface's registry marker struct), and
`dedicated_worker_agent.rs` (run-a-worker and the agent's event loop).

Known gaps:

- **importScripts only supports data: URLs.**  Fetches of other URL schemes
  are asynchronous in this architecture (through the net process), and
  importScripts is synchronous; non-data URLs throw a NotSupportedError.
- **Module workers are evaluated as classic scripts.**  "Fetch a module worker
  script graph" (run-a-worker step 12's module branch) is not implemented;
  `type: "module"` workers run their source as a classic script.
- **`error` events on the Worker object are fired directly, not as queued
  global tasks**, matching the document lifecycle commands' existing
  deviation (see `content/README.md`).
- **Worker runtime errors are conflated with fetch/parse failures, and
  ErrorEvent is missing.**  An evaluation error in the worker script (parse
  failure or a top-level exception at run-a-worker step 12.13) fires a plain
  `error` event at the Worker object; per report an exception (runtime script
  errors) it should first fire an error event at the worker global scope
  (self.onerror) and only reach the Worker object when unhandled, and the
  event must be an ErrorEvent with message/error/filename/lineno/colno.
  Runtime errors thrown inside worker *tasks* (e.g. an onmessage handler
  throwing) are not reported at all: no error event fires at the worker
  global scope and nothing reaches the Worker object — the engine swallows
  the exception.  A script parse failure should abort the worker at step
  12.4, but the engine's evaluate combines parse and run, so the two are not
  distinguished.  (`dedicated-worker-parse-error-failure.html` passes: it
  asserts the error handler receives a single plain-`Event` argument, which
  the direct event does satisfy.)
- **The default WPT run covers document-side dedicated-worker tests.**
  `tests/wpt/include.ini` selects the dedicated-worker tests under `workers/`
  that run their subtests in the document (Worker construction and
  messaging, WorkerLocation/WorkerNavigator, timers and close(), data: URL
  and nested workers, importScripts of data: URLs); tests that fail are
  selected there and disabled in `tests/wpt/meta` with the specific gap.
  Still excluded: worker-global tests (`.worker.` files and
  `fetch_tests_from_worker` need worker-side testharness, which imports
  testharness.js over http — see the importScripts gap above;
  `webmessaging/without-ports/025.html` is disabled for that reason), and
  shared-worker, module-worker, and worker tests that additionally need an
  unrelated unexposed feature (Blob, URL, fetch/XHR, canvas, SharedArrayBuffer,
  iframe/session-history navigation, redirects).
- **The worker realm does not expose `performance`.**  The
  WindowOrWorkerGlobalScope `performance` member is missing on worker
  globals, so `self.performance.now` is undefined (`WorkerPerformanceNow.html`
  is disabled for it).
- **`terminate()` does not promptly stop delivery of already-queued
  messages.**  Messages queued on the worker's outside port are still
  dispatched after `terminate()` returns, so the timing-sensitive
  `Worker_terminate_event_queue.htm` and `constructors/Worker/terminate.html`
  fail (the infinite-runner and evaluation-time terminate tests pass).
- **Worker script URLs are resolved against the document URL, not the
  document base URL**, so a worker created under a `<base href>` fails to
  load (`constructors/Worker/use-base-url.html` is disabled for it).
- **The Worker constructor and `postMessage` do not enforce their required
  arguments**: `new Worker()` and `worker.postMessage()` with no arguments
  do not throw a TypeError (`constructors/Worker/Worker-constructor.html`
  and `Worker-multi-port.html` are disabled for it).
- **Platform objects report `[object Object]` from `Object.prototype.toString`**
  (no `@@toStringTag` is exposed; affects every platform object — Worker,
  MessageEvent, Event, DOMException — on the V8 backend), so tests that
  assert class strings (`Worker_basic.htm`'s constructor subtest,
  `message-event.html`, `AbstractWorker.onerror.html`) are disabled.
- **Owner-set lifetime management is minimal.**  Terminate-on-owner-document-
destroy is wired (`destroy_document` terminates its workers, as does a
closing owner worker for its nested workers); the spec's
protected/permissible/suspendable monitoring is not.
- **Shared workers are not implemented.**  They need new-agent-cluster
  allocation (which must happen in the user agent) and the UA-side instance
  lookup by (origin, name); the dedicated-only implementation folds
  `WorkerGlobalScopeKind` into the dedicated global scope.

## Related documentation

- `content/src/webidl/README.md` — Boa platform object integration, exotic object pattern
- `content/src/js/README.md` — Boa integration specifics (Context ownership, bindings)
- `content/README.md` — Content-crate overview
- `user_agent/src/user_agent.rs` — `create_new_top_level_traversable_from_content`, `create_new_top_level_traversable`, `the_rules_for_choosing_a_navigable` (UA side), `setup_opener_for_window_open`, and the agent model (`AgentCluster`, `AgentClusterKey`, `similar_origin_window_agent`)
- `ipc_messages/src/content.rs` — `NewTraversableInfo`, `CreateEmptyDocument`, `NavigateRequest`
- `content/src/html.rs` — `the_rules_for_choosing_a_navigable` (content side), `navigate`, `ChosenNavigable`
- `content/src/html/window.rs` — `Window::open`, `window_open_steps`
- `content/src/html/global_scope.rs` — `create_auxiliary_context_document`, `set_navigable_hierarchy`
