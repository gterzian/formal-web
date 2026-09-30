//! The WebRTC extension process: the network side of every RTCPeerConnection
//! (ICE, DTLS, SCTP), on the backend selected in `backend`.
//!
//! Each peer connection gets a worker thread that owns its backend peer and
//! runs its requests in order, so one peer connection's slow operation does
//! not hold up another's. Results and backend events go straight to the
//! requesting content process as `ContentCommand::WebRtc`.

mod backend;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use backend::{Backend, Peer, PeerEngine};
use ipc_messages::content::{Command as ContentCommand, DocumentId};
use ipc_messages::graphics::GraphicsCommand;
use ipc_messages::webrtc::{
    Configuration, DataChannelHandle, DataChannelInit, DataChannelProperties, IceCandidate,
    Message, OperationId, OperationResult, Payload, PeerConnectionId, PeerEvent, Request, SdpType,
    SessionDescription, TrackKind, TransceiverDirection, TransceiverId, TransceiverSpec,
    TransceiverState,
};

/// What a peer's worker thread runs: requests from content, and follow-ups
/// the engine's event sink hands back because they need the peer.
enum Work {
    Request(Request),
    /// The engine's buffered amount for this (engine) channel fell to the
    /// low threshold.
    BufferedAmountLow(u32),
}

/// Data channel handles, content's and the engine's, for one peer.
#[derive(Default)]
struct Channels {
    to_engine: HashMap<u32, u32>,
    to_content: HashMap<u32, u32>,
    /// Per content handle, the `through` of the last send.
    through: HashMap<u32, u64>,
    next_remote: u32,
    /// The engine reports gathering complete before the end of candidates;
    /// the W3C order is the reverse, so the state is held until then.
    gathering_complete_held: bool,
    /// While a description is being applied, the engine's events wait here:
    /// the engine starts gathering and connecting while it applies the
    /// description, but the task that completes setLocalDescription or
    /// setRemoteDescription comes first in the W3C algorithms, and the
    /// events follow as tasks of their own.
    holding: bool,
    held: Vec<PeerEvent>,
}

impl Channels {
    fn bind(&mut self, content: u32, engine: u32) {
        self.to_engine.insert(content, engine);
        self.to_content.insert(engine, content);
    }

    fn content_for_remote(&mut self, engine: u32) -> u32 {
        if let Some(content) = self.to_content.get(&engine) {
            return *content;
        }
        self.next_remote += 1;
        let content = DataChannelHandle::REMOTE | self.next_remote;
        self.bind(content, engine);
        content
    }
}

/// Where one peer connection's results and events go.
#[derive(Clone)]
struct ReplyRoute {
    document_id: DocumentId,
    peer: PeerConnectionId,
    sender: Arc<Mutex<ipc::IpcSender<ContentCommand>>>,
}

impl ReplyRoute {
    fn send(&self, message: Message) {
        let command = ContentCommand::WebRtc {
            document_id: self.document_id,
            peer: self.peer,
            message,
        };
        let sent = match self.sender.lock() {
            Ok(sender) => sender.send(command),
            Err(poisoned) => poisoned.into_inner().send(command),
        };
        if let Err(error) = sent {
            log::debug!("[webrtc] content for peer {:?} is gone: {error}", self.peer);
        }
    }
}

/// Run the WebRTC extension over an established connection: the entry point
/// for the in-process transport as well as for the `formal-web-webrtc`
/// binary.
/// The WebRTC engine of one net process: every peer connection of every
/// content process it serves, each on its own worker thread.
pub struct WebRtcEngine {
    engine: Arc<Backend>,
    workers: HashMap<PeerConnectionId, crossbeam_channel::Sender<Work>>,
    /// The graphics process, which plays out the audio the engine decodes.
    graphics: GraphicsRoute,
}

/// The sender to the graphics process, shared by every peer worker's event
/// sink; `None` until the user agent hands it over.
type GraphicsRoute = Arc<Mutex<Option<ipc::IpcSender<GraphicsCommand>>>>;

