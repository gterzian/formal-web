Mirror the WPT test tree here with Servo-style `.ini` expectation files.

Example for `vendor/wpt/dom/example.html`:

```ini
[example.html]
  expected: FAIL

  [subtest name]
    expected: FAIL
```

The current runner reads:

- `__dir__.ini` root-level `disabled:` or `skip: true` to disable a mirrored directory subtree.
- `<test>.ini` root-level `disabled:` on the matching top-level test section.
- `<test>.ini` root-level `expected:` on the matching top-level test section.
- `<test>.ini` directly nested subtest `expected:` entries.

Supported expectation values are `PASS`, `FAIL`, `TIMEOUT`, `ERROR`, `NOTRUN`, `PRECONDITION_FAILED`, `CRASH`, and `SKIP`.
