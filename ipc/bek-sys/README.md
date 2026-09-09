# bek-sys — BrowserEngineKit FFI

Raw FFI bindings for Apple's BrowserEngineKit (iOS/iPadOS only), the
counterpart of `xpc-sys`. BrowserEngineKit launches a browser engine's helper
processes as OS extensions (`NetworkingProcess`, `WebContentProcess`,
`RenderingProcess`) — no launchd plists, no `std::process::Command`. This
crate exists so `ipc`'s `bek` backend can obtain a raw `xpc_connection_t`
from a BEK process handle, which the existing
`xpc_sys::XpcConnection::from_raw` wrapper then owns unmodified.

`ipc/ARCHITECTURE.md` describes the two-backend `ipc` crate; this README
covers the validation story and what is left before a BEK extension can run.

## Why a Swift shim

BrowserEngineKit's API is Swift-only and async: `NetworkingProcess
(bundleIdentifier:onInterruption:) async throws`, `makeLibXPCConnection()
throws -> xpc_connection_t`, `grantCapability(_:) throws ->
ProcessCapability.Grant`. There is no C entry point, so `swift-shim/
BekShim.swift` wraps the framework in `@_cdecl` functions that hand the Rust
side opaque process/grant handles or a raw connection pointer. `build.rs`
compiles the shim with `swiftc` against the real framework in the
iPhoneOS/iPhoneSimulator SDK for every iOS-target build.

## Validation without a device or entitlement

The point of this crate's current milestone is **linking real Swift code
against the real `BrowserEngineKit.framework` symbols** — a mismatch between
the `ipc` abstraction and BEK's actual API surfaces as a compile/link error
now. The Simulator SDK requires none of the gates that running BEK does (no
device, no paid Developer Program membership, no EU/Japan entitlement grant):

```bash
rustup target add aarch64-apple-ios-sim
cargo build -p ipc -p bek-sys --target aarch64-apple-ios-sim \
  --no-default-features --features bek        # design-validation build
cargo build -p bek-sys --example ffi_link \
  --target aarch64-apple-ios-sim              # link-check the FFI surface
```

Both need Xcode (already required for `xpc-sys`'s C shim). If any function
name, argument type, or closure signature in `BekShim.swift` does not match
what `BrowserEngineKit` declares, `swiftc` fails here — that failure is the
design check. The result links but cannot launch: running BEK's process
launch needs a code-signed, entitled app bundle on a device or Simulator.

Ownership notes for the shim boundary:

- Process handles are retained Swift `ProcessBox` objects; `invalidate`
  stops the process and releases the box.
- The connection pointer from `make_xpc_connection` carries one owned
  reference (via ARC's `passRetained`) for `XpcConnection::from_raw`, whose
  drop releases it.
- Error strings are `strdup`'d on the Swift side; Rust frees them with
  `bek_free_string`.

## Remaining work (Stage 2 — entitlement- and device-gated)

- The reverse direction: an iOS extension target (Xcode "iOS Generic
  Extension" template) whose Swift shell receives the connection from BEK's
  `handle(xpcConnection:)` callback and hands it to the Rust extension
  entry point through an `@_cdecl`-exported Rust function.
- The EU/Japan alternative-browser-engine entitlements and `arm64e` build
  support; `arm64`+`arm64e` universal host for iPad.
- A host-side `ios-embedder` shell; nothing in this repo today targets iOS.
- Engine JIT entitlement work (`allow-jit`,
  `extended-virtual-addressing`) once a JS engine is chosen for the iOS
  target.
