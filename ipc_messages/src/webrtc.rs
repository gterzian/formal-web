//! Messages between content processes and the WebRTC engine, which runs
//! inside the net process.
//!
//! The engine owns the network side of every RTCPeerConnection: ICE, DTLS,
//! SCTP. Content sends it `Request`s wrapped in `network::Request::WebRtc`,
//! and it answers on the content process's own command channel with
//! `Command::WebRtc`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::content::{Command as ContentCommand, DocumentId};

/// Identifies one RTCPeerConnection across the content and WebRTC processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerConnectionId(pub Uuid);

impl PeerConnectionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PeerConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identifies one operation whose result settles a promise in content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub u64);

/// Identifies one data channel within its peer connection; distinct from the
/// SCTP stream id. Content assigns the handles of the channels it creates, so
/// it can address a channel before the WebRTC process has answered; the
/// WebRTC process assigns the handles of channels the remote peer opens, with
/// `DataChannelHandle::REMOTE` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataChannelHandle(pub u32);

impl DataChannelHandle {
    /// Set in the handles of channels the remote peer opened.
    pub const REMOTE: u32 = 1 << 31;
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver>
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration>
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Configuration {
    pub ice_servers: Vec<IceServer>,
    /// `"all"` or `"relay"`.
    pub ice_transport_policy: String,
    /// `"balanced"`, `"max-compat"` or `"max-bundle"`.
    pub bundle_policy: String,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsdptype>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SdpType {
    Offer,
    Pranswer,
    Answer,
    Rollback,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsessiondescriptioninit>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDescription {
    pub kind: SdpType,
    pub sdp: String,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcicecandidateinit>
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: Option<u16>,
    pub username_fragment: Option<String>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit>
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataChannelInit {
    pub ordered: bool,
    pub max_packet_life_time: Option<u16>,
    pub max_retransmits: Option<u16>,
    pub protocol: String,
    pub negotiated: bool,
    pub id: Option<u16>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverdirection>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransceiverDirection {
    Sendrecv,
    Sendonly,
    Recvonly,
    Inactive,
    Stopped,
}

/// <https://w3c.github.io/mediacapture-main/#dom-mediastreamtrack-kind>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Audio,
    Video,
}

/// Identifies one transceiver within its peer connection. Content assigns
/// the ids of the transceivers it creates; the WebRTC process assigns the
/// ids of transceivers a remote offer creates, at or above
/// `TransceiverId::REMOTE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransceiverId(pub u32);

impl TransceiverId {
    /// The first id of a transceiver the remote description created.
    pub const REMOTE: u32 = 0x8000_0000;
}

/// What content knows about one of its transceivers; sent whole on every
/// change, the WebRTC process treats it as an upsert.
/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransceiverSpec {
    pub id: TransceiverId,
    pub kind: TrackKind,
    pub direction: TransceiverDirection,
    /// <https://w3c.github.io/webrtc-pc/#dfn-associated-medias-streams>
    pub stream_ids: Vec<String>,
    /// The sender's track id when the transceiver was created (the msid).
    pub sender_track_id: String,
    /// Created by addTrack(), so eligible for association with a remote
    /// m= section.
    pub from_add_track: bool,
    pub stopped: bool,
}

/// The negotiation facts of one transceiver after a description was applied,
/// for the transceiver steps of set a session description (steps 4.7.10 to
/// 4.7.16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransceiverState {
    pub id: TransceiverId,
    pub kind: TrackKind,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-mid>
    pub mid: Option<String>,
    pub direction: TransceiverDirection,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-currentdirection>
    pub current_direction: Option<TransceiverDirection>,
    /// The direction attribute of the remote m= section, from the remote
    /// peer's perspective.
    pub remote_direction: Option<TransceiverDirection>,
    pub remote_stream_ids: Vec<String>,
    pub remote_track_id: Option<String>,
    pub created_by_remote: bool,
    pub sender_track_id: String,
    pub stopped: bool,
}

/// A data channel's properties as the WebRTC process knows them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataChannelProperties {
    pub handle: DataChannelHandle,
    pub label: String,
    pub ordered: bool,
    pub max_packet_life_time: Option<u16>,
    pub max_retransmits: Option<u16>,
    pub protocol: String,
    pub negotiated: bool,
    pub id: Option<u16>,
}

/// A message on a data channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    Text(String),
    Binary(Vec<u8>),
}

/// Content to the WebRTC engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Create the peer connection's ICE agent and transports (steps 4 to 12
    /// of the RTCPeerConnection constructor run in the WebRTC process).
    CreatePeer {
        document_id: DocumentId,
        peer: PeerConnectionId,
        configuration: Configuration,
        /// The content process's command sender, where replies and events
        /// for this peer connection go.
        reply_to: ipc::IpcSender<ContentCommand>,
    },
    CreateOffer {
        peer: PeerConnectionId,
        operation: OperationId,
    },
    CreateAnswer {
        peer: PeerConnectionId,
        operation: OperationId,
    },
    SetLocalDescription {
        peer: PeerConnectionId,
        operation: OperationId,
        description: SessionDescription,
    },
    SetRemoteDescription {
        peer: PeerConnectionId,
        operation: OperationId,
        description: SessionDescription,
    },
    /// `None` is the end-of-candidates indication.
    AddIceCandidate {
        peer: PeerConnectionId,
        operation: OperationId,
        candidate: Option<IceCandidate>,
    },
    /// Create the underlying data transport of a channel content created
    /// (createDataChannel step 23). Failures come back as
    /// `PeerEvent::DataChannelError` and `PeerEvent::DataChannelClosed`.
    CreateDataChannel {
        peer: PeerConnectionId,
        channel: DataChannelHandle,
        label: String,
        init: DataChannelInit,
    },
    /// `through` is the total number of bytes content has sent on the
    /// channel, this message included; the WebRTC process echoes it in
    /// `PeerEvent::DataChannelBufferedAmount` so content can account for
    /// messages still in flight between the processes.
    DataChannelSend {
        peer: PeerConnectionId,
        channel: DataChannelHandle,
        payload: Payload,
        through: u64,
    },
    /// Ask for a `PeerEvent::DataChannelBufferedAmount` when the channel's
    /// buffered amount falls to `threshold` or below.
    DataChannelSetBufferedAmountLowThreshold {
        peer: PeerConnectionId,
        channel: DataChannelHandle,
        threshold: u64,
    },
    DataChannelClose {
        peer: PeerConnectionId,
        channel: DataChannelHandle,
    },
    /// Create or update a transceiver (addTrack, addTransceiver, a direction
    /// change, stop) so the next offer or answer negotiates it.
    UpsertTransceiver {
        peer: PeerConnectionId,
        spec: TransceiverSpec,
    },
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-getstats>
    GetStats {
        peer: PeerConnectionId,
        operation: OperationId,
    },
    /// Captured 48 kHz mono PCM for an audio sender: 20 ms from the
    /// graphics process's capture device, for the engine to encode
    /// (`Peer::push_pcm`).
    PushPcm {
        peer: PeerConnectionId,
        transceiver: TransceiverId,
        samples: Vec<i16>,
    },
    Close {
        peer: PeerConnectionId,
    },
    Shutdown,
}

