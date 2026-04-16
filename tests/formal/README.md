Local browser tests live here.

- `tests/formal/tests/` contains local browser tests. They can use upstream `testharness.js` assets or assign a compatible result object to `window.__formalWebTestResult` directly.
- `tests/formal/include.ini` controls the default selection when `formal-web test-wpt` runs without an explicit path.
- The WPT runner mounts this tree through `wpt serve` at `/__formal__/`, so local tests can use the upstream `/resources/testharness.js` and `/resources/testharnessreport.js` assets.