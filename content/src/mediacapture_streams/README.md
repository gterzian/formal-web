# content/src/mediacapture_streams

## Guidance

- A `MediaStreamTrack` names its source (`TrackSource`): a capture device of
  the user agent or the remote media of one transceiver. Samples never enter
  content: the graphics process captures the default input and hands 20 ms
  PCM frames to the WebRTC engine in the net process
  (`GraphicsCommand::StartAudioCapture`, `webrtc::Request::PushPcm`), and
  the engine's decoded audio goes back to the graphics process for playout
  (`GraphicsCommand::PlayAudioPcm`). Content only steers the two paths from
  the transceiver lifecycle (`RTCPeerConnection::update_capture`,
  `stop_playout`).
- `MediaStreamTrack`, `MediaStream` and `MediaDevices` are EventTargets: a new
  one goes through the checklist in `content/src/js/README.md` ("Adding an
  EventTarget platform object").

## Remaining work

- A remote audio track plays out as soon as its transceiver receives, whether
  or not an element shows it: `HTMLMediaElement.srcObject` stores the stream
  and runs no load algorithm, so muting an `<audio>` element or removing the
  stream from it does not silence the track.
- One capture at a time: the default input feeds every sending audio
  transceiver the same signal, and the graphics process captures for the
  first one that asks. `enumerateDevices()` lists a fixed default input and
  output; there is no device selection and no `devicechange` event.
- `getUserMedia({video: true})` rejects with "NotFoundError": no video input
  exists. `getDisplayMedia()` rejects with "NotAllowedError".
- Constraints are accepted and ignored; `getSettings()` reports the device
  id only; `getCapabilities()` and `getConstraints()` return empty
  dictionaries; `applyConstraints()` resolves without effect.
- A track's `enabled` flag and `stop()` do not reach the capture: the input
  keeps flowing until the transceiver stops sending.
- Tracks are not `[Serializable]`/`[Transferable]`.
