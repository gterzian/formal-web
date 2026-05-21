# tests/wpt/meta

Mirror the upstream WPT tree here with Servo-style `.ini` expectation files.

- `__dir__.ini` can disable a mirrored directory subtree with root-level `disabled:` or `skip: true`.
- `<test>.ini` can set root-level `disabled:` or `expected:` on the matching top-level test section, plus directly nested subtest `expected:` entries.
- Supported expectation values are `PASS`, `FAIL`, `TIMEOUT`, `ERROR`, `NOTRUN`, `PRECONDITION_FAILED`, `CRASH`, and `SKIP`.
- Prefer `disabled:` for whole-test failures, with a short reason naming the missing feature or blocking bug.
- Use `expected:` only when the page still runs and mixed top-level or subtest outcomes need explicit tracking.
