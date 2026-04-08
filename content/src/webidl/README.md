`content/src/webidl` stores the Web IDL algorithms that sit between DOM-facing logic and Boa's ECMAScript primitives.

- Callback-interface conversions and `call a user object's operation` belong here.

- This layer should depend on abstract `Get` / `IsCallable` / `Call` hooks instead of reaching into Boa directly.

- DOM event dispatch should call into this layer for listener callback invocation instead of calling Boa functions directly.