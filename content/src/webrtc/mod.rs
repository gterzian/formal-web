//! WebRTC: Real-Time Communication in Browsers
//! (<https://w3c.github.io/webrtc-pc/>), peer connections and data channels.
//!
//! The network side of every connection (ICE, DTLS, SCTP) runs in the WebRTC
//! extension process (`formal-web-webrtc`); content sends it
//! `ipc_messages::webrtc::Request`s and receives its results and events as
//! `Command::WebRtc`, which become `Task::WebRtc` tasks here, so the steps the
//! spec runs "in a task" run on this event loop. Media (tracks, transceivers)
//! is not implemented yet.

pub(crate) mod events;
pub(crate) mod rtc_data_channel;
pub(crate) mod rtc_ice_candidate;
pub(crate) mod rtc_peer_connection;
pub(crate) mod rtc_peer_connection_media;
pub(crate) mod rtc_rtp_transceiver;
pub(crate) mod rtc_session_description;
pub(crate) mod rtc_stats_report;
pub(crate) mod sdp;

use ipc_messages::webrtc::Message;

pub(crate) use events::{RTCDataChannelEvent, RTCPeerConnectionIceEvent, RTCTrackEvent};
pub(crate) use rtc_data_channel::RTCDataChannel;
pub(crate) use rtc_ice_candidate::RTCIceCandidate;
pub(crate) use rtc_peer_connection::RTCPeerConnection;
pub(crate) use rtc_peer_connection_media::{RTCRtpTransceiverInit, TrackOrKind};
pub(crate) use rtc_rtp_transceiver::{RTCRtpReceiver, RTCRtpSender, RTCRtpTransceiver};
pub(crate) use rtc_session_description::RTCSessionDescription;
pub(crate) use rtc_stats_report::RTCStatsReport;

/// What a WebRTC task does for one peer connection.
pub(crate) enum WebRtcTask {
    /// A result or event from the WebRTC process.
    Ipc(Message),
    /// <https://w3c.github.io/webrtc-pc/#dfn-update-the-negotiation-needed-flag>,
    /// step 2's queued task.
    UpdateNegotiationNeededFlag,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit>
#[derive(Debug, Clone)]
pub(crate) struct RTCDataChannelInit {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-ordered>
    pub(crate) ordered: bool,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-maxpacketlifetime>
    pub(crate) max_packet_life_time: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-maxretransmits>
    pub(crate) max_retransmits: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-protocol>
    pub(crate) protocol: String,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-negotiated>
    pub(crate) negotiated: bool,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelinit-id>
    pub(crate) id: Option<u16>,
}

impl Default for RTCDataChannelInit {
    fn default() -> Self {
        Self {
            ordered: true,
            max_packet_life_time: None,
            max_retransmits: None,
            protocol: String::new(),
            negotiated: false,
            id: None,
        }
    }
}