impl WebRtcEngine {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            engine: Arc::new(Backend::new().map_err(|error| format!("WebRTC backend: {error}"))?),
            workers: HashMap::new(),
            graphics: Arc::new(Mutex::new(None)),
        })
    }

    /// The graphics process's command sender, for audio playout.
    pub fn set_graphics_sender(&mut self, sender: ipc::IpcSender<GraphicsCommand>) {
        let mut graphics = match self.graphics.lock() {
            Ok(graphics) => graphics,
            Err(poisoned) => poisoned.into_inner(),
        };
        *graphics = Some(sender);
    }

    /// Run one request from a content process.
    pub fn handle(&mut self, request: Request) {
        let engine = &self.engine;
        let workers = &mut self.workers;
        let peer = match &request {
            Request::Shutdown => {
                workers.clear();
                return;
            }
            Request::CreatePeer {
                document_id,
                peer,
                configuration,
                reply_to,
            } => {
                let route = ReplyRoute {
                    document_id: *document_id,
                    peer: *peer,
                    sender: Arc::new(Mutex::new(reply_to.clone())),
                };
                match spawn_peer_worker(engine, configuration, route, Arc::clone(&self.graphics)) {
                    Ok(worker) => {
                        workers.insert(*peer, worker);
                    }
                    Err(error) => log::error!("[webrtc] create peer {peer:?}: {error}"),
                }
                return;
            }
            Request::CreateOffer { peer, .. }
            | Request::CreateAnswer { peer, .. }
            | Request::SetLocalDescription { peer, .. }
            | Request::SetRemoteDescription { peer, .. }
            | Request::AddIceCandidate { peer, .. }
            | Request::CreateDataChannel { peer, .. }
            | Request::DataChannelSend { peer, .. }
            | Request::DataChannelSetBufferedAmountLowThreshold { peer, .. }
            | Request::DataChannelClose { peer, .. }
            | Request::UpsertTransceiver { peer, .. }
            | Request::GetStats { peer, .. }
            | Request::PushPcm { peer, .. }
            | Request::Close { peer } => *peer,
        };
        let closing = matches!(request, Request::Close { .. });
        if let Some(worker) = workers.get(&peer)
            && worker.send(Work::Request(request)).is_err()
        {
            workers.remove(&peer);
        }
        if closing {
            workers.remove(&peer);
        }
    }
}

