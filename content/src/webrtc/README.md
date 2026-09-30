# content/src/webrtc

## Guidance

- The WebRTC engine (`webrtc` crate on qrtc) runs inside the net process;
  every request goes out as `network::Request::WebRtc` on the realm's net
  sender (`send_request`) and the answers come back as `Command::WebRtc` on
  the content process's command channel. The `webrtc` feature of `content`
  and `net` builds both sides (README.md, "Optional web features").

- Every transceiver change (addTrack, addTransceiver, removeTrack, a
  direction change, stop) sends the whole `TransceiverSpec` to the WebRTC
  process (`Request::UpsertTransceiver`), which treats it as an upsert; the
  next offer or answer negotiates it. Applying a description returns the
  `TransceiverState` of every transceiver (`OperationResult::Negotiated`),
  which `apply_transceiver_states` turns into mids, current directions,
  remote-created transceivers and `track` events.
- `getStats()` is not chained on the operations chain: each call keeps its
  promise in `stats_requests` under its own operation id and the report
  arrives as `OperationResult::Stats` (the WebRTC process's JSON, an object
  per stats entry keyed by id). `RTCStatsReport` keeps each entry as JSON
  text and the binding parses it on read, so no stats object lives in
  domain code.
- Platform objects that scripts compare by identity (`RTCRtpSender`,
  `RTCRtpReceiver`, `RTCRtpTransceiver`, `Headers`-style non-EventTargets)
  carry a `reflector` field filled through
  `with_platform_reflector_slot_mut` in `content/src/js/downcast.rs`.

## Remaining work

- Receivers' tracks are marked unmuted when the connection state becomes
  "connected", not when RTP arrives.
- `check if negotiation is needed` compares the negotiated direction the
  WebRTC process reported with the transceiver's direction; the msid lines
  of the description are not compared (step 5.3.1).
- Not implemented: `setCodecPreferences` (accepted, ignored), sender and
  receiver parameters and transports (`getParameters()` returns empty
  lists), `RTCRtpSender.dtmf`, `setConfiguration`, `restartIce`,
  `getStats(selector)` with a track, `RTCRtpScriptTransform`, simulcast.
- Encoded video (`send_frame`, `PeerEvent::MediaFrame`) has no encoder or
  decoder on either side.
- `replaceTrack(null)` leaves the capture running until the transceiver
  stops sending or `removeTrack()` runs.
