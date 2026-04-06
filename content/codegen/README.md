`content/codegen` generates binding helper stubs from the Web IDL files in `content/src/boa/bindings`.

- `interfaces.toml` declares which interfaces are concrete and therefore need descendant-aware helper generation.

- `src/main.rs` loads the configuration, scans `content/src/boa/bindings/*.webidl`, parses each interface, computes inheritance relationships, and writes `*_generated.rs` outputs next to the source Web IDL files.

- `src/parse.rs` validates each Web IDL file with `weedle2` and then extracts the subset of interface and member syntax the current generator supports.

- `src/inheritance.rs` builds the descendant map used to let generated helper functions accept concrete descendants when a binding API is defined on an ancestor interface.

- `src/emit.rs` writes the generated Rust helper functions and the regeneration banner.

- Run the generator with `cargo run --manifest-path content/codegen/Cargo.toml` from the repository root.

- Do not hand-edit `*_generated.rs` files. Change the Web IDL, generator source, or `interfaces.toml`, then rerun the generator.

- When new Web IDL syntax is introduced under `content/src/boa/bindings`, extend `src/parse.rs` first so generation continues to fail at parse time instead of silently skipping members.