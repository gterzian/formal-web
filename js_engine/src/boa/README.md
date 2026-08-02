# `boa` — Boa backend

Boa backend for `js_engine` (default feature).  Runs the full WPT suite
(latest: `executed=79 unexpected=0`; wasm tests are opt-in via the `wasm`
feature).

## Open issues

- **`JsValueCell`/`JsObjectCell` values not reclaimed after the cell drops
  once a GC ran while the cell was alive** — observed with
  `generic_js_test::tests::js_value_cell_keeps_value_alive_then_releases`:
  an object held in a `JsValueCell` (`Gc<GcRefCell<JsValue>>`) is not
  collected after the last clone drops when `gc()` ran during the survival
  phase; without the survival GC it is collected.  The object stays alive
  even after the cell's `Gc` refcount reaches zero.
  *Not investigated:* the exact boa_gc mark/sweep interaction (suspected
  interaction with `reset_non_root_count` during sweep and the
  `GcRefCell` contents).  The generic tests therefore only assert the
  keep-alive half of the cell contract.
