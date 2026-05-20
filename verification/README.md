# Verification

The verification package owns trace recording, trace validation, and the shutdown workflow that ties them together.

- The trace monitor runs as a thread in the owning main process and receives `LogEntry` messages over IPC senders shared across that process tree.
- The main process starts the monitor when verification is enabled, shares the sender with local threads directly, and sends it to the content and net sidecars as the first command on their existing IPC channels immediately after bootstrap.
- Callers must drop their own top-level `TraceSender` clones before `VerificationRun::finish()` shuts the monitor down, otherwise the IPC channel stays open and shutdown validation blocks waiting for the monitor thread to observe channel closure.
- Only the content and net sidecar entry points receive a shared trace sender from the main process.
- The TLA+ specifications live under `verification/tla_specs/`.
- The canonical end-to-end navigation verification command lives at `./verification/verify-navigation.sh` from the repository root.
- Verification sessions clear the temp verification root before they start, write recorded NDJSON logs and TLC working files under a temporary directory for the current run, and remove that directory after validation finishes.
- The current in-tree trace specs constrain TLC with recorded event names and event arguments. The monitor can record abstract-state update payloads, but the validator currently rejects update-bearing traces until the corresponding trace spec models `UpdateVariables`-style constraints for them.
- The repository does not vendor `tla2tools.jar`; verification launches the TLA+ Toolbox installation at `/Applications/TLA+ Toolbox.app/Contents/Eclipse/tla2tools.jar` by default and uses `FORMAL_WEB_TLC_WORKERS=8` unless the environment overrides it.
- Verification startup and shutdown remove legacy repo-local trace directories such as `tla-traces/`, `states/`, `tla_specs/states/`, and `verification/tla_specs/states/`, along with ignored TLC `.out` files under both spec roots, so verification runs do not leave logs or TLC state traces in the repository.
- Shutdown validation treats the recorded NDJSON log as the observed execution trace, generates the companion `TraceData` module for TLC, runs the corresponding trace spec, prints the result, and then removes the temporary verification artifacts.