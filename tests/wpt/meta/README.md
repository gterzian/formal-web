Mirror the WPT test tree here with Servo-style `.ini` expectation files.

Example for `vendor/wpt/dom/example.html`:

```ini
[example.html]
  expected: FAIL

  [subtest name]
    expected: FAIL
```

The simplified wrapper currently reads `expected:` on the matching top-level
test section and on directly nested subtest sections.
