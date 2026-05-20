`content/src/webidl` stores the Web IDL algorithms that sit between DOM-facing logic and the ECMAScript operations used by the current runtime.

- Callback-interface conversions and `call a user object's operation` belong here.

- This layer should depend on abstract `Get` / `IsCallable` / `Call` hooks instead of reaching into engine-specific context APIs directly.

- Keep `IsCallable` checks aligned with the current Web IDL algorithm step by applying them to the ECMAScript value produced by that step before narrowing to a callable object for `Call`, and keep helper diagnostics generic to callback/interface algorithms rather than to one caller such as event listeners.

- Keep shared context-backed adapters for those hooks here; DOM, HTML, and Streams code should delegate to them instead of reimplementing that callback-operation glue locally.

- Keep Promise creation and rejection-reason conversion helpers here, and structure them against Web IDL "Creating and manipulating Promises" algorithms:
	- https://webidl.spec.whatwg.org/#js-promise-manipulation
	- https://webidl.spec.whatwg.org/#a-promise-resolved-with
	- https://webidl.spec.whatwg.org/#a-promise-rejected-with
	- https://webidl.spec.whatwg.org/#js-to-promise

- DOM event dispatch should call into this layer for listener callback invocation instead of calling engine functions directly.

- Spec is found under the top-level `/web_standards/WebIDL.html`

- Do not add unit-tests; use wpt tests, and add your own under tests/formal when necessary.