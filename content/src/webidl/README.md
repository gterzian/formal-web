# content/src/webidl

`content/src/webidl` stores the shared Web IDL algorithms that sit between DOM, HTML, and Streams code and the ECMAScript operations used by the current runtime.

- Callback-interface conversion, `call a user object's operation`, and promise helpers belong here.
- This layer should depend on abstract `Get`, `IsCallable`, and `Call` hooks instead of reaching into engine-specific context APIs directly.
- Keep the context-backed adapters for those hooks here so DOM, HTML, and Streams code can delegate instead of reimplementing callback glue locally.
- Promise helpers here should follow the Web IDL promise algorithms, including `#js-promise-manipulation`, `#a-promise-resolved-with`, `#a-promise-rejected-with`, and `#js-to-promise`.
- DOM event dispatch and other callback sites should call into this layer instead of calling Boa directly.
- The spec source is `web_standards/WebIDL.html`.