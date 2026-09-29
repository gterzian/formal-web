//! The WebRTC extension process: the network side of every RTCPeerConnection
//! (ICE, DTLS, SCTP), on the backend selected in `backend`.
//!
//! Each peer connection gets a worker thread that owns its backend peer and
//! runs its requests in order, so one peer connection's slow operation does
//! not hold up another's. Results and backend events go straight to the
//! requesting content process as `ContentCommand::WebRtc`.

mod backend;

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::thread;

use backend::{Backend, Peer, PeerEngine};
use ipc_messages::content::{Command as ContentCommand, DocumentId};
use ipc_messages::webrtc::{
    Configuration, DataChannelHandle, DataChannelInit, DataChannelProperties, IceCandidate,
    Message, OperationId, OperationResult, Payload, PeerConnectionId, PeerEvent, Request, Response,
    SdpType, SessionDescription,
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

fn webrtc_token_from_args() -> Result<Option<String>, String> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--webrtc-token" {
            return args
                .next()
                .map(Some)
                .ok_or_else(|| String::from("missing webrtc token value"));
        }
    }
    Ok(None)
}

pub fn run_webrtc_process_from_args() -> Result<(), String> {
    let token = webrtc_token_from_args()?;
    ipc::run_extension::<Request, Response>(
        &token.unwrap_or_default(),
        run_webrtc_process_with_server,
    )
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
pub fn run_webrtc_process_with_server(
    server: ipc::ExtensionServer<Response, Request>,
) -> Result<(), String> {
    let request_receiver = ipc::crossbeam_proxy(server.connection.receiver);
    let engine: Arc<Backend> =
        Arc::new(Backend::new().map_err(|error| format!("WebRTC backend: {error}"))?);
    let mut workers: HashMap<PeerConnectionId, crossbeam_channel::Sender<Work>> = HashMap::new();

    while let Ok(incoming) = request_receiver.recv() {
        let request = incoming.payload;
        let peer = match &request {
            Request::Shutdown => break,
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
                match spawn_peer_worker(&engine, configuration, route) {
                    Ok(worker) => {
                        workers.insert(*peer, worker);
                    }
                    Err(error) => log::error!("[webrtc] create peer {peer:?}: {error}"),
                }
                continue;
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
    Ok(())
}

fn spawn_peer_worker(
    engine: &Arc<Backend>,
    configuration: &Configuration,
    route: ReplyRoute,
) -> Result<crossbeam_channel::Sender<Work>, String> {
    let (sender, receiver) = crossbeam_channel::unbounded::<Work>();
    let channels = Arc::new(Mutex::new(Channels::default()));
    let events = route.clone();
    let sink_channels = Arc::clone(&channels);
    let sink_work = sender.clone();
    let sink: backend::EventSink = Arc::new(move |event| {
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
            complete(operation, done_result(result.map(|_| ())));
            hold_events(channels, false, route);
        }
        Request::SetRemoteDescription {
            operation,
            description,
            ..
        } => {
            hold_events(channels, true, route);
            let result = peer.set_remote_description(&session_description(description));
            complete(operation, done_result(result.map(|_| ())));
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
            let _ = work.send(Work::BufferedAmountLow(handle));
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
