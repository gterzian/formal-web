# tests/formal

Local browser tests live under `tests/formal/tests/` and run through the repository's WPT-compatible runner.

- `tests/formal/include.ini` controls the default selection for `cargo run --manifest-path tests/wpt_runner/Cargo.toml --bin formal-web-wpt --`.
- The runner mounts this tree at `/__formal__/`, so local tests can reuse upstream `/resources/testharness.js` and `/resources/testharnessreport.js`.
- Tests can report through `testharness.js` or assign a compatible result object to `window.__formalWebTestResult` directly.
## Known failures

- `webrtc-data-channel-loopback.html` times out at its `ondatachannel` step
  while Cloudflare WARP is connected on the host: signaling completes, both
  connections add each other's candidates and both stay in ICE "checking".
  The SDP then carries host candidates on the tunnel addresses (172.16.0.2
  and a 2606:4700 IPv6), and a UDP packet between two sockets bound to
  172.16.0.2 is never delivered, while 192.168.x and 127.0.0.1 work.
  qrtc's own in-process test (`native::tests::loopback_data_channel`, run
  from the git checkout with `cargo test --offline --locked`) fails on the
  same host at the same time, so the stall is in the ICE agent's handling
  of the tunnel candidates, not in formal-web. With WARP disconnected the
  test passes again.
  `scratchpad/webrtc-phase2/loopback-stages.html` is the same page with a
  passing subtest per reached step, for locating the stall.

## Manual tests

- `webrtc-audio-loopback.html` is not in `include.ini`: it captures the
  default audio input (microphone permission for the graphics helper, no VPN
  tunnel) and checks through `getStats()` that the encoded audio reaches
  the other peer. Run it with the session harness or the WPT runner by path.
