# tests/webrtc

WebRTC checks that drive formal-web over WebDriver without `wpt serve`. Build
first (`cargo build -p formal-web -p content -p net -p graphics -p webrtc`),
then run under a display (`xvfb-run -a` on a headless Linux machine):

```sh
node tests/webrtc/loopback.mjs
node tests/webrtc/testharness.mjs webrtc-data-channel-loopback.html
node tests/webrtc/testharness.mjs webrtc/RTCPeerConnection-constructor.html
```

- `loopback.mjs` runs two RTCPeerConnections in one page: negotiation, a data
  channel, text and binary messages both ways, close. It prints each step.
- `testharness.mjs` runs one testharness.js page: a bare file name from
  `tests/formal/tests`, or a path under `vendor/wpt`. It serves WPT's
  `testharness.js` and records results with its own `testharnessreport.js`.

The same pages are selected for the WPT runner in `tests/formal/include.ini`
and `tests/wpt/include.ini`.