/// The outcome of one operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationResult {
    Description(SessionDescription),
    Done,
    /// A description was applied: the state of every transceiver.
    Negotiated(Vec<TransceiverState>),
    /// The stats report as JSON: an object per stats entry, keyed by id.
    Stats(String),
    /// A DOMException name and message.
    Error {
        name: String,
        message: String,
    },
}

/// An event from the peer connection's ICE agent, DTLS or SCTP transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerEvent {
    /// A local candidate; `None` is the end of gathering.
    IceCandidate(Option<IceCandidate>),
    IceGatheringState(String),
    IceConnectionState(String),
    ConnectionState(String),
    NegotiationNeeded,
    /// The remote peer opened a data channel.
    DataChannel(DataChannelProperties),
    DataChannelOpen {
        channel: DataChannelHandle,
        id: Option<u16>,
    },
    DataChannelMessage {
        channel: DataChannelHandle,
        payload: Payload,
    },
    /// The bytes still buffered in the channel's underlying data transport,
    /// `amount`, once content had sent `through` bytes in total.
    DataChannelBufferedAmount {
        channel: DataChannelHandle,
        through: u64,
        amount: u64,
    },
    DataChannelClosed {
        channel: DataChannelHandle,
    },
    DataChannelError {
        channel: DataChannelHandle,
        message: String,
    },
}

/// WebRTC process to content, inside `Command::WebRtc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Completed {
        operation: OperationId,
        result: OperationResult,
    },
    Event(PeerEvent),
}
