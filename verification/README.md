# verification crate

The verification crate owns trace recording, TLA+ validation, and the shutdown workflow that ties them together.

- The main process starts the trace monitor, shares senders with local workers, and sends the same sender to the content and net sidecars after IPC bootstrap.
- Trace specs live under `verification/tla_specs/`, and recorded NDJSON logs plus TLC working files live in temporary directories that are removed after validation.
- `VerificationRun::finish()` requires all top-level `TraceSender` clones to be dropped so shutdown can observe channel closure and complete.
- Verification uses the local TLA+ Toolbox jar at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar` by default.
- `./verification/verify-navigation.sh` is the canonical end-to-end navigation verification command. It drives one link click and treats successful `Navigation` TLA+ validation as the acceptance criterion.
- `./verification/verify-rendering.sh` is the screenshot-based rendering verification command. It focuses on startup-artifact rendering, visible cross-origin iframe composition, and wheel-driven iframe scrolling, and it is intentionally separate from TLA+ navigation validation.
- `./verification/run-cdp-startup-feature-check.sh` is the Rust-native external CDP feature-check command for startup artifact checks.
- `verification/src/bin/cdp_smoke_check.rs` is the reusable CDP client implementation used by the script above.