`content/src/webidl` stores the Web IDL algorithms that sit between DOM-facing logic and Boa's ECMAScript primitives.

- Callback-interface conversions and `call a user object's operation` belong here.

- This layer should depend on abstract `Get` / `IsCallable` / `Call` hooks instead of reaching into Boa directly.

- Keep Promise creation and rejection-reason conversion helpers here, and structure them against Web IDL "Creating and manipulating Promises" algorithms:
	- https://webidl.spec.whatwg.org/#js-promise-manipulation
	- https://webidl.spec.whatwg.org/#a-promise-resolved-with
	- https://webidl.spec.whatwg.org/#a-promise-rejected-with
	- https://webidl.spec.whatwg.org/#js-to-promise

- DOM event dispatch should call into this layer for listener callback invocation instead of calling Boa functions directly.

- Spec is found under the top-level `/web_standards/WebIDL.html`

- Do not add unit-tests; use wpt tests, and add your own under tests/formal when necessary.