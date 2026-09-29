//! Media Capture and Streams (<https://w3c.github.io/mediacapture-main/>):
//! media streams, their tracks and the `navigator.mediaDevices` entry point.
//!
//! Capture sources belong to the user agent: a track created by
//! `getUserMedia()` names its device and carries no samples through content.

pub(crate) mod events;
pub(crate) mod media_devices;
pub(crate) mod media_stream;
pub(crate) mod media_stream_track;

pub(crate) use events::MediaStreamTrackEvent;
pub(crate) use media_devices::{MediaDeviceInfo, MediaDevices, MediaStreamConstraints};
pub(crate) use media_stream::{MediaStream, MediaStreamInit};
pub(crate) use media_stream_track::{MediaStreamTrack, MediaStreamTrackState, TrackSource};
