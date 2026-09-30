use std::cell::RefCell;
use std::rc::Rc;

use ipc_messages::content::DocumentId;
use ipc_messages::network::Request as NetworkRequest;
use ipc_messages::webrtc::{
    Configuration, DataChannelHandle, IceServer, Message, OperationId, OperationResult,
    PeerConnectionId, PeerEvent, Request, SessionDescription,
};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::records::PromiseResolvers;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::fire_event;
use crate::html::event_loop::Task;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;

use super::WebRtcTask;
use super::events::{
    RTCDataChannelEvent, RTCPeerConnectionIceEvent, RTCPeerConnectionIceEventInit,
};
use super::rtc_data_channel::{RTCDataChannel, RTCDataChannelState};
use super::rtc_ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use super::rtc_rtp_transceiver::RTCRtpTransceiver;
use super::rtc_session_description::{
    RTCSdpType, RTCSessionDescription, RTCSessionDescriptionInit, description_init_object,
};
use super::sdp;
use crate::dom::fire_event_using;
use crate::mediacapture_streams::MediaStream;

type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver>
#[derive(Debug, Clone, Default)]
pub(crate) struct RTCIceServer {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver-urls>
    pub(crate) urls: Vec<String>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver-username>
    pub(crate) username: Option<String>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtciceserver-credential>
    pub(crate) credential: Option<String>,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration>
#[derive(Debug, Clone)]
pub(crate) struct RTCConfiguration {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration-iceservers>
    pub(crate) ice_servers: Vec<RTCIceServer>,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration-icetransportpolicy>
    pub(crate) ice_transport_policy: String,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration-bundlepolicy>
    pub(crate) bundle_policy: String,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration-rtcpmuxpolicy>
    pub(crate) rtcp_mux_policy: String,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcconfiguration-icecandidatepoolsize>
    pub(crate) ice_candidate_pool_size: u8,
}

impl Default for RTCConfiguration {
    fn default() -> Self {
        Self {
            ice_servers: Vec::new(),
            ice_transport_policy: String::from("all"),
            bundle_policy: String::from("balanced"),
            rtcp_mux_policy: String::from("require"),
            ice_candidate_pool_size: 0,
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcsignalingstate>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RTCSignalingState {
    Stable,
    HaveLocalOffer,
    HaveRemoteOffer,
    HaveLocalPranswer,
    HaveRemotePranswer,
    Closed,
}

impl RTCSignalingState {
    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::HaveLocalOffer => "have-local-offer",
            Self::HaveRemoteOffer => "have-remote-offer",
            Self::HaveLocalPranswer => "have-local-pranswer",
            Self::HaveRemotePranswer => "have-remote-pranswer",
            Self::Closed => "closed",
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcofferoptions> and
/// <https://w3c.github.io/webrtc-pc/#dom-rtclocalsessiondescriptioninit>:
/// setLocalDescription's argument, whose type may be absent.
#[derive(Debug, Clone, Default)]
pub(crate) struct RTCLocalSessionDescriptionInit {
    pub(crate) type_: Option<RTCSdpType>,
    pub(crate) sdp: String,
}

/// The internal slots of an RTCPeerConnection, shared by every clone.
#[derive(Debug)]
pub(super) struct PeerConnectionSlots {
    /// <https://w3c.github.io/webrtc-pc/#dfn-documentorigin>
    pub(super) document_origin: String,
    /// The document the connection belongs to, for the tasks it queues.
    document_id: Option<DocumentId>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-configuration>
    configuration: Option<RTCConfiguration>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-isclosed>
    is_closed: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-negotiationneeded>
    negotiation_needed: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-updatenegotiationneededflagonemptychain>
    update_negotiation_needed_flag_on_empty_chain: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-lastcreatedoffer>
    last_created_offer: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-lastcreatedanswer>
    last_created_answer: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-earlycandidates>
    early_candidates: Vec<RTCIceCandidateInit>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-signalingstate>
    signaling_state: RTCSignalingState,
    /// <https://w3c.github.io/webrtc-pc/#dfn-iceconnectionstate>
    ice_connection_state: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-icegatheringstate>
    ice_gathering_state: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-connectionstate>
    connection_state: String,
    /// The descriptions of [[PendingLocalDescription]],
    /// [[CurrentLocalDescription]], [[PendingRemoteDescription]] and
    /// [[CurrentRemoteDescription]], as the algorithms read them.
    pending_local: Option<RTCSessionDescriptionInit>,
    current_local: Option<RTCSessionDescriptionInit>,
    pending_remote: Option<RTCSessionDescriptionInit>,
    current_remote: Option<RTCSessionDescriptionInit>,
    /// Whether any RTCDataChannel was created on the connection.
    created_data_channel: bool,
    pub(super) next_operation: u64,
    next_channel: u32,
    /// The id the next transceiver content creates takes.
    pub(super) next_transceiver: u32,
}

/// One getStats() call waiting for the WebRTC process's report.
#[gc_struct]
pub(super) struct StatsRequest {
    #[ignore_trace]
    pub(super) operation: OperationId,
    pub(super) resolvers: PromiseResolvers<Types>,
}

/// What one chained operation does.
#[derive(Debug, Clone)]
enum OperationSteps {
    CreateOffer,
    CreateAnswer,
    /// setLocalDescription's chained steps: the description's type, if
    /// present, and sdp. Step 4.1 resolves an absent type when the
    /// operation runs.
    SetLocal {
        type_: Option<RTCSdpType>,
        sdp: String,
    },
    /// setRemoteDescription's chained steps; `rollback_first` when the
    /// offer is invalid for the signaling state (step 3.1).
    SetRemote {
        description: RTCSessionDescriptionInit,
        rollback_first: bool,
    },
    AddIceCandidate {
        candidate: RTCIceCandidateInit,
    },
}

/// What a chained operation's IPC request is for, when one is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlight {
    CreateOffer,
    CreateAnswer,
    SetLocal,
    SetRemote,
    AddIceCandidate,
}

/// One element of [[Operations]]: the operation and the promise p that
/// "chain an operation" returned for it.
#[gc_struct]
struct ChainedOperation {
    #[ignore_trace]
    steps: OperationSteps,

    #[ignore_trace]
    in_flight: Option<(OperationId, InFlight)>,

    promise: JsObject,
    resolve: JsObject,
    reject: JsObject,
}

/// The outcome of an operation's promise.
enum Settled {
    Fulfilled(JsValue),
    Rejected(JsValue),
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection>
#[gc_struct]
pub(crate) struct RTCPeerConnection {
    /// The connection's EventTarget base.
    pub(crate) event_target: EventTarget,

    /// The connection's id in the WebRTC process.
    #[ignore_trace]
    pub(crate) id: PeerConnectionId,

    #[ignore_trace]
    pub(super) slots: Rc<RefCell<PeerConnectionSlots>>,

    /// <https://w3c.github.io/webrtc-pc/#dfn-operations>
    operations: GcCell<Vec<ChainedOperation>>,

    /// <https://w3c.github.io/webrtc-pc/#dfn-datachannels>
    data_channels: GcCell<Vec<RTCDataChannel>>,

    /// Every channel of the connection whose underlying data transport is
    /// not yet closed, by handle; the events of the WebRTC process address
    /// channels that already left [[DataChannels]] (the closing procedure's
    /// step 3) until their close event.
    channels: GcCell<Vec<RTCDataChannel>>,

    /// <https://w3c.github.io/webrtc-pc/#dfn-set-of-transceivers>
    pub(super) transceivers: GcCell<Vec<RTCRtpTransceiver>>,

    /// The MediaStream objects the remote descriptions named, by id
    /// (set a session description step 4.7.15.3.1).
    pub(super) remote_streams: GcCell<Vec<MediaStream>>,

    /// The getStats() calls whose report the WebRTC process has not yet
    /// delivered.
    pub(super) stats_requests: GcCell<Vec<StatsRequest>>,

    /// The RTCSessionDescription objects of [[PendingLocalDescription]],
    /// [[CurrentLocalDescription]], [[PendingRemoteDescription]] and
    /// [[CurrentRemoteDescription]].
    pending_local_description: GcCell<Option<JsObject>>,
    current_local_description: GcCell<Option<JsObject>>,
    pending_remote_description: GcCell<Option<JsObject>>,
    current_remote_description: GcCell<Option<JsObject>>,
}

impl EventTargetAccess for RTCPeerConnection {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

/// Send a request to the WebRTC engine in the net process.
pub(super) fn send_request(request: Request, ec: &mut dyn ExecutionContext<Types>) {
    let sender = with_global_scope(ec, |global_scope, _ec| {
        Ok(global_scope.network_extension_sender())
    })
    .ok()
    .flatten();
    match sender {
        Some(sender) => {
            if let Err(error) = sender.send(NetworkRequest::WebRtc(request)) {
                log::error!("[webrtc] request: {error}");
            }
        }
        None => log::error!("[webrtc] no net process for this realm"),
    }
}

/// A new promise capability, in the current realm.
fn new_promise(
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(JsObject, JsObject, JsObject), Types> {
    let realm = ec.current_realm();
    let intrinsics = ec.realm_intrinsics(&realm);
    let capability = ec.new_promise_capability(intrinsics.promise)?;
    let promise = Types::value_as_object(&capability.promise)
        .ok_or_else(|| ec.new_type_error("promise capability without a promise"))?;
    Ok((
        promise,
        Types::object_from_function(capability.resolve),
        Types::object_from_function(capability.reject),
    ))
}

impl RTCPeerConnection {
    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection>
    pub(crate) fn constructor(
        configuration: RTCConfiguration,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: If any of the steps enumerated below fails for a reason not
        //         specified here, throw an UnknownError with the message
        //         attribute set to an appropriate description.
        // Step 2: Let connection be a newly created RTCPeerConnection object.
        // Step 3: Let connection have a [[DocumentOrigin]] internal slot,
        //         initialized to the relevant settings object's origin.
        let (document_origin, document_id, reply_to) =
            with_global_scope(ec, |global_scope, _ec| {
                Ok((
                    global_scope
                        .creation_url()
                        .map(|url| url.origin().ascii_serialization())
                        .unwrap_or_else(|| String::from("null")),
                    global_scope.document_id(),
                    global_scope.content_command_sender(),
                ))
            })?;
        // Step 4: Let configuration be the method's first argument.
        // Step 5: If the certificates value in configuration is non-empty, run
        //         the following steps for each certificate in certificates:
        // Step 5.1: If the value of certificate.expires is less than the
        //           current time, throw an InvalidAccessError.
        // Step 5.2: If certificate.[[Origin]] is not same origin with
        //           connection.[[DocumentOrigin]], throw an InvalidAccessError.
        // Step 5.3: Store certificate.
        // Step 6: Else, generate one or more new RTCCertificate instances with
        //         this RTCPeerConnection instance and store them.
        // Note: RTCCertificate is not implemented, so no value converts to
        // one and certificates is always empty here (the binding throws a
        // TypeError for any element); the WebRTC process generates the
        // connection's certificate (step 6).
        let connection = Self {
            event_target: EventTarget::new(ec),
            id: PeerConnectionId::new(),
            // Steps 9-27 initialize these internal slots.
            slots: Rc::new(RefCell::new(PeerConnectionSlots {
                document_origin,
                document_id,
                // Step 8: Let connection have a [[Configuration]] internal
                //         slot, initialized to null.
                configuration: None,
                // Step 9: Let connection have an [[IsClosed]] internal slot,
                //         initialized to false.
                is_closed: false,
                // Step 10: Let connection have a [[NegotiationNeeded]]
                //          internal slot, initialized to false.
                negotiation_needed: false,
                // Step 11: Let connection have an [[SctpTransport]] internal
                //          slot, initialized to null.
                // Step 12: Let connection have an
                //          [[LastStableStateSctpTransport]] internal slot,
                //          initialized to null.
                // Note: RTCSctpTransport is not implemented.
                // Step 15: Let connection have a
                //          [[UpdateNegotiationNeededFlagOnEmptyChain]]
                //          internal slot, initialized to false.
                update_negotiation_needed_flag_on_empty_chain: false,
                // Step 16: Let connection have an [[LastCreatedOffer]]
                //          internal slot, initialized to "".
                last_created_offer: String::new(),
                // Step 17: Let connection have an [[LastCreatedAnswer]]
                //          internal slot, initialized to "".
                last_created_answer: String::new(),
                // Step 18: Let connection have an [[EarlyCandidates]] internal
                //          slot, initialized to an empty list.
                early_candidates: Vec::new(),
                // Step 19: Let connection have an [[SignalingState]] internal
                //          slot, initialized to "stable".
                signaling_state: RTCSignalingState::Stable,
                // Step 20: Let connection have an [[IceConnectionState]]
                //          internal slot, initialized to "new".
                ice_connection_state: String::from("new"),
                // Step 21: Let connection have an [[IceGatheringState]]
                //          internal slot, initialized to "new".
                ice_gathering_state: String::from("new"),
                // Step 22: Let connection have an [[ConnectionState]] internal
                //          slot, initialized to "new".
                connection_state: String::from("new"),
                // Step 23: Let connection have a [[PendingLocalDescription]]
                //          internal slot, initialized to null.
                pending_local: None,
                // Step 24: Let connection have a [[CurrentLocalDescription]]
                //          internal slot, initialized to null.
                current_local: None,
                // Step 25: Let connection have a [[PendingRemoteDescription]]
                //          internal slot, initialized to null.
                pending_remote: None,
                // Step 26: Let connection have a [[CurrentRemoteDescription]]
                //          internal slot, initialized to null.
                current_remote: None,
                // Step 27: Let connection have a
                //          [[LocalIceCredentialsToReplace]] internal slot,
                //          initialized to an empty set.
                // Note: restartIce() is not implemented, so the set stays
                // empty.
                created_data_channel: false,
                next_operation: 0,
                next_channel: 0,
                next_transceiver: 0,
            })),
            transceivers: gc_cell_new(Vec::new(), ec),
            remote_streams: gc_cell_new(Vec::new(), ec),
            stats_requests: gc_cell_new(Vec::new(), ec),
            // Step 14: Let connection have an [[Operations]] internal slot,
            //          representing an operations chain, initialized to an
            //          empty list.
            operations: gc_cell_new(Vec::new(), ec),
            // Step 13: Let connection have a [[DataChannels]] internal slot,
            //          initialized to an empty ordered set.
            data_channels: gc_cell_new(Vec::new(), ec),
            channels: gc_cell_new(Vec::new(), ec),
            pending_local_description: gc_cell_new(None, ec),
            current_local_description: gc_cell_new(None, ec),
            pending_remote_description: gc_cell_new(None, ec),
            current_remote_description: gc_cell_new(None, ec),
        };
        // Step 8 (cont.): Set the configuration specified by configuration.
        let ipc_configuration = connection.set_the_configuration(configuration, ec)?;
        // Step 7: Initialize connection's ICE Agent.
        // Note: The ICE Agent lives in the WebRTC process; it starts once the
        // configuration is known, so this runs after step 8's validation.
        let (Some(document_id), Some(reply_to)) = (document_id, reply_to) else {
            return Err(ec.new_type_error("RTCPeerConnection needs a document"));
        };
        send_request(
            Request::CreatePeer {
                document_id,
                peer: connection.id,
                configuration: ipc_configuration,
                reply_to,
            },
            ec,
        );
        // The realm keeps the connection to route the WebRTC process's
        // results and events to it (see `GlobalScope::peer_connection`).
        let registered = connection.clone();
        with_global_scope(ec, move |global_scope, ec| {
            global_scope.register_peer_connection(registered, ec);
            Ok(())
        })?;
        // Step 28: Return connection.
        Ok(connection)
    }

    /// <https://w3c.github.io/webrtc-pc/#set-pc-configuration>
    fn set_the_configuration(
        &self,
        configuration: RTCConfiguration,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Configuration, Types> {
        // Step 1: Let connection be the target RTCPeerConnection object.
        // Step 2: Let oldConfig be connection.[[Configuration]].
        // Step 3: If oldConfig is not null, run the following steps, and if
        //         any of them fail, throw an InvalidModificationError:
        // Note: setConfiguration() is not implemented; this runs only from
        // the constructor, where oldConfig is null.
        // Step 4: Let iceServers be configuration.iceServers.
        // Step 5: Truncate iceServers to the maximum number of supported
        //         elements.
        // Note: No maximum.
        let mut ice_servers = Vec::new();
        // Step 6: For each server in iceServers, run the following steps:
        for server in &configuration.ice_servers {
            // Step 6.1: Let urls be server.urls.
            // Step 6.2: If urls is a string, set urls to a list consisting of
            //           just that string.
            // Note: The binding converted the union to a list.
            // Step 6.3: If urls is empty, throw a "SyntaxError" DOMException.
            if server.urls.is_empty() {
                return Err(crate::webidl::syntax_error_value(ec));
            }
            // Step 6.4: For each url in urls, run the validate an ICE server
            //           URL algorithm on url.
            for url in &server.urls {
                validate_an_ice_server_url(url, server, ec)?;
            }
            ice_servers.push(IceServer {
                urls: server.urls.clone(),
                username: server.username.clone(),
                credential: server.credential.clone(),
            });
        }
        // Step 7: Set the ICE Agent's ICE transports setting to the value of
        //         configuration.iceTransportPolicy.
        // Step 8: Set the ICE Agent's prefetched ICE candidate pool size as
        //         defined in [[!RFC9429]] to the value of
        //         configuration.iceCandidatePoolSize.
        // Note: Candidate pooling is not implemented; the size is kept.
        // Step 9: Set the ICE Agent's ICE servers list to iceServers.
        // Note: The WebRTC process applies these settings (see
        // `Request::CreatePeer`).
        let ipc = Configuration {
            ice_servers,
            ice_transport_policy: configuration.ice_transport_policy.clone(),
            bundle_policy: configuration.bundle_policy.clone(),
        };
        // Step 10: Store configuration in the [[Configuration]] internal slot.
        self.slots.borrow_mut().configuration = Some(configuration);
        Ok(ipc)
    }

    /// Whether [[IsClosed]] is true.
    pub(crate) fn is_closed(&self) -> bool {
        self.slots.borrow().is_closed
    }

    // ── Attributes ─────────────────────────────────────────────────────

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-localdescription>
    pub(crate) fn local_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        // The localDescription attribute MUST return
        // [[PendingLocalDescription]] if it is not null and otherwise it MUST
        // return [[CurrentLocalDescription]].
        self.pending_local_description
            .borrow(ec)
            .clone()
            .or_else(|| self.current_local_description.borrow(ec).clone())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-currentlocaldesc>
    pub(crate) fn current_local_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        self.current_local_description.borrow(ec).clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-pendinglocaldesc>
    pub(crate) fn pending_local_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        self.pending_local_description.borrow(ec).clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-remotedescription>
    pub(crate) fn remote_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        // The remoteDescription attribute MUST return
        // [[PendingRemoteDescription]] if it is not null and otherwise it
        // MUST return [[CurrentRemoteDescription]].
        self.pending_remote_description
            .borrow(ec)
            .clone()
            .or_else(|| self.current_remote_description.borrow(ec).clone())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-currentremotedesc>
    pub(crate) fn current_remote_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        self.current_remote_description.borrow(ec).clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-pendingremotedesc>
    pub(crate) fn pending_remote_description(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<JsObject> {
        self.pending_remote_description.borrow(ec).clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-signaling-state>
    pub(crate) fn signaling_state(&self) -> &'static str {
        self.slots.borrow().signaling_state.as_idl()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-ice-gathering-state>
    pub(crate) fn ice_gathering_state(&self) -> String {
        self.slots.borrow().ice_gathering_state.clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-ice-connection-state>
    pub(crate) fn ice_connection_state(&self) -> String {
        self.slots.borrow().ice_connection_state.clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-connection-state>
    pub(crate) fn connection_state(&self) -> String {
        self.slots.borrow().connection_state.clone()
    }

    fn remote_description_init(&self) -> Option<RTCSessionDescriptionInit> {
        let slots = self.slots.borrow();
        slots
            .pending_remote
            .clone()
            .or_else(|| slots.current_remote.clone())
    }

    fn local_description_init(&self) -> Option<RTCSessionDescriptionInit> {
        let slots = self.slots.borrow();
        slots
            .pending_local
            .clone()
            .or_else(|| slots.current_local.clone())
    }

    // ── Methods ────────────────────────────────────────────────────────

    /// <https://w3c.github.io/webrtc-pc/#dfn-createoffer>
    pub(crate) fn create_offer(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 2: If connection.[[IsClosed]] is true, return a promise
        //         rejected with a newly created InvalidStateError.
        if self.is_closed() {
            let error = crate::webidl::invalid_state_error_value(ec);
            return crate::webidl::rejected_promise(error, ec);
        }
        // Step 3: Return the result of chaining the result of creating an
        //         offer with connection to connection's operations chain.
        self.chain_an_operation(OperationSteps::CreateOffer, ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-createanswer>
    pub(crate) fn create_answer(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 2: If connection.[[IsClosed]] is true, return a promise
        //         rejected with a newly created InvalidStateError.
        if self.is_closed() {
            let error = crate::webidl::invalid_state_error_value(ec);
            return crate::webidl::rejected_promise(error, ec);
        }
        // Step 3: Return the result of chaining the result of creating an
        //         answer with connection to connection's operations chain.
        self.chain_an_operation(OperationSteps::CreateAnswer, ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setlocaldescription>
    pub(crate) fn set_local_description(
        &self,
        description: RTCLocalSessionDescriptionInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let description be the method's first argument.
        // Step 2: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 3: Let sdp be description.sdp.
        // Step 4: Return the result of chaining the following steps to
        //         connection's operations chain:
        // Note: Step 4.1 (the type) is resolved when the operation runs, in
        // `execute_operation`; the chained steps carry the description.
        self.chain_an_operation(
            OperationSteps::SetLocal {
                type_: description.type_,
                sdp: description.sdp,
            },
            ec,
        )
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setremotedescription>
    pub(crate) fn set_remote_description(
        &self,
        description: RTCSessionDescriptionInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let description be the method's first argument.
        // Step 2: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 3: Return the result of chaining the following steps to
        //         connection's operations chain:
        if self.is_closed() {
            let error = crate::webidl::invalid_state_error_value(ec);
            return crate::webidl::rejected_promise(error, ec);
        }
        self.chain_an_operation(
            OperationSteps::SetRemote {
                description,
                rollback_first: false,
            },
            ec,
        )
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-addicecandidate>
    pub(crate) fn add_ice_candidate(
        &self,
        candidate: RTCIceCandidateInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let candidate be the method's argument.
        // Step 2: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 3: If candidate.candidate is not an empty string and both
        //         candidate.sdpMid and candidate.sdpMLineIndex are null,
        //         return a promise rejected with a newly created TypeError.
        if !candidate.candidate.is_empty()
            && candidate.sdp_mid.is_none()
            && candidate.sdp_m_line_index.is_none()
        {
            let error = ec.new_type_error("sdpMid and sdpMLineIndex are both null");
            return crate::webidl::rejected_promise(error, ec);
        }
        if self.is_closed() {
            let error = crate::webidl::invalid_state_error_value(ec);
            return crate::webidl::rejected_promise(error, ec);
        }
        // Step 4: Return the result of chaining the following steps to
        //         connection's operations chain:
        self.chain_an_operation(OperationSteps::AddIceCandidate { candidate }, ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-createdatachannel>
    pub(crate) fn create_data_channel(
        &self,
        label: String,
        options: super::RTCDataChannelInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<RTCDataChannel, Types> {
        // Step 1: Let connection be the RTCPeerConnection object on which the
        //         method is invoked.
        // Step 2: If connection.[[IsClosed]] is true, throw an
        //         InvalidStateError.
        if self.is_closed() {
            return Err(crate::webidl::invalid_state_error_value(ec));
        }
        // Step 3: Create an RTCDataChannel, channel.
        let (handle, document_origin) = {
            let mut slots = self.slots.borrow_mut();
            slots.next_channel += 1;
            (
                DataChannelHandle(slots.next_channel),
                slots.document_origin.clone(),
            )
        };
        let channel = RTCDataChannel::create(self.id, handle, document_origin, ec)?;
        {
            let mut slots = channel.slots.borrow_mut();
            // Step 4: Initialize channel.[[DataChannelLabel]] to the value of
            //         the first argument.
            slots.label = label;
            // Step 5: If the UTF-8 representation of [[DataChannelLabel]] is
            //         longer than 65535 bytes, throw a TypeError.
            if slots.label.len() > 65535 {
                drop(slots);
                return Err(ec.new_type_error("label is longer than 65535 bytes"));
            }
            // Step 6: Let options be the second argument.
            // Step 7: Initialize channel.[[MaxPacketLifeTime]] to
            //         option.maxPacketLifeTime, if present, otherwise null.
            slots.max_packet_life_time = options.max_packet_life_time;
            // Step 8: Initialize channel.[[MaxRetransmits]] to
            //         option.maxRetransmits, if present, otherwise null.
            slots.max_retransmits = options.max_retransmits;
            // Step 9: Initialize channel.[[Ordered]] to option.ordered.
            slots.ordered = options.ordered;
            // Step 10: Initialize channel.[[DataChannelProtocol]] to
            //          option.protocol.
            slots.protocol = options.protocol;
            // Step 11: If the UTF-8 representation of [[DataChannelProtocol]]
            //          is longer than 65535 bytes, throw a TypeError.
            if slots.protocol.len() > 65535 {
                drop(slots);
                return Err(ec.new_type_error("protocol is longer than 65535 bytes"));
            }
            // Step 12: Initialize channel.[[Negotiated]] to option.negotiated.
            slots.negotiated = options.negotiated;
            // Step 13: Initialize channel.[[DataChannelId]] to the value of
            //          option.id, if it is present and [[Negotiated]] is true,
            //          otherwise null.
            slots.id = if slots.negotiated { options.id } else { None };
            // Step 14: If [[Negotiated]] is true and [[DataChannelId]] is
            //          null, throw a TypeError.
            if slots.negotiated && slots.id.is_none() {
                drop(slots);
                return Err(ec.new_type_error("a negotiated channel needs an id"));
            }
            // Step 15: If both [[MaxPacketLifeTime]] and [[MaxRetransmits]]
            //          attributes are set (not null), throw a TypeError.
            if slots.max_packet_life_time.is_some() && slots.max_retransmits.is_some() {
                drop(slots);
                return Err(ec.new_type_error(
                    "maxPacketLifeTime and maxRetransmits are mutually exclusive",
                ));
            }
            // Step 16: If a setting, either [[MaxPacketLifeTime]] or
            //          [[MaxRetransmits]], has been set to indicate unreliable
            //          mode, and that value exceeds the maximum value
            //          supported by the user agent, the value MUST be set to
            //          the user agents maximum value.
            // Note: The maximum is 65535, the largest value either takes.
            // Step 17: If [[DataChannelId]] is equal to 65535, which is
            //          greater than the maximum allowed ID of 65534 but still
            //          qualifies as an unsigned short, throw a TypeError.
            if slots.id == Some(65535) {
                drop(slots);
                return Err(ec.new_type_error("id 65535 is not allowed"));
            }
            // Step 18: If the [[DataChannelId]] slot is null (due to no ID
            //          being passed into createDataChannel, or [[Negotiated]]
            //          being false), and the DTLS role of the SCTP transport
            //          has already been negotiated, then initialize
            //          [[DataChannelId]] to a value generated by the user
            //          agent, according to [[RFC8832]], and skip to the next
            //          step. If no available ID could be generated, or if the
            //          value of the [[DataChannelId]] slot is being used by an
            //          existing RTCDataChannel, throw an OperationError
            //          exception.
            // Note: The WebRTC process generates the id; the channel learns it
            // when it opens.
            // Step 19: Let transport be connection.[[SctpTransport]]. If the
            //          [[DataChannelId]] slot is not null, transport is in the
            //          "connected" state and [[DataChannelId]] is greater or
            //          equal to transport.[[MaxChannels]], throw an
            //          OperationError.
            // Note: RTCSctpTransport is not implemented.
        }
        // Step 20: If channel is the first RTCDataChannel created on
        //          connection, update the negotiation-needed flag for
        //          connection.
        let first = !std::mem::replace(&mut self.slots.borrow_mut().created_data_channel, true);
        if first {
            self.update_the_negotiation_needed_flag(ec);
        }
        // Step 21: Append channel to connection.[[DataChannels]].
        self.data_channels.borrow_mut(ec).push(channel.clone());
        self.channels.borrow_mut(ec).push(channel.clone());
        // Step 22: Return channel and continue the following steps in
        //          parallel.
        // Step 23: Create channel's associated underlying data transport and
        //          configure it according to the relevant properties of
        //          channel.
        let label = channel.slots.borrow().label.clone();
        send_request(
            Request::CreateDataChannel {
                peer: self.id,
                channel: handle,
                label,
                init: channel.init_for_transport(),
            },
            ec,
        );
        Ok(channel)
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-close>
    pub(crate) fn close(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 2: close the connection with connection and the value false.
        self.close_the_connection(ec);
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-close-the-connection>
    pub(crate) fn close_the_connection(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return;
        }
        {
            let mut slots = self.slots.borrow_mut();
            // Step 2: Set connection.[[IsClosed]] to true.
            slots.is_closed = true;
            // Step 3: Set connection.[[SignalingState]] to "closed". This does
            //         not fire any event.
            slots.signaling_state = RTCSignalingState::Closed;
        }
        // Step 4: Let transceivers be the result of executing the
        //         CollectTransceivers algorithm. For every RTCRtpTransceiver
        //         transceiver in transceivers, run the following steps:
        // Step 4.1: If transceiver.[[Stopped]] is true, abort these sub
        //           steps.
        // Step 4.2: Stop the RTCRtpTransceiver with transceiver and disappear
        //           set to true.
        self.stop_transceivers_on_close(ec);
        // Step 5: Set the [[ReadyState]] slot of each of connection's
        //         RTCDataChannels to "closed".
        let channels = self.data_channels.borrow(ec).clone();
        for channel in channels {
            channel.set_closed();
        }
        // Step 6: If connection.[[SctpTransport]] is not null, tear down the
        //         underlying SCTP association by sending an SCTP ABORT chunk
        //         and set the [[SctpTransportState]] to "closed".
        // Step 7: Set the [[DtlsTransportState]] slot of each of connection's
        //         RTCDtlsTransports to "closed".
        // Step 8: Destroy connection's ICE Agent, abruptly ending any active
        //         ICE processing and releasing any relevant resources (e.g.
        //         TURN permissions).
        // Step 9: Set the [[IceTransportState]] slot of each of connection's
        //         RTCIceTransports to "closed".
        // Note: Steps 6-9 run in the WebRTC process, which drops the peer.
        send_request(Request::Close { peer: self.id }, ec);
        {
            let mut slots = self.slots.borrow_mut();
            // Step 10: Set connection.[[IceConnectionState]] to "closed". This
            //          does not fire any event.
            slots.ice_connection_state = String::from("closed");
            // Step 11: Set connection.[[ConnectionState]] to "closed". This
            //          does not fire any event.
            slots.connection_state = String::from("closed");
        }
        // The realm no longer routes tasks to the connection.
        let id = self.id;
        if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
            global_scope.unregister_peer_connection(id, ec);
            Ok(())
        }) {
            log::error!("[webrtc] unregister peer connection: {}", error.display());
        }
    }

    /// Closing procedure step 3 and closed step 4: remove a channel from
    /// [[DataChannels]].
    pub(crate) fn remove_from_data_channels(
        &self,
        handle: DataChannelHandle,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.data_channels
            .borrow_mut(ec)
            .retain(|channel| channel.handle != handle);
    }

    fn channel(
        &self,
        handle: DataChannelHandle,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<RTCDataChannel> {
        self.channels
            .borrow(ec)
            .iter()
            .find(|channel| channel.handle == handle)
            .cloned()
    }

    // ── The operations chain ───────────────────────────────────────────

    /// <https://w3c.github.io/webrtc-pc/#dfn-chain-an-operation>
    fn chain_an_operation(
        &self,
        steps: OperationSteps,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let connection be the RTCPeerConnection object.
        // Step 2: If connection.[[IsClosed]] is true, return a promise
        //         rejected with a newly created InvalidStateError.
        if self.is_closed() {
            let error = crate::webidl::invalid_state_error_value(ec);
            return crate::webidl::rejected_promise(error, ec);
        }
        // Step 3: Let operation be the operation to be chained.
        // Step 4: Let p be a new promise.
        let (promise, resolve, reject) = new_promise(ec)?;
        // Step 5: Append operation to [[Operations]].
        let length = {
            let mut operations = self.operations.borrow_mut(ec);
            operations.push(ChainedOperation {
                steps,
                in_flight: None,
                promise: promise.clone(),
                resolve,
                reject,
            });
            operations.len()
        };
        // Step 6: If the length of [[Operations]] is exactly 1, execute
        //         operation.
        if length == 1 {
            self.execute_first_operation(ec)?;
        }
        // Step 7: Upon fulfillment or rejection of the promise returned by
        //         the operation, run the following steps:
        // Note: Step 7 runs in `operation_settled`, called when the
        // operation's promise settles.
        // Step 8: Return p.
        Ok(promise)
    }

    fn next_operation_id(&self) -> OperationId {
        let mut slots = self.slots.borrow_mut();
        slots.next_operation += 1;
        OperationId(slots.next_operation)
    }

    fn set_in_flight(
        &self,
        in_flight: Option<(OperationId, InFlight)>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if let Some(first) = self.operations.borrow_mut(ec).first_mut() {
            first.in_flight = in_flight;
        }
    }

    /// Execute the first operation of [[Operations]]: run its synchronous
    /// steps, then either settle its promise or send the request whose
    /// result a task delivers.
    fn execute_first_operation(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some(steps) = self
            .operations
            .borrow(ec)
            .first()
            .map(|operation| operation.steps.clone())
        else {
            return Ok(());
        };
        match steps {
            OperationSteps::CreateOffer => self.creating_an_offer(ec),
            OperationSteps::CreateAnswer => self.creating_an_answer(ec),
            OperationSteps::SetLocal { type_, sdp } => self.set_local_chained_steps(type_, sdp, ec),
            OperationSteps::SetRemote { description, .. } => {
                self.set_remote_chained_steps(description, ec)
            }
            OperationSteps::AddIceCandidate { candidate } => {
                self.add_ice_candidate_chained_steps(candidate, ec)
            }
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-chain-an-operation>, step 7:
    /// the operation's promise settled with `settled`.
    fn operation_settled(
        &self,
        settled: Settled,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 7.1: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        let Some((resolve, reject)) = self
            .operations
            .borrow(ec)
            .first()
            .map(|operation| (operation.resolve.clone(), operation.reject.clone()))
        else {
            return Ok(());
        };
        let undefined = ec.value_undefined();
        match settled {
            // Step 7.2: If the promise returned by operation was fulfilled
            //           with a value, fulfill p with that value.
            Settled::Fulfilled(value) => {
                ec.call(&resolve, &undefined, &[value])?;
            }
            // Step 7.3: If the promise returned by operation was rejected with
            //           a value, reject p with that value.
            Settled::Rejected(reason) => {
                ec.call(&reject, &undefined, &[reason])?;
            }
        }
        // Step 7.4: Upon fulfillment or rejection of p, execute the following
        //           steps:
        // Note: These run right after p settles, in the same task, rather
        // than in a promise reaction job; nothing observable runs in between
        // except p's own reactions, which are queued after this task.
        // Step 7.4.1: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // Step 7.4.2: Remove the first element of [[Operations]].
        let remaining = {
            let mut operations = self.operations.borrow_mut(ec);
            if !operations.is_empty() {
                operations.remove(0);
            }
            operations.len()
        };
        // Step 7.4.3: If [[Operations]] is non-empty, execute the operation
        //             represented by the first element of [[Operations]], and
        //             abort these steps.
        if remaining > 0 {
            return self.execute_first_operation(ec);
        }
        // Step 7.4.4: If connection.[[UpdateNegotiationNeededFlagOnEmptyChain]]
        //             is false, abort these steps.
        // Step 7.4.5: Set connection.[[UpdateNegotiationNeededFlagOnEmptyChain]]
        //             to false.
        let update = std::mem::replace(
            &mut self
                .slots
                .borrow_mut()
                .update_negotiation_needed_flag_on_empty_chain,
            false,
        );
        if !update {
            return Ok(());
        }
        // Step 7.4.6: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        Ok(())
    }

    fn reject_first(
        &self,
        reason: JsValue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        self.operation_settled(Settled::Rejected(reason), ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-offer>
    fn creating_an_offer(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        // Step 1: If connection.[[SignalingState]] is neither "stable" nor
        //         "have-local-offer", return a promise rejected with a newly
        //         created InvalidStateError.
        let state = self.slots.borrow().signaling_state;
        if !matches!(
            state,
            RTCSignalingState::Stable | RTCSignalingState::HaveLocalOffer
        ) {
            let error = crate::webidl::invalid_state_error_value(ec);
            return self.reject_first(error, ec);
        }
        // Step 2: Let p be a new promise.
        // Step 3: In parallel, begin the in-parallel steps to create an offer
        //         given connection and p.
        // Step 4: Return p.
        // Note: The in-parallel steps run in the WebRTC process; the final
        // steps run in the task that delivers its result
        // (`final_steps_to_create_an_offer`).
        let operation = self.next_operation_id();
        self.set_in_flight(Some((operation, InFlight::CreateOffer)), ec);
        send_request(
            Request::CreateOffer {
                peer: self.id,
                operation,
            },
            ec,
        );
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-answer>
    fn creating_an_answer(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        // Step 1: If connection.[[SignalingState]] is neither
        //         "have-remote-offer" nor "have-local-pranswer", return a
        //         promise rejected with a newly created InvalidStateError.
        let state = self.slots.borrow().signaling_state;
        if !matches!(
            state,
            RTCSignalingState::HaveRemoteOffer | RTCSignalingState::HaveLocalPranswer
        ) {
            let error = crate::webidl::invalid_state_error_value(ec);
            return self.reject_first(error, ec);
        }
        // Step 2: Let p be a new promise.
        // Step 3: In parallel, begin the in-parallel steps to create an answer
        //         given connection and p.
        // Step 4: Return p.
        let operation = self.next_operation_id();
        self.set_in_flight(Some((operation, InFlight::CreateAnswer)), ec);
        send_request(
            Request::CreateAnswer {
                peer: self.id,
                operation,
            },
            ec,
        );
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setlocaldescription>,
    /// step 4's chained steps.
    fn set_local_chained_steps(
        &self,
        description_type: Option<RTCSdpType>,
        mut sdp: String,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let (state, last_offer, last_answer) = {
            let slots = self.slots.borrow();
            (
                slots.signaling_state,
                slots.last_created_offer.clone(),
                slots.last_created_answer.clone(),
            )
        };
        // Step 4.1: Let type be description.type if present, or "offer" if not
        //           present and connection.[[SignalingState]] is either
        //           "stable", "have-local-offer", or "have-remote-pranswer";
        //           otherwise "answer".
        let type_ = if let Some(type_) = description_type {
            type_
        } else if matches!(
            state,
            RTCSignalingState::Stable
                | RTCSignalingState::HaveLocalOffer
                | RTCSignalingState::HaveRemotePranswer
        ) {
            RTCSdpType::Offer
        } else {
            RTCSdpType::Answer
        };
        // Step 4.2: If type is "offer", and sdp is not the empty string and
        //           not equal to connection.[[LastCreatedOffer]], then return
        //           a promise rejected with a newly created
        //           InvalidModificationError and abort these steps.
        if type_ == RTCSdpType::Offer && !sdp.is_empty() && sdp != last_offer {
            let error = crate::webidl::invalid_modification_error_value(
                String::from("the offer is not the last created offer"),
                ec,
            );
            return self.reject_first(error, ec);
        }
        // Step 4.3: If type is "answer" or "pranswer", and sdp is not the
        //           empty string and not equal to
        //           connection.[[LastCreatedAnswer]], then return a promise
        //           rejected with a newly created InvalidModificationError and
        //           abort these steps.
        if matches!(type_, RTCSdpType::Answer | RTCSdpType::Pranswer)
            && !sdp.is_empty()
            && sdp != last_answer
        {
            let error = crate::webidl::invalid_modification_error_value(
                String::from("the answer is not the last created answer"),
                ec,
            );
            return self.reject_first(error, ec);
        }
        // Step 4.4: If sdp is the empty string, and type is "offer", then run
        //           the following sub steps:
        if sdp.is_empty() && type_ == RTCSdpType::Offer {
            // Step 4.4.1: Set sdp to the value of connection.[[LastCreatedOffer]].
            sdp = last_offer;
            // Step 4.4.2: If sdp is the empty string, or if it no longer
            //             accurately represents the offerer's system state of
            //             connection, then let p be the result of creating an
            //             offer with connection, and return the result of
            //             reacting to p with a fulfillment step that sets the
            //             local session description indicated by its first
            //             argument.
            if sdp.is_empty() {
                self.retype_first(RTCSdpType::Offer, ec);
                return self.creating_an_offer(ec);
            }
        }
        // Step 4.5: If sdp is the empty string, and type is "answer" or
        //           "pranswer", then run the following sub steps:
        if sdp.is_empty() && matches!(type_, RTCSdpType::Answer | RTCSdpType::Pranswer) {
            // Step 4.5.1: Set sdp to the value of connection.[[LastCreatedAnswer]].
            sdp = last_answer;
            // Step 4.5.2: If sdp is the empty string, or if it no longer
            //             accurately represents the answerer's system state of
            //             connection, then let p be the result of creating an
            //             answer with connection, and return the result of
            //             reacting to p with the following fulfillment steps:
            // Step 4.5.2.1: Let answer be the first argument to these
            //               fulfillment steps.
            // Step 4.5.2.2: Return the result of setting the local session
            //               description indicated by {type, answer.sdp}.
            if sdp.is_empty() {
                self.retype_first(type_, ec);
                return self.creating_an_answer(ec);
            }
        }
        // Step 4.6: Return the result of setting the local session description
        //           indicated by {type, sdp}.
        self.retype_first(type_, ec);
        self.set_a_session_description(RTCSessionDescriptionInit { type_, sdp }, false, ec)
    }

    /// Record the resolved type of the first operation, a setLocalDescription.
    fn retype_first(&self, resolved: RTCSdpType, ec: &mut dyn ExecutionContext<Types>) {
        if let Some(first) = self.operations.borrow_mut(ec).first_mut()
            && let OperationSteps::SetLocal { type_, .. } = &mut first.steps
        {
            *type_ = Some(resolved);
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-setremotedescription>,
    /// step 3's chained steps.
    fn set_remote_chained_steps(
        &self,
        description: RTCSessionDescriptionInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 3.1: If description.type is "offer" and is invalid for the
        //           current connection.[[SignalingState]] as described in
        //           [[!RFC9429]], then run the following sub steps:
        let state = self.slots.borrow().signaling_state;
        if description.type_ == RTCSdpType::Offer
            && !matches!(
                state,
                RTCSignalingState::Stable | RTCSignalingState::HaveRemoteOffer
            )
        {
            // Step 3.1.1: Let p be the result of setting the local session
            //             description indicated by {type: "rollback"}.
            // Step 3.1.2: Return the result of reacting to p with a
            //             fulfillment step that sets the remote session
            //             description description, and abort these steps.
            if let Some(first) = self.operations.borrow_mut(ec).first_mut()
                && let OperationSteps::SetRemote { rollback_first, .. } = &mut first.steps
            {
                *rollback_first = true;
            }
            return self.set_a_session_description(
                RTCSessionDescriptionInit {
                    type_: RTCSdpType::Rollback,
                    sdp: String::new(),
                },
                false,
                ec,
            );
        }
        // Step 3.2: Return the result of setting the remote session
        //           description description.
        self.set_a_session_description(description, true, ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#set-description>, steps 1-4 up to
    /// the in-parallel work: the synchronous checks, then the request whose
    /// result the task of step 4.6 or 4.7 handles.
    fn set_a_session_description(
        &self,
        description: RTCSessionDescriptionInit,
        remote: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let p be a new promise.
        // Step 2: If description.type is "rollback" and
        //         connection.[[SignalingState]] is either "stable",
        //         "have-local-pranswer", or "have-remote-pranswer", then reject
        //         p with a newly created InvalidStateError and abort these
        //         steps.
        let state = self.slots.borrow().signaling_state;
        if description.type_ == RTCSdpType::Rollback
            && matches!(
                state,
                RTCSignalingState::Stable
                    | RTCSignalingState::HaveLocalPranswer
                    | RTCSignalingState::HaveRemotePranswer
            )
        {
            let error = crate::webidl::invalid_state_error_value(ec);
            return self.reject_first(error, ec);
        }
        // Step 3: Let jsepSetOfTransceivers be a shallow copy of connection's
        //         set of transceivers.
        // Note: Transceivers are not implemented.
        // Step 4: In parallel, start the process to apply description as
        //         described in [[!RFC9429]], with these additional
        //         restrictions:
        // Note: The WebRTC process applies the description; the task that
        // delivers its result runs step 4.6 or 4.7
        // (`set_description_applied`).
        let operation = self.next_operation_id();
        let request = if remote {
            self.set_in_flight(Some((operation, InFlight::SetRemote)), ec);
            Request::SetRemoteDescription {
                peer: self.id,
                operation,
                description: description.to_ipc(),
            }
        } else {
            self.set_in_flight(Some((operation, InFlight::SetLocal)), ec);
            Request::SetLocalDescription {
                peer: self.id,
                operation,
                description: description.to_ipc(),
            }
        };
        send_request(request, ec);
        // Step 5: Return p.
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-peerconnection-addicecandidate>,
    /// step 4's chained steps.
    fn add_ice_candidate_chained_steps(
        &self,
        candidate: RTCIceCandidateInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 4.1: If remoteDescription is null return a promise rejected
        //           with a newly created InvalidStateError.
        let Some(remote) = self.remote_description_init() else {
            let error = crate::webidl::invalid_state_error_value(ec);
            return self.reject_first(error, ec);
        };
        let sections = sdp::media_descriptions(&remote.sdp);
        let section = if let Some(mid) = &candidate.sdp_mid {
            // Step 4.2: If candidate.sdpMid is not null, run the following
            //           steps:
            // Step 4.2.1: If candidate.sdpMid is not equal to the mid of any
            //             media description in remoteDescription, return a
            //             promise rejected with a newly created
            //             OperationError.
            match sections
                .iter()
                .find(|section| section.mid.as_deref() == Some(mid.as_str()))
            {
                Some(section) => Some(section),
                None => {
                    let error = crate::webidl::operation_error_value(
                        String::from("sdpMid matches no media description"),
                        ec,
                    );
                    return self.reject_first(error, ec);
                }
            }
        } else if let Some(index) = candidate.sdp_m_line_index {
            // Step 4.3: Else, if candidate.sdpMLineIndex is not null, run the
            //           following steps:
            // Step 4.3.1: If candidate.sdpMLineIndex is equal to or larger
            //             than the number of media descriptions in
            //             remoteDescription, return a promise rejected with a
            //             newly created OperationError.
            match sections.get(usize::from(index)) {
                Some(section) => Some(section),
                None => {
                    let error = crate::webidl::operation_error_value(
                        String::from("sdpMLineIndex is out of range"),
                        ec,
                    );
                    return self.reject_first(error, ec);
                }
            }
        } else {
            None
        };
        // Step 4.4: If either candidate.sdpMid or candidate.sdpMLineIndex
        //           indicate a media description in remoteDescription whose
        //           associated transceiver is stopped, return a promise
        //           resolved with undefined.
        // Note: Transceivers are not implemented.
        // Step 4.5: If candidate.usernameFragment is not null, and is not
        //           equal to any username fragment present in the
        //           corresponding media description of an applied remote
        //           description, return a promise rejected with a newly
        //           created OperationError.
        if let (Some(ufrag), Some(section)) = (&candidate.username_fragment, section)
            && section.ice_ufrag.as_deref() != Some(ufrag.as_str())
        {
            let error = crate::webidl::operation_error_value(
                String::from("usernameFragment matches no ICE generation"),
                ec,
            );
            return self.reject_first(error, ec);
        }
        // Step 4.6: Let p be a new promise.
        // Step 4.7: In parallel, if the candidate is not administratively
        //           prohibited, add the ICE candidate candidate as described
        //           in [[!RFC9429]]. ... If candidate.candidate is an empty
        //           string, process candidate as an end-of-candidates
        //           indication for the corresponding media description and
        //           ICE candidate generation.
        // Note: The WebRTC process adds the candidate; the task delivering
        // its result runs step 4.7.1 or 4.7.2 (`add_ice_candidate_done`).
        let operation = self.next_operation_id();
        self.set_in_flight(Some((operation, InFlight::AddIceCandidate)), ec);
        let ipc_candidate = if candidate.candidate.is_empty() {
            None
        } else {
            Some(candidate.to_ipc())
        };
        send_request(
            Request::AddIceCandidate {
                peer: self.id,
                operation,
                candidate: ipc_candidate,
            },
            ec,
        );
        // Step 4.8: Return p.
        Ok(())
    }

    // ── Tasks from the WebRTC process ──────────────────────────────────

    /// Run one WebRTC task for this connection.
    pub(crate) fn run_task(
        &self,
        task: WebRtcTask,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        match task {
            WebRtcTask::UpdateNegotiationNeededFlag => {
                self.negotiation_needed_task(time_millis, ec)
            }
            WebRtcTask::Ipc(Message::Completed {
                operation,
                result: OperationResult::Stats(report),
            }) => self.stats_completed(operation, &report, ec),
            WebRtcTask::Ipc(Message::Completed { operation, result }) => {
                self.operation_completed(operation, result, time_millis, ec)
            }
            WebRtcTask::Ipc(Message::Event(event)) => self.peer_event(event, time_millis, ec),
        }
    }

    /// A result of the WebRTC process for the first operation of
    /// [[Operations]].
    fn operation_completed(
        &self,
        operation: OperationId,
        result: OperationResult,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some((steps, in_flight)) = self.operations.borrow(ec).first().and_then(|first| {
            first
                .in_flight
                .filter(|(id, _)| *id == operation)
                .map(|(_, kind)| (first.steps.clone(), kind))
        }) else {
            return Ok(());
        };
        self.set_in_flight(None, ec);
        match (in_flight, result) {
            (InFlight::CreateOffer, OperationResult::Description(description)) => {
                let sdp = self.final_steps_to_create_an_offer(description)?;
                match steps {
                    // setLocalDescription step 4.4.2: the fulfillment step
                    // sets the local session description indicated by the
                    // offer.
                    OperationSteps::SetLocal { .. } => {
                        let Some(sdp) = sdp else { return Ok(()) };
                        self.set_a_session_description(
                            RTCSessionDescriptionInit {
                                type_: RTCSdpType::Offer,
                                sdp,
                            },
                            false,
                            ec,
                        )
                    }
                    _ => match sdp {
                        // Step 6: Resolve p with offer.
                        Some(sdp) => {
                            let offer = description_init_object(RTCSdpType::Offer, &sdp, ec)?;
                            self.operation_settled(
                                Settled::Fulfilled(Types::value_from_object(offer)),
                                ec,
                            )
                        }
                        None => Ok(()),
                    },
                }
            }
            (InFlight::CreateAnswer, OperationResult::Description(description)) => {
                let sdp = self.final_steps_to_create_an_answer(description);
                match steps {
                    // setLocalDescription steps 4.5.2.1-4.5.2.2.
                    OperationSteps::SetLocal { type_, .. } => self.set_a_session_description(
                        RTCSessionDescriptionInit {
                            type_: type_.unwrap_or(RTCSdpType::Answer),
                            sdp,
                        },
                        false,
                        ec,
                    ),
                    _ => {
                        // Step 5: Resolve p with answer.
                        let answer = description_init_object(RTCSdpType::Answer, &sdp, ec)?;
                        self.operation_settled(
                            Settled::Fulfilled(Types::value_from_object(answer)),
                            ec,
                        )
                    }
                }
            }
            (
                InFlight::SetLocal | InFlight::SetRemote,
                result @ (OperationResult::Done | OperationResult::Negotiated(_)),
            ) => {
                let states = match result {
                    OperationResult::Negotiated(states) => states,
                    _ => Vec::new(),
                };
                let remote = in_flight == InFlight::SetRemote;
                let description = match (&steps, remote) {
                    (
                        OperationSteps::SetRemote {
                            rollback_first: true,
                            ..
                        },
                        false,
                    ) => RTCSessionDescriptionInit {
                        type_: RTCSdpType::Rollback,
                        sdp: String::new(),
                    },
                    (OperationSteps::SetRemote { description, .. }, true) => description.clone(),
                    (OperationSteps::SetLocal { type_, sdp }, false) => {
                        let type_ = type_.unwrap_or(RTCSdpType::Offer);
                        let sdp = if sdp.is_empty() {
                            let slots = self.slots.borrow();
                            match type_ {
                                RTCSdpType::Offer => slots.last_created_offer.clone(),
                                _ => slots.last_created_answer.clone(),
                            }
                        } else {
                            sdp.clone()
                        };
                        RTCSessionDescriptionInit { type_, sdp }
                    }
                    _ => return Ok(()),
                };
                let continue_with_remote = match &steps {
                    OperationSteps::SetRemote {
                        rollback_first: true,
                        description: remote_description,
                    } if !remote => Some(remote_description.clone()),
                    _ => None,
                };
                let applied =
                    self.set_description_applied(&description, remote, time_millis, ec)?;
                // Steps 4.7.10 to 4.7.16: the transceivers the description
                // negotiated, and the track events for remote media.
                if applied {
                    self.apply_transceiver_states(states, remote, time_millis, ec)?;
                }
                match (applied, continue_with_remote) {
                    // setRemoteDescription step 3.1.2: after the rollback,
                    // set the remote session description.
                    (true, Some(remote_description)) => {
                        if let Some(first) = self.operations.borrow_mut(ec).first_mut()
                            && let OperationSteps::SetRemote { rollback_first, .. } =
                                &mut first.steps
                        {
                            *rollback_first = false;
                        }
                        self.set_a_session_description(remote_description, true, ec)
                    }
                    // Step 4.7.21: Resolve p with undefined.
                    (true, None) => {
                        let undefined = ec.value_undefined();
                        self.operation_settled(Settled::Fulfilled(undefined), ec)
                    }
                    (false, _) => Ok(()),
                }
            }
            (InFlight::AddIceCandidate, OperationResult::Done) => {
                // Step 4.7.2: If candidate is applied successfully, ... the
                //             user agent MUST queue a task that runs the
                //             following steps:
                // Step 4.7.2.1: If connection.[[IsClosed]] is true, then abort
                //               these steps.
                if self.is_closed() {
                    return Ok(());
                }
                // Step 4.7.2.2: If connection.[[PendingRemoteDescription]] is
                //               not null, and represents the ICE generation
                //               for which candidate was processed, add
                //               candidate to
                //               connection.[[PendingRemoteDescription]].sdp.
                // Step 4.7.2.3: If connection.[[CurrentRemoteDescription]] is
                //               not null, and represents the ICE generation
                //               for which candidate was processed, add
                //               candidate to
                //               connection.[[CurrentRemoteDescription]].sdp.
                // Note: The description objects keep the SDP as set; added
                // candidates are not written into it.
                // Step 4.7.2.4: Resolve p with undefined.
                let undefined = ec.value_undefined();
                self.operation_settled(Settled::Fulfilled(undefined), ec)
            }
            (kind, OperationResult::Error { name, message }) => {
                if self.is_closed() {
                    return Ok(());
                }
                let reason = match kind {
                    // Set a session description, steps 4.6.2-4.6.7; the
                    // WebRTC process names the error. addIceCandidate step
                    // 4.7.1.2: Reject p with a newly created OperationError.
                    InFlight::AddIceCandidate => crate::webidl::operation_error_value(message, ec),
                    // The in-parallel steps to create an offer or answer,
                    // step 3: reject p with a newly created OperationError.
                    InFlight::CreateOffer | InFlight::CreateAnswer => {
                        crate::webidl::operation_error_value(message, ec)
                    }
                    InFlight::SetLocal | InFlight::SetRemote => {
                        crate::webidl::named_dom_exception_value(name, message, ec)
                    }
                };
                self.reject_first(reason, ec)
            }
            (_, _) => {
                let error = crate::webidl::operation_error_value(
                    String::from("unexpected result from the WebRTC process"),
                    ec,
                );
                self.reject_first(error, ec)
            }
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-final-steps-to-create-an-offer>
    /// Returns the offer's SDP, or None when the steps aborted.
    fn final_steps_to_create_an_offer(
        &self,
        description: SessionDescription,
    ) -> Completion<Option<String>, Types> {
        // Step 1: If connection.[[IsClosed]] is true, then abort these steps.
        if self.is_closed() {
            return Ok(None);
        }
        // Step 2: If connection was modified in such a way that additional
        //         inspection of the offerer's system state is necessary, then
        //         in parallel begin the in-parallel steps to create an offer
        //         again, given connection and p, and abort these steps.
        // Note: The WebRTC process generated the offer from its current
        // state; nothing is inspected on this side.
        // Step 3: Given the information that was obtained from previous
        //         inspection, the current state of connection and its
        //         RTCRtpTransceivers, generate an SDP offer, sdpString, as
        //         described in [[!RFC9429]].
        let sdp_string = description.sdp;
        // Step 4: Let offer be a newly created RTCSessionDescriptionInit
        //         dictionary with its type member initialized to the string
        //         "offer" and its sdp member initialized to sdpString.
        // Step 5: Set the [[LastCreatedOffer]] internal slot to sdpString.
        self.slots.borrow_mut().last_created_offer = sdp_string.clone();
        // Step 6: Resolve p with offer.
        // Note: The caller resolves p.
        Ok(Some(sdp_string))
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-final-steps-to-create-an-answer>
    fn final_steps_to_create_an_answer(&self, description: SessionDescription) -> String {
        // Step 1: Let filteredCodecs be the result of applying the following
        //         filter on transceiver.[[PreferredCodecs]].
        // Step 2: If this is an answer to an offer to receive simulcast, then
        //         for each media section requesting to receive simulcast, run
        //         the following steps:
        // Note: Transceivers are not implemented.
        // Step 3: Let answer be a newly created RTCSessionDescriptionInit
        //         dictionary with its type member initialized to the string
        //         "answer" and its sdp member initialized to sdpString.
        let sdp_string = description.sdp;
        // Step 4: Set the [[LastCreatedAnswer]] internal slot to sdpString.
        self.slots.borrow_mut().last_created_answer = sdp_string.clone();
        // Step 5: Resolve p with answer.
        // Note: The caller resolves p.
        sdp_string
    }

    /// <https://w3c.github.io/webrtc-pc/#set-description>, step 4.7: the
    /// task that runs once description was applied. Returns false when the
    /// steps aborted.
    fn set_description_applied(
        &self,
        description: &RTCSessionDescriptionInit,
        remote: bool,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<bool, Types> {
        // Step 4.7.1: If connection.[[IsClosed]] is true, then abort these
        //             steps.
        if self.is_closed() {
            return Ok(false);
        }
        // Step 4.7.2: If remote is true and description is of type "offer",
        //             then if any addTrack() methods on connection succeeded
        //             during the process to apply description, abort these
        //             steps and start the process over as if they had
        //             succeeded prior, to include the extra transceiver(s) in
        //             the process.
        // Step 4.7.3: If any promises from setParameters methods on
        //             RTCRtpSenders associated with connection are not
        //             settled, abort these steps and start the process over.
        // Note: Tracks and senders are not implemented.
        let state_before = self.slots.borrow().signaling_state;
        // Step 4.7.4: If description is of type "offer" and
        //             connection.[[SignalingState]] is "stable", then run the
        //             following steps:
        // Step 4.7.4.1: Set connection.[[LastStableStateSctpTransport]] to
        //               connection.[[SctpTransport]].
        // Step 4.7.4.2: For each transceiver in connection's set of
        //               transceivers, run the following steps:
        // Note: RTCSctpTransport and transceivers are not implemented.
        let new_object = RTCSessionDescription::new_object(description, ec)?;
        let mut release_early_candidates = false;
        if !remote {
            // Step 4.7.5: If remote is false, then run one of the following
            //             steps:
            match description.type_ {
                // Step 4.7.5.1: If description is of type "offer", set
                //               connection.[[PendingLocalDescription]] to a
                //               new RTCSessionDescription object constructed
                //               from description, set
                //               connection.[[SignalingState]] to
                //               "have-local-offer", and release early
                //               candidates.
                RTCSdpType::Offer => {
                    self.pending_local_description.set(Some(new_object), ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.pending_local = Some(description.clone());
                    slots.signaling_state = RTCSignalingState::HaveLocalOffer;
                    release_early_candidates = true;
                }
                // Step 4.7.5.2: If description is of type "answer", then this
                //               completes an offer answer negotiation. Set
                //               connection.[[CurrentLocalDescription]] to a
                //               new RTCSessionDescription object constructed
                //               from description, and set
                //               connection.[[CurrentRemoteDescription]] to
                //               connection.[[PendingRemoteDescription]]. Set
                //               both connection.[[PendingRemoteDescription]]
                //               and connection.[[PendingLocalDescription]] to
                //               null. Set both connection.[[LastCreatedOffer]]
                //               and connection.[[LastCreatedAnswer]] to "", set
                //               connection.[[SignalingState]] to "stable", and
                //               release early candidates. Finally, if none of
                //               the ICE credentials in
                //               connection.[[LocalIceCredentialsToReplace]] are
                //               present in description, then set
                //               connection.[[LocalIceCredentialsToReplace]] to
                //               an empty set.
                RTCSdpType::Answer => {
                    let pending_remote_object = self.pending_remote_description.borrow(ec).clone();
                    self.current_local_description.set(Some(new_object), ec);
                    self.current_remote_description
                        .set(pending_remote_object, ec);
                    self.pending_remote_description.set(None, ec);
                    self.pending_local_description.set(None, ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.current_local = Some(description.clone());
                    slots.current_remote = slots.pending_remote.take();
                    slots.pending_local = None;
                    slots.last_created_offer.clear();
                    slots.last_created_answer.clear();
                    slots.signaling_state = RTCSignalingState::Stable;
                    release_early_candidates = true;
                }
                // Step 4.7.5.3: If description is of type "pranswer", then set
                //               connection.[[PendingLocalDescription]] to a
                //               new RTCSessionDescription object constructed
                //               from description, set
                //               connection.[[SignalingState]] to
                //               "have-local-pranswer", and release early
                //               candidates.
                RTCSdpType::Pranswer => {
                    self.pending_local_description.set(Some(new_object), ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.pending_local = Some(description.clone());
                    slots.signaling_state = RTCSignalingState::HaveLocalPranswer;
                    release_early_candidates = true;
                }
                RTCSdpType::Rollback => {}
            }
        } else {
            // Step 4.7.6: Otherwise, (if remote is true) run one of the
            //             following steps:
            match description.type_ {
                // Step 4.7.6.1: If description is of type "offer", set
                //               connection.[[PendingRemoteDescription]]
                //               attribute to a new RTCSessionDescription
                //               object constructed from description, and set
                //               connection.[[SignalingState]] to
                //               "have-remote-offer".
                RTCSdpType::Offer => {
                    self.pending_remote_description.set(Some(new_object), ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.pending_remote = Some(description.clone());
                    slots.signaling_state = RTCSignalingState::HaveRemoteOffer;
                }
                // Step 4.7.6.2: If description is of type "answer", then this
                //               completes an offer answer negotiation. Set
                //               connection.[[CurrentRemoteDescription]] to a
                //               new RTCSessionDescription object constructed
                //               from description, and set
                //               connection.[[CurrentLocalDescription]] to
                //               connection.[[PendingLocalDescription]]. Set
                //               both connection.[[PendingRemoteDescription]]
                //               and connection.[[PendingLocalDescription]] to
                //               null. Set both connection.[[LastCreatedOffer]]
                //               and connection.[[LastCreatedAnswer]] to "", and
                //               set connection.[[SignalingState]] to "stable".
                //               Finally, if none of the ICE credentials in
                //               connection.[[LocalIceCredentialsToReplace]] are
                //               present in the newly set
                //               connection.[[CurrentLocalDescription]], then
                //               set connection.[[LocalIceCredentialsToReplace]]
                //               to an empty set.
                RTCSdpType::Answer => {
                    let pending_local_object = self.pending_local_description.borrow(ec).clone();
                    self.current_remote_description.set(Some(new_object), ec);
                    self.current_local_description.set(pending_local_object, ec);
                    self.pending_remote_description.set(None, ec);
                    self.pending_local_description.set(None, ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.current_remote = Some(description.clone());
                    slots.current_local = slots.pending_local.take();
                    slots.pending_remote = None;
                    slots.last_created_offer.clear();
                    slots.last_created_answer.clear();
                    slots.signaling_state = RTCSignalingState::Stable;
                }
                // Step 4.7.6.3: If description is of type "pranswer", then set
                //               connection.[[PendingRemoteDescription]] to a
                //               new RTCSessionDescription object constructed
                //               from description and set
                //               connection.[[SignalingState]] to
                //               "have-remote-pranswer".
                RTCSdpType::Pranswer => {
                    self.pending_remote_description.set(Some(new_object), ec);
                    let mut slots = self.slots.borrow_mut();
                    slots.pending_remote = Some(description.clone());
                    slots.signaling_state = RTCSignalingState::HaveRemotePranswer;
                }
                RTCSdpType::Rollback => {}
            }
        }
        // Step 4.7.7: If description is of type "offer", and it describes an
        //             SCTP association as defined in [[RFC8841]] Section 10.2,
        //             and connection.[[SctpTransport]] is null, set
        //             connection.[[SctpTransport]] to the result of creating
        //             an RTCSctpTransport with an initial state of
        //             "connecting".
        // Step 4.7.8: If description is of type "answer", and it initiates the
        //             closure of an existing SCTP association, as defined in
        //             [[RFC8841]], Sections 10.3 and 10.4, set
        //             connection.[[SctpTransport]] to null.
        // Step 4.7.9: Let trackEventInits, muteTracks, addList, removeList and
        //             errorList be empty lists.
        // Step 4.7.10: If description is of type "answer" or "pranswer", then
        //              run the following steps:
        // Step 4.7.10.1: If description initiates the establishment of a new
        //                SCTP association, as defined in [[RFC8841]], Sections
        //                10.3 and 10.4, or an SCTP association already exists,
        //                update the data max message size of
        //                connection.[[SctpTransport]].
        // Step 4.7.10.2: If description negotiates the DTLS role of the SCTP
        //                transport, then for each RTCDataChannel, channel,
        //                with a null id, run the following step:
        // Step 4.7.10.2.1: Give channel a new ID generated according to
        //                  [[RFC8832]]. If no available ID could be generated,
        //                  set channel.[[ReadyState]] to "closed", and add
        //                  channnel to errorList.
        // Note: RTCSctpTransport is not implemented, and the WebRTC process
        // generates the channel ids; a channel learns its id when it opens,
        // and an id failure reaches it as a data channel error.
        // Step 4.7.11: If description is not of type "rollback", then run the
        //              following steps:
        // Note: Steps 4.7.11.1-4.7.11.2 apply media descriptions to
        // transceivers, which are not implemented.
        if description.type_ == RTCSdpType::Rollback {
            // Step 4.7.12: Otherwise, (if description is of type "rollback")
            //              run the following steps:
            // Step 4.7.12.1: Let pendingDescription be either
            //                connection.[[PendingLocalDescription]] or
            //                connection.[[PendingRemoteDescription]], whichever
            //                one is not null.
            // Step 4.7.12.2: Set connection.[[SctpTransport]] to
            //                connection.[[LastStableStateSctpTransport]].
            // Step 4.7.12.3: For each transceiver in the connection's set of
            //                transceivers run the following steps:
            // Note: RTCSctpTransport and transceivers are not implemented.
            // Step 4.7.12.4: Set connection.[[PendingLocalDescription]] and
            //                connection.[[PendingRemoteDescription]] to null,
            //                and set connection.[[SignalingState]] to "stable".
            self.pending_local_description.set(None, ec);
            self.pending_remote_description.set(None, ec);
            let mut slots = self.slots.borrow_mut();
            slots.pending_local = None;
            slots.pending_remote = None;
            slots.signaling_state = RTCSignalingState::Stable;
        }
        // Step 4.7.13: If description is of type "answer", then run the
        //              following steps:
        // Step 4.7.13.1: For each transceiver in the connection's set of
        //                transceivers run the following steps:
        // Note: Transceivers are not implemented.
        let state_after = self.slots.borrow().signaling_state;
        // Step 4.7.14: If connection.[[SignalingState]] is now "stable", run
        //              the following steps:
        if state_after == RTCSignalingState::Stable {
            // Step 4.7.14.1: For any transceiver that was removed from the set
            //                of transceivers in a previous step, ...
            // Step 4.7.14.2: For each transceiver in connection's set of
            //                transceivers:
            // Note: Transceivers are not implemented.
            // Step 4.7.14.3: Clear the negotiation-needed flag and update the
            //                negotiation-needed flag.
            self.slots.borrow_mut().negotiation_needed = false;
            self.update_the_negotiation_needed_flag(ec);
        }
        // Step 4.7.15: If connection.[[SignalingState]] changed above, fire an
        //              event named signalingstatechange at connection.
        if state_after != state_before {
            fire_event(ec, self, "signalingstatechange", time_millis, false)?;
        }
        // Step 4.7.16: For each channel in errorList, fire an event named
        //              error using the RTCErrorEvent interface with the
        //              errorDetail attribute set to "data-channel-failure" at
        //              channel.
        // Step 4.7.17: For each track in muteTracks, set the muted state of
        //              track to the value true.
        // Step 4.7.18: For each stream and track pair in removeList, remove
        //              the track track from stream.
        // Step 4.7.19: For each stream and track pair in addList, add the
        //              track track to stream.
        // Step 4.7.20: For each entry entry in trackEventInits, fire an event
        //              named track using the RTCTrackEvent interface ...
        // Note: The lists stay empty (see steps 4.7.10 and 4.7.11).
        // "Release early candidates": surface each candidate of
        // [[EarlyCandidates]], then clear it.
        // <https://w3c.github.io/webrtc-pc/#dfn-release-early-candidates>
        if release_early_candidates {
            let early = std::mem::take(&mut self.slots.borrow_mut().early_candidates);
            for candidate in early {
                self.surface_a_candidate(candidate, time_millis, ec)?;
            }
        }
        // Step 4.7.21: Resolve p with undefined.
        // Note: The caller resolves p.
        Ok(true)
    }

    // ── Negotiation-needed ─────────────────────────────────────────────

    /// <https://w3c.github.io/webrtc-pc/#dfn-update-the-negotiation-needed-flag>
    pub(super) fn update_the_negotiation_needed_flag(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: If the length of connection.[[Operations]] is not 0, then
        //         set connection.[[UpdateNegotiationNeededFlagOnEmptyChain]]
        //         to true, and abort these steps.
        if !self.operations.borrow(ec).is_empty() {
            self.slots
                .borrow_mut()
                .update_negotiation_needed_flag_on_empty_chain = true;
            return;
        }
        // Step 2: Queue a task to run the following steps:
        // Note: The steps run in `negotiation_needed_task`.
        let Some(document_id) = self.slots.borrow().document_id else {
            return;
        };
        let peer = self.id;
        if let Err(error) = with_global_scope(ec, move |global_scope, _ec| {
            if let Ok(task_sources) = global_scope.task_sources() {
                task_sources.task_queue().queue_a_task(Task::WebRtc {
                    document_id,
                    peer,
                    task: WebRtcTask::UpdateNegotiationNeededFlag,
                });
            }
            Ok(())
        }) {
            log::error!(
                "[webrtc] queue negotiation-needed task: {}",
                error.display()
            );
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-update-the-negotiation-needed-flag>,
    /// the queued task of step 2.
    fn negotiation_needed_task(
        &self,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 2.1: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // Step 2.2: If the length of connection.[[Operations]] is not 0, then
        //           set connection.[[UpdateNegotiationNeededFlagOnEmptyChain]]
        //           to true, and abort these steps.
        if !self.operations.borrow(ec).is_empty() {
            self.slots
                .borrow_mut()
                .update_negotiation_needed_flag_on_empty_chain = true;
            return Ok(());
        }
        // Step 2.3: If connection.[[SignalingState]] is not "stable", abort
        //           these steps.
        if self.slots.borrow().signaling_state != RTCSignalingState::Stable {
            return Ok(());
        }
        // Step 2.4: If the result of checking if negotiation is needed is
        //           false, clear the negotiation-needed flag by setting
        //           connection.[[NegotiationNeeded]] to false, and abort these
        //           steps.
        if !self.check_if_negotiation_is_needed(ec) {
            self.slots.borrow_mut().negotiation_needed = false;
            return Ok(());
        }
        // Step 2.5: If connection.[[NegotiationNeeded]] is already true, abort
        //           these steps.
        // Step 2.6: Set connection.[[NegotiationNeeded]] to true.
        if std::mem::replace(&mut self.slots.borrow_mut().negotiation_needed, true) {
            return Ok(());
        }
        // Step 2.7: Fire an event named negotiationneeded at connection.
        fire_event(ec, self, "negotiationneeded", time_millis, false).map(|_| ())
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-check-if-negotiation-is-needed>
    fn check_if_negotiation_is_needed(&self, ec: &mut dyn ExecutionContext<Types>) -> bool {
        let slots = self.slots.borrow();
        // Step 1: If any implementation-specific negotiation is required, as
        //         described at the start of this section, return true.
        // Step 2: If connection.[[LocalIceCredentialsToReplace]] is not
        //         empty, return true.
        // Note: No implementation-specific negotiation, and restartIce() is
        // not implemented.
        // Step 3: Let description be connection.[[CurrentLocalDescription]].
        let description = slots.current_local.as_ref();
        // Step 4: If connection has created any RTCDataChannels, and no m=
        //         section in description has been negotiated yet for data,
        //         return true.
        if slots.created_data_channel
            && !description.is_some_and(|description| sdp::negotiates_data(&description.sdp))
        {
            return true;
        }
        // Step 5: For each transceiver in connection's set of transceivers,
        //         perform the following checks:
        drop(slots);
        if self.transceivers_need_negotiation(ec) {
            return true;
        }

        // Step 6: If all the preceding checks were performed and true was not
        //         returned, nothing remains to be negotiated; return false.
        false
    }

    // ── ICE and connection state ───────────────────────────────────────

    /// An event of the connection's ICE Agent, DTLS or SCTP transport.
    fn peer_event(
        &self,
        event: PeerEvent,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        match event {
            PeerEvent::IceCandidate(Some(candidate)) => {
                self.new_candidate(RTCIceCandidateInit::from_ipc(candidate), time_millis, ec)
            }
            PeerEvent::IceCandidate(None) => self.end_of_candidates(time_millis, ec),
            PeerEvent::IceGatheringState(state) => {
                self.gathering_state_changed(state, time_millis, ec)
            }
            PeerEvent::IceConnectionState(state) => {
                // <https://w3c.github.io/webrtc-pc/#dfn-change-the-selected-candidate-pair-and-state>
                // If connection.[[IsClosed]] is true, abort these steps.
                if self.is_closed() {
                    return Ok(());
                }
                // Set connection.[[IceConnectionState]] to the value of
                // deriving a new state value as described by the
                // RTCIceConnectionState enum.
                // Note: The WebRTC process derives the state.
                let changed = {
                    let mut slots = self.slots.borrow_mut();
                    let changed = slots.ice_connection_state != state;
                    slots.ice_connection_state = state;
                    changed
                };
                // If connectionIceConnectionStateChanged is true, fire an
                // event named iceconnectionstatechange at connection.
                if changed {
                    fire_event(ec, self, "iceconnectionstatechange", time_millis, false)?;
                }
                Ok(())
            }
            PeerEvent::ConnectionState(state) => {
                // <https://w3c.github.io/webrtc-pc/#update-the-connection-state>
                // Step 1: Let connection be this RTCPeerConnection object
                //         associated with the RTCDtlsTransport object whose
                //         state changed.
                // Step 2: If connection.[[IsClosed]] is true, abort these
                //         steps.
                if self.is_closed() {
                    return Ok(());
                }
                // Step 3: Let newState be the value of deriving a new state
                //         value as described by the RTCPeerConnectionState
                //         enum.
                // Note: The WebRTC process derives the state.
                // Step 4: If connection.[[ConnectionState]] is equal to
                //         newState, abort these steps.
                if self.slots.borrow().connection_state == state {
                    return Ok(());
                }
                // Step 5: Set connection.[[ConnectionState]] to newState.
                let connected = state == "connected";
                self.slots.borrow_mut().connection_state = state;
                // Media flows once the transports connect: the receivers'
                // tracks leave their initial muted state.
                if connected {
                    self.receiving_tracks_unmuted(time_millis, ec)?;
                }
                // Step 6: Fire an event named connectionstatechange at
                //         connection.
                fire_event(ec, self, "connectionstatechange", time_millis, false).map(|_| ())
            }
            // Note: Content decides negotiation-needed itself (see
            // `update_the_negotiation_needed_flag`).
            PeerEvent::NegotiationNeeded => Ok(()),
            PeerEvent::DataChannel(properties) => {
                self.announce_data_channel(properties, time_millis, ec)
            }
            PeerEvent::DataChannelOpen { channel, id } => {
                let closed = self.is_closed();
                match self.channel(channel, ec) {
                    Some(channel) => channel.announce_open(closed, id, time_millis, ec),
                    None => Ok(()),
                }
            }
            PeerEvent::DataChannelMessage { channel, payload } => match self.channel(channel, ec) {
                Some(channel) => channel.receive_message(payload, time_millis, ec),
                None => Ok(()),
            },
            PeerEvent::DataChannelBufferedAmount {
                channel,
                through,
                amount,
            } => match self.channel(channel, ec) {
                Some(channel) => channel.update_buffered_amount(through, amount, time_millis, ec),
                None => Ok(()),
            },
            PeerEvent::DataChannelError { channel, message } => {
                log::debug!("[webrtc] channel {}: {message}", channel.0);
                if let Some(channel) = self.channel(channel, ec) {
                    channel.slots.borrow_mut().transport_error = true;
                }
                Ok(())
            }
            PeerEvent::DataChannelClosed { channel: handle } => {
                let Some(channel) = self.channel(handle, ec) else {
                    return Ok(());
                };
                self.channels
                    .borrow_mut(ec)
                    .retain(|channel| channel.handle != handle);
                let with_error = channel.slots.borrow().transport_error;
                channel.transport_closed(with_error, time_millis, ec)
            }
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#rtcicetransport-gathering>: "When
    /// the ICE Agent indicates that a new ICE candidate is available".
    fn new_candidate(
        &self,
        candidate: RTCIceCandidateInit,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Let candidate be the available ICE candidate.
        // Let connection be the RTCPeerConnection object associated with this
        // ICE Agent.
        // If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // If either connection.[[PendingLocalDescription]] or
        // connection.[[CurrentLocalDescription]] are not null, and represent
        // the ICE generation for which candidate was gathered, surface the
        // candidate with candidate and connection, and abort these steps.
        if self.local_description_init().is_some() {
            return self.surface_a_candidate(candidate, time_millis, ec);
        }
        // Otherwise, append candidate to connection.[[EarlyCandidates]].
        self.slots.borrow_mut().early_candidates.push(candidate);
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-surface-a-candidate>
    fn surface_a_candidate(
        &self,
        candidate: RTCIceCandidateInit,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // Step 2: Let transport be the RTCIceTransport for which candidate is
        //         being made available.
        // Step 3: If connection.[[PendingLocalDescription]] is not null, and
        //         represents the ICE generation for which candidate was
        //         gathered, add candidate to
        //         connection.[[PendingLocalDescription]].sdp.
        // Step 4: If connection.[[CurrentLocalDescription]] is not null, and
        //         represents the ICE generation for which candidate was
        //         gathered, add candidate to
        //         connection.[[CurrentLocalDescription]].sdp.
        // Note: The description objects keep the SDP as set; gathered
        // candidates are not written into it.
        // Step 5: Let newCandidate be the result of creating an RTCIceCandidate
        //         with a new dictionary whose sdpMid and sdpMLineIndex are set
        //         to the values associated with this RTCIceTransport,
        //         usernameFragment is set to the username fragment of the
        //         candidate, and candidate is set to a string encoded using
        //         the candidate-attribute grammar to represent candidate.
        let candidate = self.with_transport_ufrag(candidate);
        let new_candidate = crate::webidl::bindings::create_interface_instance::<
            Types,
            RTCIceCandidate,
        >(RTCIceCandidate::create(candidate), ec)?;
        // Step 6: Add newCandidate to transport's set of local candidates.
        // Note: RTCIceTransport is not implemented.
        // Step 7: Fire an event named icecandidate using the
        //         RTCPeerConnectionIceEvent interface with the candidate
        //         attribute set to newCandidate at connection.
        self.fire_icecandidate(Some(new_candidate), time_millis, ec)
    }

    /// The username fragment of the local description's media description
    /// the candidate belongs to.
    fn with_transport_ufrag(&self, mut candidate: RTCIceCandidateInit) -> RTCIceCandidateInit {
        if candidate.username_fragment.is_none()
            && let Some(local) = self.local_description_init()
        {
            let sections = sdp::media_descriptions(&local.sdp);
            let section = match (&candidate.sdp_mid, candidate.sdp_m_line_index) {
                (Some(mid), _) => sections
                    .iter()
                    .find(|section| section.mid.as_deref() == Some(mid.as_str())),
                (None, Some(index)) => sections.get(usize::from(index)),
                (None, None) => sections.first(),
            };
            candidate.username_fragment = section.and_then(|section| section.ice_ufrag.clone());
        }
        candidate
    }

    fn fire_icecandidate(
        &self,
        candidate: Option<JsObject>,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let event = RTCPeerConnectionIceEvent::new(
            String::from("icecandidate"),
            RTCPeerConnectionIceEventInit {
                bubbles: false,
                cancelable: false,
                composed: false,
                candidate,
                url: None,
            },
            ec,
        );
        fire_event_using(&self.event_target, event, time_millis, ec).map(|_| ())
    }

    /// <https://w3c.github.io/webrtc-pc/#rtcicetransport-gathering>: "When
    /// the ICE Agent is finished gathering a generation of candidates".
    fn end_of_candidates(
        &self,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // If connection.[[PendingLocalDescription]] is not null, and
        // represents the ICE generation for which gathering finished, add
        // a=end-of-candidates to connection.[[PendingLocalDescription]].sdp.
        // If connection.[[CurrentLocalDescription]] is not null, and
        // represents the ICE generation for which gathering finished, add
        // a=end-of-candidates to connection.[[CurrentLocalDescription]].sdp.
        // Note: As for surfaced candidates, the SDP is kept as set.
        // Let endOfGatheringCandidate be the result of creating an
        // RTCIceCandidate with a new dictionary whose sdpMid and sdpMLineIndex
        // are set to the values associated with this RTCIceTransport,
        // usernameFragment is set to the username fragment of the generation
        // of candidates for which gathering finished, and candidate is set to
        // "".
        let first_mid = self
            .local_description_init()
            .and_then(|local| sdp::media_descriptions(&local.sdp).into_iter().next())
            .and_then(|section| section.mid);
        let candidate = self.with_transport_ufrag(RTCIceCandidateInit {
            candidate: String::new(),
            sdp_mid: first_mid,
            sdp_m_line_index: Some(0),
            username_fragment: None,
            relay_protocol: None,
            url: None,
        });
        let end_of_gathering_candidate = crate::webidl::bindings::create_interface_instance::<
            Types,
            RTCIceCandidate,
        >(RTCIceCandidate::create(candidate), ec)?;
        // Fire an event named icecandidate using the RTCPeerConnectionIceEvent
        // interface with the candidate attribute set to
        // endOfGatheringCandidate at connection.
        self.fire_icecandidate(Some(end_of_gathering_candidate), time_millis, ec)
    }

    /// <https://w3c.github.io/webrtc-pc/#rtcicetransport-gathering>: the
    /// tasks that set [[IceGatheringState]].
    fn gathering_state_changed(
        &self,
        state: String,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // Set transport.[[IceGathererState]] to gathering (or complete).
        // Note: RTCIceTransport is not implemented.
        // Set connection.[[IceGatheringState]] to the value of deriving a new
        // state value as described by the RTCIceGatheringState enum.
        // Let connectionIceGatheringStateChanged be true if
        // connection.[[IceGatheringState]] changed in the previous step,
        // otherwise false.
        let complete = state == "complete";
        let changed = {
            let mut slots = self.slots.borrow_mut();
            let changed = slots.ice_gathering_state != state;
            slots.ice_gathering_state = state;
            changed
        };
        // Do not read or modify state beyond this point.
        // Fire an event named gatheringstatechange at transport.
        // If connectionIceGatheringStateChanged is true, fire an event named
        // icegatheringstatechange at connection.
        if changed {
            fire_event(ec, self, "icegatheringstatechange", time_millis, false)?;
        }
        // (The second task, once gathering completed:) Fire an event named
        // icecandidate using the RTCPeerConnectionIceEvent interface with the
        // candidate attribute set to null at connection.
        if complete {
            self.fire_icecandidate(None, time_millis, ec)?;
        }
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#announcing-a-data-channel-instance>
    fn announce_data_channel(
        &self,
        properties: ipc_messages::webrtc::DataChannelProperties,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let connection be the RTCPeerConnection object associated
        //         with the underlying data transport.
        // Step 2: If connection.[[IsClosed]] is true, abort these steps.
        if self.is_closed() {
            return Ok(());
        }
        // Step 3: Create an RTCDataChannel, channel.
        let document_origin = self.slots.borrow().document_origin.clone();
        let channel = RTCDataChannel::create(self.id, properties.handle, document_origin, ec)?;
        // Step 4: Let configuration be an information bundle received from the
        //         other peer as a part of the process to establish the
        //         underlying data transport described by the WebRTC
        //         DataChannel Protocol specification [[RFC8832]].
        // Step 5: Initialize channel.[[DataChannelLabel]], [[Ordered]],
        //         [[MaxPacketLifeTime]], [[MaxRetransmits]],
        //         [[DataChannelProtocol]], and [[DataChannelId]] internal
        //         slots to the corresponding values in configuration.
        // Step 6: Initialize channel.[[Negotiated]] to false.
        channel.initialize_from(&properties);
        // Step 7: Append channel to connection.[[DataChannels]].
        self.data_channels.borrow_mut(ec).push(channel.clone());
        self.channels.borrow_mut(ec).push(channel.clone());
        // Step 8: Set channel.[[ReadyState]] to "open" (but do not fire the
        //         open event, yet).
        channel.slots.borrow_mut().ready_state = RTCDataChannelState::Open;
        // Step 9: Fire an event named datachannel using the
        //         RTCDataChannelEvent interface with the channel attribute set
        //         to channel at connection.
        let channel_object = channel
            .object()
            .ok_or_else(|| ec.new_type_error("RTCDataChannel without its object"))?;
        let event = RTCDataChannelEvent::new(
            String::from("datachannel"),
            false,
            false,
            false,
            channel_object,
            ec,
        );
        fire_event_using(&self.event_target, event, time_millis, ec)?;
        // Step 10: Announce the data channel as open.
        // Note: The WebRTC process's own open notification for the channel
        // follows; `announce_open` announces a channel once.
        channel.announce_open(self.is_closed(), properties.id, time_millis, ec)
    }
}

/// <https://w3c.github.io/webrtc-pc/#dfn-validate-an-ice-server-url>
fn validate_an_ice_server_url(
    url: &str,
    server: &RTCIceServer,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 1: Let parsedURL be the result of parsing url.
    // Step 2: If any of the following conditions apply, then throw a
    //         "SyntaxError" DOMException:
    //         parsedURL is failure; parsedURL's scheme is neither "stun",
    //         "stuns", "turn", nor "turns"; parsedURL does not have an opaque
    //         path; parsedURL's opaque path contains one or more "/" or "@".
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(crate::webidl::syntax_error_value(ec));
    };
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "stun" | "stuns" | "turn" | "turns") {
        return Err(crate::webidl::syntax_error_value(ec));
    }
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    if path.is_empty() || path.starts_with('/') || path.contains('/') || path.contains('@') {
        return Err(crate::webidl::syntax_error_value(ec));
    }
    // Step 3: If parsedURL's scheme is not implemented by the user agent,
    //         then throw a NotSupportedError.
    // Note: All four schemes are implemented.
    // Step 4: Let hostAndPortURL be result of parsing the concatenation of
    //         "https://" and parsedURL's path.
    // Step 5: If hostAndPortURL is failure, then throw a "SyntaxError"
    //         DOMException. If hostAndPortURL's path, username, or password
    //         is non-null, then throw a "SyntaxError" DOMException.
    let host_and_port = url::Url::parse(&format!("https://{path}"))
        .map_err(|_| crate::webidl::syntax_error_value(ec))?;
    if !host_and_port.username().is_empty()
        || host_and_port.password().is_some()
        || host_and_port.path() != "/"
    {
        return Err(crate::webidl::syntax_error_value(ec));
    }
    // Step 6: If parsedURL's query is non-null and if parsedURL's query is
    //         different from either "transport=udp" or "transport=tcp", throw
    //         a "SyntaxError" DOMException.
    if let Some(query) = query
        && query != "transport=udp"
        && query != "transport=tcp"
    {
        return Err(crate::webidl::syntax_error_value(ec));
    }
    // Step 7: If parsedURL's' scheme is "turn" or "turns", and either of
    //         server.username or server.credential are missing or their UTF-8
    //         representations fail to conform to [[RFC8489]] section 14.3 and
    //         [[RFC8265]] section 4.1 respectively, then throw an
    //         InvalidAccessError.
    if matches!(scheme.as_str(), "turn" | "turns")
        && (server.username.is_none() || server.credential.is_none())
    {
        return Err(crate::webidl::invalid_access_error_value(
            String::from("TURN servers need a username and a credential"),
            ec,
        ));
    }
    Ok(())
}