fn spawn_peer_worker(
    engine: &Arc<Backend>,
    configuration: &Configuration,
    route: ReplyRoute,
    graphics: GraphicsRoute,
) -> Result<crossbeam_channel::Sender<Work>, String> {
    let (sender, receiver) = crossbeam_channel::unbounded::<Work>();
    let channels = Arc::new(Mutex::new(Channels::default()));
    let events = route.clone();
    let sink_channels = Arc::clone(&channels);
    let sink_work = sender.clone();
    let sink_peer = route.peer;
    let sink: backend::EventSink = Arc::new(move |event| {
        // Decoded audio goes to the graphics process for playout, not to
        // content.
        if let backend::PeerEvent::AudioPcm { tx, samples } = event {
            let graphics = match graphics.lock() {
                Ok(graphics) => graphics,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(sender) = graphics.as_ref()
                && let Err(error) = sender.send(GraphicsCommand::PlayAudioPcm {
                    peer: sink_peer,
                    transceiver: TransceiverId(tx),
                    samples,
                })
            {
                log::debug!("[webrtc] audio playout for peer {sink_peer:?}: {error}");
            }
            return;
        }
        let mut channels = match sink_channels.lock() {
            Ok(channels) => channels,
            Err(poisoned) => poisoned.into_inner(),
        };
        for event in peer_events(event, &mut channels, &sink_work) {
            if channels.holding {
                channels.held.push(event);
            } else {
                events.send(Message::Event(event));
            }
        }
    });
    let peer = engine
        .create_peer(&rtc_configuration(configuration), sink)
        .map_err(|error| error.to_string())?;
    thread::Builder::new()
        .name(format!("webrtc-peer-{}", route.peer.0))
        .spawn(move || {
            while let Ok(work) = receiver.recv() {
                match work {
                    Work::Request(request) => {
                        let closing = matches!(request, Request::Close { .. });
                        run_request(peer.as_ref(), request, &route, &channels);
                        if closing {
                            break;
                        }
                    }
                    Work::BufferedAmountLow(engine_handle) => {
                        report_buffered_amount(peer.as_ref(), engine_handle, &route, &channels);
                    }
                }
            }
            peer.close();
        })
        .map_err(|error| error.to_string())?;
    Ok(sender)
}

/// Hold the engine's events, or send the held ones and stop holding.
fn hold_events(channels: &Mutex<Channels>, hold: bool, route: &ReplyRoute) {
    let mut channels = match channels.lock() {
        Ok(channels) => channels,
        Err(poisoned) => poisoned.into_inner(),
    };
    channels.holding = hold;
    if !hold {
        for event in std::mem::take(&mut channels.held) {
            route.send(Message::Event(event));
        }
    }
}

/// Send content the engine's buffered amount for one channel.
fn report_buffered_amount(
    peer: &dyn Peer,
    engine_handle: u32,
    route: &ReplyRoute,
    channels: &Mutex<Channels>,
) {
    let (content, through) = {
        let channels = match channels.lock() {
            Ok(channels) => channels,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(content) = channels.to_content.get(&engine_handle).copied() else {
            return;
        };
        (
            content,
            channels.through.get(&content).copied().unwrap_or(0),
        )
    };
    let amount = peer.dc_buffered_amount(engine_handle).unwrap_or(0);
    route.send(Message::Event(PeerEvent::DataChannelBufferedAmount {
        channel: DataChannelHandle(content),
        through,
        amount,
    }));
}

fn run_request(peer: &dyn Peer, request: Request, route: &ReplyRoute, channels: &Mutex<Channels>) {
    let complete = |operation: OperationId, result: OperationResult| {
        route.send(Message::Completed { operation, result });
    };
    let engine_handle = |content: DataChannelHandle| -> Option<u32> {
        let channels = match channels.lock() {
            Ok(channels) => channels,
            Err(poisoned) => poisoned.into_inner(),
        };
        channels.to_engine.get(&content.0).copied()
    };
    match request {
        Request::CreateOffer { operation, .. } => {
            complete(operation, description_result(peer.create_offer()));
        }
        Request::CreateAnswer { operation, .. } => {
            complete(operation, description_result(peer.create_answer()));
        }
        Request::SetLocalDescription {
            operation,
            description,
            ..
        } => {
            hold_events(channels, true, route);
            let result = peer.set_local_description(&session_description(description));
            complete(operation, negotiated_result(result));
            hold_events(channels, false, route);
        }
        Request::SetRemoteDescription {
            operation,
            description,
            ..
        } => {
            hold_events(channels, true, route);
            let result = peer.set_remote_description(&session_description(description));
            complete(operation, negotiated_result(result));
            hold_events(channels, false, route);
        }
        Request::AddIceCandidate {
            operation,
            candidate,
            ..
        } => {
            // The end-of-candidates indication is accepted and has no
            // effect on the backend.
            let result = match candidate {
                Some(candidate) => peer.add_ice_candidate(&ice_candidate(candidate)),
                None => Ok(()),
            };
            complete(operation, done_result(result));
        }
        Request::CreateDataChannel {
            channel,
            label,
            init,
            ..
        } => match peer.create_data_channel(&label, &data_channel_init(init)) {
            Ok(info) => {
                let mut channels = match channels.lock() {
                    Ok(channels) => channels,
                    Err(poisoned) => poisoned.into_inner(),
                };
                channels.bind(channel.0, info.handle);
            }
            Err(error) => {
                route.send(Message::Event(PeerEvent::DataChannelError {
                    channel,
                    message: error.to_string(),
                }));
                route.send(Message::Event(PeerEvent::DataChannelClosed { channel }));
            }
        },
        Request::DataChannelSend {
            channel,
            payload,
            through,
            ..
        } => {
            let Some(handle) = engine_handle(channel) else {
                return;
            };
            let payload = match payload {
                Payload::Text(text) => backend::Payload::Text(text),
                Payload::Binary(bytes) => backend::Payload::Binary(bytes),
            };
            if let Err(error) = peer.dc_send(handle, payload) {
                log::debug!("[webrtc] send on channel {}: {error}", channel.0);
            }
            if let Ok(mut channels) = channels.lock() {
                channels.through.insert(channel.0, through);
            }
            report_buffered_amount(peer, handle, route, channels);
        }
        Request::DataChannelSetBufferedAmountLowThreshold {
            channel, threshold, ..
        } => {
            if let Some(handle) = engine_handle(channel)
                && let Err(error) = peer.dc_set_buffered_amount_low_threshold(handle, threshold)
            {
                log::debug!("[webrtc] threshold on channel {}: {error}", channel.0);
            }
        }
        Request::DataChannelClose { channel, .. } => {
            if let Some(handle) = engine_handle(channel)
                && let Err(error) = peer.dc_close(handle)
            {
                log::debug!("[webrtc] close channel {}: {error}", channel.0);
            }
        }
        Request::UpsertTransceiver { spec, .. } => {
            if let Err(error) = peer.upsert_transceiver(transceiver_spec(spec)) {
                log::error!("[webrtc] upsert transceiver: {error}");
            }
        }
        Request::PushPcm {
            transceiver,
            samples,
            ..
        } => {
            if let Err(error) = peer.push_pcm(transceiver.0, samples) {
                log::debug!("[webrtc] push pcm: {error}");
            }
        }
        Request::GetStats { operation, .. } => {
            let result = match peer.stats() {
                Ok(stats) => OperationResult::Stats(stats.to_string()),
                Err(error) => error_result(error),
            };
            complete(operation, result);
        }
        Request::Close { .. } | Request::CreatePeer { .. } | Request::Shutdown => {}
    }
}

fn rtc_configuration(configuration: &Configuration) -> backend::RtcConfiguration {
    backend::RtcConfiguration {
        ice_servers: configuration
            .ice_servers
            .iter()
            .map(|server| backend::IceServer {
                urls: server.urls.clone(),
                username: server.username.clone(),
                credential: server.credential.clone(),
            })
            .collect(),
        ice_transport_policy: Some(configuration.ice_transport_policy.clone()),
        bundle_policy: Some(configuration.bundle_policy.clone()),
        ..Default::default()
    }
}

fn session_description(description: SessionDescription) -> backend::SessionDescription {
    backend::SessionDescription {
        kind: match description.kind {
            SdpType::Offer => backend::SdpType::Offer,
            SdpType::Pranswer => backend::SdpType::Pranswer,
            SdpType::Answer => backend::SdpType::Answer,
            SdpType::Rollback => backend::SdpType::Rollback,
        },
        sdp: description.sdp,
    }
}

fn description_result(
    result: Result<backend::SessionDescription, backend::Error>,
) -> OperationResult {
    match result {
        Ok(description) => OperationResult::Description(SessionDescription {
            kind: match description.kind {
                backend::SdpType::Offer => SdpType::Offer,
                backend::SdpType::Pranswer => SdpType::Pranswer,
                backend::SdpType::Answer => SdpType::Answer,
                backend::SdpType::Rollback => SdpType::Rollback,
            },
            sdp: description.sdp,
        }),
        Err(error) => error_result(error),
    }
}

fn done_result(result: Result<(), backend::Error>) -> OperationResult {
    match result {
        Ok(()) => OperationResult::Done,
        Err(error) => error_result(error),
    }
}

/// The result of applying a description: the transceiver states, or the
/// error.
fn negotiated_result(
    result: Result<Vec<backend::TransceiverState>, backend::Error>,
) -> OperationResult {
    match result {
        Ok(states) => {
            OperationResult::Negotiated(states.into_iter().map(transceiver_state).collect())
        }
        Err(error) => error_result(error),
    }
}

fn track_kind(kind: TrackKind) -> backend::TrackKind {
    match kind {
        TrackKind::Audio => backend::TrackKind::Audio,
        TrackKind::Video => backend::TrackKind::Video,
    }
}

fn track_kind_from_backend(kind: backend::TrackKind) -> TrackKind {
    match kind {
        backend::TrackKind::Audio => TrackKind::Audio,
        backend::TrackKind::Video => TrackKind::Video,
    }
}

fn direction(direction: TransceiverDirection) -> backend::Direction {
    match direction {
        TransceiverDirection::Sendrecv => backend::Direction::Sendrecv,
        TransceiverDirection::Sendonly => backend::Direction::Sendonly,
        TransceiverDirection::Recvonly => backend::Direction::Recvonly,
        TransceiverDirection::Inactive => backend::Direction::Inactive,
        TransceiverDirection::Stopped => backend::Direction::Stopped,
    }
}

fn direction_from_backend(direction: backend::Direction) -> TransceiverDirection {
    match direction {
        backend::Direction::Sendrecv => TransceiverDirection::Sendrecv,
        backend::Direction::Sendonly => TransceiverDirection::Sendonly,
        backend::Direction::Recvonly => TransceiverDirection::Recvonly,
        backend::Direction::Inactive => TransceiverDirection::Inactive,
        backend::Direction::Stopped => TransceiverDirection::Stopped,
    }
}

fn transceiver_spec(spec: TransceiverSpec) -> backend::TransceiverSpec {
    backend::TransceiverSpec {
        id: spec.id.0,
        kind: track_kind(spec.kind),
        direction: direction(spec.direction),
        stream_ids: spec.stream_ids,
        sender_track_id: spec.sender_track_id,
        from_add_track: spec.from_add_track,
        stopped: spec.stopped,
        codec_preferences: Vec::new(),
    }
}

fn transceiver_state(state: backend::TransceiverState) -> TransceiverState {
    TransceiverState {
        id: TransceiverId(state.id),
        kind: track_kind_from_backend(state.kind),
        mid: state.mid,
        direction: direction_from_backend(state.direction),
        current_direction: state.current_direction.map(direction_from_backend),
        remote_direction: state.remote_direction.map(direction_from_backend),
        remote_stream_ids: state.remote_stream_ids,
        remote_track_id: state.remote_track_id,
        created_by_remote: state.created_by_remote,
        sender_track_id: state.sender_track_id,
        stopped: state.stopped,
    }
}

fn error_result(error: backend::Error) -> OperationResult {
    OperationResult::Error {
        name: error.dom_name().to_string(),
        message: error.to_string(),
    }
}

fn ice_candidate(candidate: IceCandidate) -> backend::IceCandidate {
    backend::IceCandidate {
        candidate: candidate.candidate,
        sdp_mid: candidate.sdp_mid,
        sdp_m_line_index: candidate.sdp_m_line_index.map(u32::from),
    }
}

fn data_channel_init(init: DataChannelInit) -> backend::DataChannelInit {
    backend::DataChannelInit {
        ordered: Some(init.ordered),
        max_packet_life_time: init.max_packet_life_time,
        max_retransmits: init.max_retransmits,
        protocol: Some(init.protocol),
        negotiated: Some(init.negotiated),
        id: init.id,
    }
}

fn data_channel_properties(info: backend::DataChannelInfo, content: u32) -> DataChannelProperties {
    DataChannelProperties {
        handle: DataChannelHandle(content),
        label: info.label,
        ordered: info.ordered,
        max_packet_life_time: info.max_packet_life_time,
        max_retransmits: info.max_retransmits,
        protocol: info.protocol,
        negotiated: info.negotiated,
        id: info.id,
    }
}

/// Translate one engine event into what content receives, in W3C order.
fn peer_events(
    event: backend::PeerEvent,
    channels: &mut Channels,
    work: &crossbeam_channel::Sender<Work>,
) -> Vec<PeerEvent> {
    use backend::PeerEvent as E;
    let content = |channels: &Channels, engine: u32| channels.to_content.get(&engine).copied();
    match event {
        E::IceCandidate { candidate: None } => {
            // The end of candidates, then the held gathering state.
            let mut out = vec![PeerEvent::IceCandidate(None)];
            if std::mem::take(&mut channels.gathering_complete_held) {
                out.push(PeerEvent::IceGatheringState(String::from("complete")));
            }
            out
        }
        E::IceCandidate {
            candidate: Some(candidate),
        } => vec![PeerEvent::IceCandidate(Some(IceCandidate {
            candidate: candidate.candidate,
            sdp_mid: candidate.sdp_mid,
            sdp_m_line_index: candidate
                .sdp_m_line_index
                .and_then(|index| u16::try_from(index).ok()),
            username_fragment: None,
        }))],
        E::IceGatheringStateChange { state } if state == "complete" => {
            channels.gathering_complete_held = true;
            Vec::new()
        }
        E::IceGatheringStateChange { state } => vec![PeerEvent::IceGatheringState(state)],
        E::IceConnectionStateChange { state } => vec![PeerEvent::IceConnectionState(state)],
        E::ConnectionStateChange { state } => vec![PeerEvent::ConnectionState(state)],
        E::NegotiationNeeded => vec![PeerEvent::NegotiationNeeded],
        E::DataChannel { channel } => {
            let handle = channels.content_for_remote(channel.handle);
            vec![PeerEvent::DataChannel(data_channel_properties(
                channel, handle,
            ))]
        }
        E::DcOpen { handle, id } => content(channels, handle)
            .map(|channel| PeerEvent::DataChannelOpen {
                channel: DataChannelHandle(channel),
                id,
            })
            .into_iter()
            .collect(),
        E::DcMessage { handle, payload } => content(channels, handle)
            .map(|channel| PeerEvent::DataChannelMessage {
                channel: DataChannelHandle(channel),
                payload: match payload {
                    backend::Payload::Text(text) => Payload::Text(text),
                    backend::Payload::Binary(bytes) => Payload::Binary(bytes),
                },
            })
            .into_iter()
            .collect(),
        E::DcBufferedAmountLow { handle } => {
            // The worker thread holds the peer, which the amount is read from.
            if let Err(error) = work.send(Work::BufferedAmountLow(handle)) {
                log::debug!("[webrtc] peer worker is gone, dropping buffered amount low: {error}");
            }
            Vec::new()
        }
        E::DcClose { handle } => content(channels, handle)
            .map(|channel| PeerEvent::DataChannelClosed {
                channel: DataChannelHandle(channel),
            })
            .into_iter()
            .collect(),
        E::DcError { handle, message } => content(channels, handle)
            .map(|channel| PeerEvent::DataChannelError {
                channel: DataChannelHandle(channel),
                message,
            })
            .into_iter()
            .collect(),
        // Signaling state is kept by content per the set-description
        // algorithm; media events have no receiver in phase 1.
        E::SignalingStateChange { .. }
        | E::KeyframeRequest { .. }
        | E::TargetBitrate { .. }
        | E::MediaFrame(_)
        | E::AudioPcm { .. }
        | E::EncodedOut(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Channels, PeerEvent, Work, backend, peer_events};
    use ipc_messages::webrtc::{DataChannelHandle, Payload};

    fn channels_and_work() -> (
        Channels,
        crossbeam_channel::Sender<Work>,
        crossbeam_channel::Receiver<Work>,
    ) {
        let (sender, receiver) = crossbeam_channel::unbounded::<Work>();
        (Channels::default(), sender, receiver)
    }

    fn remote_channel(handle: u32, label: &str) -> backend::PeerEvent {
        backend::PeerEvent::DataChannel {
            channel: backend::DataChannelInfo {
                handle,
                label: String::from(label),
                ordered: true,
                protocol: String::new(),
                negotiated: false,
                id: Some(3),
                max_packet_life_time: None,
                max_retransmits: None,
            },
        }
    }

    #[test]
    fn remote_channel_handles_carry_the_remote_bit_and_stay_stable() {
        let mut channels = Channels::default();
        let first = channels.content_for_remote(7);
        let again = channels.content_for_remote(7);
        let second = channels.content_for_remote(8);
        assert_eq!(first, again);
        assert_ne!(first, second);
        assert_ne!(first & DataChannelHandle::REMOTE, 0);
        assert_ne!(second & DataChannelHandle::REMOTE, 0);
        assert_eq!(channels.to_engine.get(&first), Some(&7));
    }

    #[test]
    fn gathering_complete_waits_for_the_end_of_candidates() {
        let (mut channels, work, _receiver) = channels_and_work();
        let held = peer_events(
            backend::PeerEvent::IceGatheringStateChange {
                state: String::from("complete"),
            },
            &mut channels,
            &work,
        );
        assert!(held.is_empty());
        assert!(channels.gathering_complete_held);

        let released = peer_events(
            backend::PeerEvent::IceCandidate { candidate: None },
            &mut channels,
            &work,
        );
        assert!(matches!(released[0], PeerEvent::IceCandidate(None)));
        assert!(matches!(&released[1], PeerEvent::IceGatheringState(state) if state == "complete"));
        assert_eq!(released.len(), 2);
        assert!(!channels.gathering_complete_held);
    }

    #[test]
    fn other_gathering_states_pass_through() {
        let (mut channels, work, _receiver) = channels_and_work();
        let events = peer_events(
            backend::PeerEvent::IceGatheringStateChange {
                state: String::from("gathering"),
            },
            &mut channels,
            &work,
        );
        assert!(matches!(&events[0], PeerEvent::IceGatheringState(state) if state == "gathering"));
        assert!(!channels.gathering_complete_held);
    }

    #[test]
    fn a_candidate_keeps_its_mid_and_line_index() {
        let (mut channels, work, _receiver) = channels_and_work();
        let events = peer_events(
            backend::PeerEvent::IceCandidate {
                candidate: Some(backend::IceCandidate {
                    candidate: String::from("candidate:1 1 udp 1 127.0.0.1 9 typ host"),
                    sdp_mid: Some(String::from("0")),
                    sdp_m_line_index: Some(0),
                }),
            },
            &mut channels,
            &work,
        );
        let PeerEvent::IceCandidate(Some(candidate)) = &events[0] else {
            panic!("expected a candidate, got {events:?}");
        };
        assert_eq!(candidate.sdp_mid.as_deref(), Some("0"));
        assert_eq!(candidate.sdp_m_line_index, Some(0));
        assert!(candidate.candidate.starts_with("candidate:1"));
    }

    #[test]
    fn a_remote_channel_is_announced_and_its_events_use_the_content_handle() {
        let (mut channels, work, _receiver) = channels_and_work();
        let announced = peer_events(remote_channel(5, "chat"), &mut channels, &work);
        let PeerEvent::DataChannel(properties) = &announced[0] else {
            panic!("expected a data channel announcement, got {announced:?}");
        };
        let content = properties.handle;
        assert_ne!(content.0 & DataChannelHandle::REMOTE, 0);
        assert_eq!(properties.label, "chat");
        assert_eq!(properties.id, Some(3));

        let opened = peer_events(
            backend::PeerEvent::DcOpen {
                handle: 5,
                id: Some(3),
            },
            &mut channels,
            &work,
        );
        assert!(
            matches!(&opened[0], PeerEvent::DataChannelOpen { channel, id: Some(3) } if *channel == content)
        );

        let message = peer_events(
            backend::PeerEvent::DcMessage {
                handle: 5,
                payload: backend::Payload::Text(String::from("hi")),
            },
            &mut channels,
            &work,
        );
        assert!(
            matches!(&message[0], PeerEvent::DataChannelMessage { channel, payload: Payload::Text(text) } if *channel == content && text == "hi")
        );

        let closed = peer_events(
            backend::PeerEvent::DcClose { handle: 5 },
            &mut channels,
            &work,
        );
        assert!(
            matches!(&closed[0], PeerEvent::DataChannelClosed { channel } if *channel == content)
        );
    }

    #[test]
    fn events_for_unknown_engine_channels_are_dropped() {
        let (mut channels, work, _receiver) = channels_and_work();
        let events = peer_events(
            backend::PeerEvent::DcOpen {
                handle: 42,
                id: None,
            },
            &mut channels,
            &work,
        );
        assert!(events.is_empty());
        let events = peer_events(
            backend::PeerEvent::DcError {
                handle: 42,
                message: String::from("boom"),
            },
            &mut channels,
            &work,
        );
        assert!(events.is_empty());
    }

    #[test]
    fn buffered_amount_low_goes_to_the_peer_worker() {
        let (mut channels, work, receiver) = channels_and_work();
        channels.bind(1, 9);
        let events = peer_events(
            backend::PeerEvent::DcBufferedAmountLow { handle: 9 },
            &mut channels,
            &work,
        );
        assert!(events.is_empty());
        assert!(matches!(
            receiver.try_recv(),
            Ok(Work::BufferedAmountLow(9))
        ));
    }

    #[test]
    fn media_and_signaling_events_have_no_content_counterpart() {
        let (mut channels, work, _receiver) = channels_and_work();
        let events = peer_events(
            backend::PeerEvent::SignalingStateChange {
                state: String::from("stable"),
            },
            &mut channels,
            &work,
        );
        assert!(events.is_empty());
        let events = peer_events(
            backend::PeerEvent::TargetBitrate { bps: 1 },
            &mut channels,
            &work,
        );
        assert!(events.is_empty());
    }
}
