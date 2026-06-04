# WPT Runner (`formal-web-wpt`)

Executes Web Platform Tests (WPT) against formal-web via the WebDriver
protocol and `wpt serve`.

## Running

```bash
# Default selection (controlled by tests/wpt/include.ini and tests/formal/include.ini)
rustup run 1.92.0 cargo run --release -- wpt

# Single test by path
rustup run 1.92.0 cargo run --release -- wpt dom/nodes/Element-hasAttribute.html

# WPT-test-relative path
rustup run 1.92.0 cargo run --release -- wpt vendor/wpt/dom/nodes/Element-hasAttribute.html

# Formal test
rustup run 1.92.0 cargo run --release -- wpt formal/load-event-fires.html

# List selected tests without executing them
rustup run 1.92.0 cargo run --release -- wpt --list

# Headed mode (show browser window)
rustup run 1.92.0 cargo run --release -- wpt --headed
```

The WPT runner is also re-executable directly after the initial build:

```bash
target/release/formal-web-wpt dom/nodes/Element-hasAttribute.html
```

## Architecture

### Entrypoint flow

```
cargo run --release -- wpt <args>
  │
  ├─ cargo builds `formal-web` (entrypoint binary)
  │   └─ build.rs pre-builds component binaries (content, net) into
  │      target/sidecar-prebuild/ and copies them to target/release/
  │
  ├─ formal-web starts, calls wpt_runner::run(args)
  │   └─ maybe_reexec_test_wpt_runner() checks if the current binary
  │      is target/release/formal-web-wpt. If not:
  │      ├─ build_runner_executable() builds the runner binary into
  │      │  target/wpt-prebuild/ and copies to target/release/
  │      │  └─ component binaries are skipped if they already exist
  │      │     (build.rs already placed them)
  │      └─ re-execs via target/release/formal-web-wpt <args>
  │
  └─ formal-web-wpt starts, calls wpt_runner::run(args)
      └─ normal execution path (no re-exec needed)
```

### Runtime flow

```
wpt_runner::run(args)
  ├─ IncludeFilter::load("tests/wpt/include.ini") — builds test selection
  ├─ MetaTree::load("tests/wpt/meta/") — loads expected results / skips
  ├─ collect_selected_tests() — walks WPT + formal test roots
  ├─ WptServeProcess::start() — launches `wpt serve` via Python
  ├─ SharedTestRunner::start() — launches formal-web-embedder (WebDriver)
  │   └─ each test: navigate → wait for completion → compare result
  └─ Print summary, write report JSON, return exit code
```

## Python dependency

The `wpt serve` subprocess requires a Python 3 interpreter with:

- **`ssl` module** — needed by `wptserve` for HTTPS support
- **`venv` module** — needed by `wpt` to set up its virtual environment
- **Working system libraries** — `pyexpat` (expat), `_ssl` (OpenSSL)

### Version detection

The runner discovers a working Python via `resolve_python_interpreter()`:

1. `PYTHON` environment variable (user override)
2. `python3` — fallback
3. `python3.10`, `python3.11`, `python3.12`, `python3.13`

Each candidate is tested with `python3 -c "import ssl; import venv"`. A plain
`--version` check is insufficient because several macOS Python installs pass
`--version` but fail at runtime:

| Python | `--version` | `import ssl` | `import venv` | Root cause |
|--------|-------------|--------------|---------------|------------|
| Homebrew 3.14 | ✅ | ✅ | ❌ | `pyexpat` references `_XML_SetAllocTrackerActivationThreshold` not in system expat |
| pyenv 3.10.7  | ✅ | ❌ | ✅ | `_ssl` needs `openssl@1.1` which was uninstalled |
| Homebrew 3.12 | ✅ | ✅ | ✅ | Works |

### Absolute path resolution

Once a working interpreter is found, `resolve_to_absolute()` resolves it to an
absolute path by running `python3 -c "import sys; print(sys.executable)"`. This
bypasses pyenv shims and `.python-version` file interference when the
subprocess working directory changes to `vendor/wpt/` (which contains a
`.python-version` file requesting Python 3.9, which may not be installed).

## Build details

### Component binaries

There are two places that build the component binaries (`formal-web-embedder`,
`formal-web-content`, `formal-web-net`):

1. **`build.rs`** (workspace entrypoint) — prebuilds into
   `target/sidecar-prebuild/` and copies to `target/release/`.
   Cleans `sidecar-prebuild/` before each build to prevent stale-artifact
   dependency resolution conflicts.

2. **`build_runner_executable()`** in `lib.rs` — builds the runner into
   `target/wpt-prebuild/`. For component binaries, it skips the build if they
   already exist in `target/release/` (placed there by `build.rs`). The runner
   binary itself is always built.

### Re-exec optimization

To avoid cargo profile mismatches between the debug-mode cargo runner and
the release build, `maybe_reexec_test_wpt_runner()` builds the runner binary
for the target profile (release by default) and re-execs via that binary.
This ensures the runner and component binaries share the same profile.

## Test selection

- **`tests/wpt/include.ini`** — Controls which WPT tests run by default.
  Uses `skip: true` at the root with `skip: false` opt-ins per file/directory.
- **`tests/formal/include.ini`** — Same for local formal tests under
  `tests/formal/tests/`.
- **`tests/wpt/meta/`** — WPT metadata (`.ini` files) that declare expected
  results and disable individual tests with TODO reasons.
- **`tests/formal/meta/`** — Same for formal tests.

### Updating metadata when sub-tests start passing

When a task unlocks new passes within a test file, the metadata for that file
must be updated so the default run (`cargo run --release -- wpt`) reports
`unexpected=0`.

**Workflow:**

1. Run the test file in isolation to see which sub-tests still fail:
   ```bash
   cargo run --release -- wpt path/to/test.any.js 2>&1
   ```
2. Remove `expected:` lines from the test's `.ini` file for sub-tests that
   now pass.
3. Add `expected:` lines for any sub-tests that still fail due to
   infrastructure gaps (e.g. missing platform objects like `Blob`,
   `MessageChannel`, unsupported `ResizableArrayBuffer`, etc.), with a
   `# TODO:` comment naming the missing feature.
4. If all sub-tests now pass, remove the file-level `expected: FAIL` or
   `disabled:` line entirely so the test counts as `expected PASS`.
5. Re-run the default suite and confirm `unexpected=0`.

See `tests/wpt/meta/README.md` for the metadata file format.

## Troubleshooting

### Python `ssl` / `venv` errors

```text
Error: ... python3.14 ... exited with return code 1
```

The `wpt serve` subprocess cannot create a virtual environment. Check which
Python is being resolved:

```bash
# Test candidates the runner will try
for cmd in python3 python3.10 python3.11 python3.12; do
  echo -n "$cmd: "; $cmd -c "import ssl; import venv; print('ok')" 2>&1
done
```

Set `PYTHON` to force a specific interpreter:

```bash
PYTHON=python3.12 target/release/formal-web-wpt <test>
```

### Build errors with `boa_gc`

```text
note: there are multiple different versions of crate `boa_gc` in the dependency graph
```

Stale artifacts in `target/sidecar-prebuild/` or `target/wpt-prebuild/`. Clean
those directories and retry:

```bash
rm -rf target/sidecar-prebuild target/wpt-prebuild
```
