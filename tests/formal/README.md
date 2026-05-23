# tests/formal

Local browser tests live under `tests/formal/tests/` and run through the repository's WPT-compatible runner.

- `tests/formal/include.ini` controls the default selection for `cargo run --manifest-path tests/wpt_runner/Cargo.toml --bin formal-web-wpt --`.
- The runner mounts this tree at `/__formal__/`, so local tests can reuse upstream `/resources/testharness.js` and `/resources/testharnessreport.js`.
- Tests can report through `testharness.js` or assign a compatible result object to `window.__formalWebTestResult` directly.