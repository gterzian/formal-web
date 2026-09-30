//! The WebRTC backend: an engine behind qrtc's `PeerEngine` and `Peer`
//! traits. qrtc's native engine is the only one; another engine would be
//! selected here by a feature.

pub(crate) use qrtc::native::NativeEngine as Backend;

pub(crate) use qrtc::{
    DataChannelInfo, DataChannelInit, Error, EventSink, IceCandidate, IceServer, Payload, Peer,
    PeerEngine, PeerEvent, RtcConfiguration, SdpType, SessionDescription,
};
