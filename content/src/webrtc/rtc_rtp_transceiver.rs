use std::cell::RefCell;
use std::rc::Rc;

use ipc_messages::webrtc::{
    PeerConnectionId, TrackKind, TransceiverDirection, TransceiverId, TransceiverSpec,
};
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::js::Types;
use crate::mediacapture_streams::{MediaStreamTrack, TrackSource};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{invalid_state_error_value, rejected_promise, resolved_promise};

type JsObject = <Types as JsTypes>::JsObject;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverdirection>
pub(crate) fn direction_as_idl(direction: TransceiverDirection) -> &'static str {
    match direction {
        TransceiverDirection::Sendrecv => "sendrecv",
        TransceiverDirection::Sendonly => "sendonly",
        TransceiverDirection::Recvonly => "recvonly",
        TransceiverDirection::Inactive => "inactive",
        TransceiverDirection::Stopped => "stopped",
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverdirection>
pub(crate) fn direction_from_idl(direction: &str) -> Option<TransceiverDirection> {
    match direction {
        "sendrecv" => Some(TransceiverDirection::Sendrecv),
        "sendonly" => Some(TransceiverDirection::Sendonly),
        "recvonly" => Some(TransceiverDirection::Recvonly),
        "inactive" => Some(TransceiverDirection::Inactive),
        "stopped" => Some(TransceiverDirection::Stopped),
        _ => None,
    }
}

/// Whether a direction sends media.
pub(crate) fn direction_sends(direction: TransceiverDirection) -> bool {
    matches!(
        direction,
        TransceiverDirection::Sendrecv | TransceiverDirection::Sendonly
    )
}

/// Whether a direction receives media.
pub(crate) fn direction_receives(direction: TransceiverDirection) -> bool {
    matches!(
        direction,
        TransceiverDirection::Sendrecv | TransceiverDirection::Recvonly
    )
}

/// The internal slots of an RTCRtpSender, shared by every clone of it.
#[derive(Debug)]
pub(crate) struct SenderSlots {
    /// The kind of track the sender sends.
    pub(crate) kind: TrackKind,
    /// The id of the track the sender was created with (the msid track id
    /// the offer carries).
    pub(crate) track_id_at_creation: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-associated-medias-streams>
    pub(crate) associated_media_stream_ids: Vec<String>,
    /// The transceiver the sender belongs to.
    pub(crate) transceiver: TransceiverId,
    /// The connection the sender belongs to.
    pub(crate) peer: PeerConnectionId,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpsender>
#[gc_struct]
pub(crate) struct RTCRtpSender {
    /// <https://w3c.github.io/webrtc-pc/#dfn-sendertrack>
    track: GcCell<Option<MediaStreamTrack>>,

    #[ignore_trace]
    pub(crate) slots: Rc<RefCell<SenderSlots>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub(crate) reflector: Option<JsObject>,
}

impl RTCRtpSender {
    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcrtpsender>
    pub(crate) fn create(
        track: Option<MediaStreamTrack>,
        kind: TrackKind,
        streams: &[String],
        transceiver: TransceiverId,
        peer: PeerConnectionId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let sender be a new RTCRtpSender object.
        // Step 2: Let sender have a [[SenderTrack]] internal slot initialized
        //         to track.
        // Step 3: Let sender have a [[SenderTransport]] internal slot
        //         initialized to null.
        // Step 4: Let sender have a [[LastReturnedParameters]] internal slot
        //         initialized to null.
        // Step 5: Let sender have an [[AssociatedMediaStreamIds]] internal
        //         slot, representing a list of Ids of MediaStream objects
        //         that the MediaStreamTrack object of this sender is
        //         associated with.
        // Step 6: Set sender.[[AssociatedMediaStreamIds]] to an empty set.
        // Step 7: For each stream in streams, add stream.id to
        //         [[AssociatedMediaStreamIds]] if it's not already there.
        // Step 8: Let sender have a [[SendEncodings]] internal slot,
        //         representing a list of RTCRtpEncodingParameters
        //         dictionaries.
        // Step 9: If sendEncodings is given as input to this algorithm, and is
        //         non-empty, set the [[SendEncodings]] slot to sendEncodings.
        // Step 10: Otherwise, set it to a list containing a single
        //          RTCRtpEncodingParameters with active set to true.
        // Step 11: Let sender have a [[LastReturnedParameters]] internal slot,
        //          initialized to null.
        // Note: The transport, the encodings and the parameters are not
        // modeled; getParameters() returns a default set.
        let track_id_at_creation = track
            .as_ref()
            .map(MediaStreamTrack::id)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut stream_ids: Vec<String> = Vec::new();
        for stream in streams {
            if !stream_ids.contains(stream) {
                stream_ids.push(stream.clone());
            }
        }
        let sender = Self {
            track: gc_cell_new(track, ec),
            slots: Rc::new(RefCell::new(SenderSlots {
                kind,
                track_id_at_creation,
                associated_media_stream_ids: stream_ids,
                transceiver,
                peer,
            })),
            reflector: None,
        };

        // Step 12: Return sender.
        let object = create_interface_instance::<Types, RTCRtpSender>(sender, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<RTCRtpSender>().cloned())
            .ok_or_else(|| ec.new_type_error("RTCRtpSender instance is not an RTCRtpSender"))
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpsender-track>
    pub(crate) fn track(&self, ec: &mut dyn ExecutionContext<Types>) -> Option<MediaStreamTrack> {
        // The track attribute's getter returns the value of the
        // [[SenderTrack]] slot.
        self.track.borrow(ec).clone()
    }

    pub(crate) fn set_track(
        &self,
        track: Option<MediaStreamTrack>,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        *self.track.borrow_mut(ec) = track;
    }

    pub(crate) fn kind(&self) -> TrackKind {
        self.slots.borrow().kind
    }

    pub(crate) fn transceiver_id(&self) -> TransceiverId {
        self.slots.borrow().transceiver
    }

    pub(crate) fn track_id_at_creation(&self) -> String {
        self.slots.borrow().track_id_at_creation.clone()
    }

    pub(crate) fn associated_media_stream_ids(&self) -> Vec<String> {
        self.slots.borrow().associated_media_stream_ids.clone()
    }

    pub(crate) fn set_streams(&self, streams: Vec<String>) {
        self.slots.borrow_mut().associated_media_stream_ids = streams;
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpreceiver>
#[gc_struct]
pub(crate) struct RTCRtpReceiver {
    /// <https://w3c.github.io/webrtc-pc/#dfn-receivertrack>
    track: MediaStreamTrack,

    /// <https://w3c.github.io/webrtc-pc/#dfn-associated-remote-media-streams>
    #[ignore_trace]
    associated_remote_media_streams: Rc<RefCell<Vec<String>>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub(crate) reflector: Option<JsObject>,
}

impl RTCRtpReceiver {
    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcrtpreceiver>
    pub(crate) fn create(
        kind: TrackKind,
        peer: PeerConnectionId,
        transceiver: TransceiverId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let receiver be a new RTCRtpReceiver object.
        // Step 2: Let track be a new MediaStreamTrack object [[!GETUSERMEDIA]].
        //         The source of track is a remote source provided by receiver.
        //         Note that the track.id is generated by the user agent and
        //         does not map to any track IDs on the remote side.
        // Step 3: Initialize track.kind to kind.
        // Step 4: Initialize track.label to the result of concatenating the
        //         string "remote " with kind.
        // Step 5: Initialize track.readyState to live.
        // Step 6: Initialize track.muted to true. See the MediaStreamTrack
        //         section about how the muted attribute reflects if media is
        //         flowing.
        let label = format!(
            "remote {}",
            match kind {
                TrackKind::Audio => "audio",
                TrackKind::Video => "video",
            }
        );
        let track = MediaStreamTrack::create(
            TrackSource::Remote { peer, transceiver },
            kind,
            label,
            true,
            ec,
        )?;

        // Step 7: Let receiver have a [[ReceiverTrack]] internal slot
        //         initialized to track.
        // Step 8: Let receiver have a [[ReceiverTransport]] internal slot
        //         initialized to null.
        // Step 9: Let receiver have a [[LastStableStateReceiverTransport]]
        //         internal slot initialized to null.
        // Step 10: Let receiver have an [[AssociatedRemoteMediaStreams]]
        //          internal slot, representing a list of MediaStream objects
        //          that the MediaStreamTrack object of this receiver is
        //          associated with, and initialized to an empty list.
        // Step 11: Let receiver have a
        //          [[LastStableStateAssociatedRemoteMediaStreams]] internal
        //          slot and initialize it to an empty list.
        // Step 12: Let receiver have a [[ReceiveCodecs]] internal slot,
        //          representing a list of RTCRtpCodecParameters dictionaries,
        //          and initialized to an empty list.
        // Step 13: Let receiver have a [[LastStableStateReceiveCodecs]]
        //          internal slot and initialize it to an empty list.
        // Note: The transport and codecs are not modeled.
        let receiver = Self {
            track,
            associated_remote_media_streams: Rc::new(RefCell::new(Vec::new())),
            reflector: None,
        };

        // Step 14: Return receiver.
        let object = create_interface_instance::<Types, RTCRtpReceiver>(receiver, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<RTCRtpReceiver>().cloned())
            .ok_or_else(|| ec.new_type_error("RTCRtpReceiver instance is not an RTCRtpReceiver"))
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpreceiver-track>
    pub(crate) fn track(&self) -> MediaStreamTrack {
        // The track attribute is the track that is associated with this
        // RTCRtpReceiver object receiver.
        self.track.clone()
    }

    pub(crate) fn associated_remote_media_streams(&self) -> Vec<String> {
        self.associated_remote_media_streams.borrow().clone()
    }

    pub(crate) fn set_associated_remote_media_streams(&self, streams: Vec<String>) {
        *self.associated_remote_media_streams.borrow_mut() = streams;
    }
}

/// The internal slots of an RTCRtpTransceiver, shared by every clone of it.
#[derive(Debug)]
pub(crate) struct TransceiverSlots {
    /// The transceiver's id in the WebRTC process.
    pub(crate) id: TransceiverId,
    pub(crate) kind: TrackKind,
    /// <https://w3c.github.io/webrtc-pc/#dfn-mid>
    pub(crate) mid: Option<String>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-direction>
    pub(crate) direction: TransceiverDirection,
    /// <https://w3c.github.io/webrtc-pc/#dfn-currentdirection>
    pub(crate) current_direction: Option<TransceiverDirection>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-fireddirection>
    pub(crate) fired_direction: Option<TransceiverDirection>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-receptive>
    pub(crate) receptive: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-stopping>
    pub(crate) stopping: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-stopped>
    pub(crate) stopped: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-jsepmid>
    pub(crate) jsep_mid: Option<String>,
    /// Created by addTrack(): eligible to reuse for a remote m= section.
    pub(crate) from_add_track: bool,
    /// [[CurrentDirection]] has been "sendrecv" or "sendonly" once (addTrack
    /// step 8: "the sender has never been used to send").
    pub(crate) sent_once: bool,
    /// The graphics process captures the default audio input for this
    /// transceiver's sender.
    pub(crate) capturing: bool,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver>
#[gc_struct]
pub(crate) struct RTCRtpTransceiver {
    /// <https://w3c.github.io/webrtc-pc/#dfn-sender>
    pub(crate) sender: RTCRtpSender,

    /// <https://w3c.github.io/webrtc-pc/#dfn-receiver>
    pub(crate) receiver: RTCRtpReceiver,

    #[ignore_trace]
    pub(crate) slots: Rc<RefCell<TransceiverSlots>>,

    /// <https://webidl.spec.whatwg.org/#dfn-platform-object>
    pub(crate) reflector: Option<JsObject>,
}

impl RTCRtpTransceiver {
    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcrtptransceiver>
    pub(crate) fn create(
        id: TransceiverId,
        kind: TrackKind,
        sender: RTCRtpSender,
        receiver: RTCRtpReceiver,
        direction: TransceiverDirection,
        from_add_track: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let transceiver be a new RTCRtpTransceiver object.
        // Step 2: Let transceiver have a [[Sender]] internal slot, initialized
        //         to sender.
        // Step 3: Let transceiver have a [[Receiver]] internal slot,
        //         initialized to receiver.
        // Step 4: Let transceiver have a [[Stopping]] internal slot,
        //         initialized to false.
        // Step 5: Let transceiver have a [[Stopped]] internal slot,
        //         initialized to false.
        // Step 6: Let transceiver have a [[Direction]] internal slot,
        //         initialized to direction.
        // Step 7: Let transceiver have a [[Receptive]] internal slot,
        //         initialized to false.
        // Step 8: Let transceiver have a [[CurrentDirection]] internal slot,
        //         initialized to null.
        // Step 9: Let transceiver have a [[FiredDirection]] internal slot,
        //         initialized to null.
        // Step 10: Let transceiver have a [[PreferredCodecs]] internal slot,
        //          initialized to an empty list.
        // Step 11: Let transceiver have a [[JsepMid]] internal slot,
        //          initialized to null. This is the "RtpTransceiver mid
        //          property" defined in [[RFC9429]], and is only modified
        //          there.
        // Step 12: Let transceiver have a [[Mid]] internal slot, initialized
        //          to null. On setting, [[JsepMid]] is copied to [[Mid]].
        // Note: Codec preferences are not modeled.
        let transceiver = Self {
            sender,
            receiver,
            slots: Rc::new(RefCell::new(TransceiverSlots {
                id,
                kind,
                mid: None,
                direction,
                current_direction: None,
                fired_direction: None,
                receptive: false,
                stopping: false,
                stopped: false,
                jsep_mid: None,
                from_add_track,
                sent_once: false,
                capturing: false,
            })),
            reflector: None,
        };

        // Step 13: Return transceiver.
        let object = create_interface_instance::<Types, RTCRtpTransceiver>(transceiver, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<RTCRtpTransceiver>().cloned())
            .ok_or_else(|| {
                ec.new_type_error("RTCRtpTransceiver instance is not an RTCRtpTransceiver")
            })
    }

    pub(crate) fn id(&self) -> TransceiverId {
        self.slots.borrow().id
    }

    pub(crate) fn kind(&self) -> TrackKind {
        self.slots.borrow().kind
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-mid>
    pub(crate) fn mid(&self) -> Option<String> {
        // The mid attribute's getter returns the value of the [[Mid]] slot.
        self.slots.borrow().mid.clone()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-direction>
    pub(crate) fn direction(&self) -> TransceiverDirection {
        // Step 1: Let transceiver be the RTCRtpTransceiver object on which the
        //         getter is invoked.
        // Step 2: If transceiver.[[Stopping]] is true, return "stopped".
        let slots = self.slots.borrow();
        if slots.stopping {
            return TransceiverDirection::Stopped;
        }

        // Step 3: Otherwise, return the value of the [[Direction]] slot.
        slots.direction
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-currentdirection>
    pub(crate) fn current_direction(&self) -> Option<TransceiverDirection> {
        // Step 1: Let transceiver be the RTCRtpTransceiver object on which the
        //         getter is invoked.
        // Step 2: If transceiver.[[Stopped]] is true, return "stopped".
        let slots = self.slots.borrow();
        if slots.stopped {
            return Some(TransceiverDirection::Stopped);
        }

        // Step 3: Otherwise, return the value of the [[CurrentDirection]]
        //         slot.
        slots.current_direction
    }

    pub(crate) fn stopped(&self) -> bool {
        self.slots.borrow().stopped
    }

    pub(crate) fn stopping(&self) -> bool {
        self.slots.borrow().stopping
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-stop-the-rtcrtptransceiver>
    pub(crate) fn stop_the_rtcrtptransceiver(
        &self,
        disappear: bool,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let sender be transceiver.[[Sender]].
        // Step 2: Let receiver be transceiver.[[Receiver]].
        // Step 3: Stop sending media with sender.
        // Step 4: Send an RTCP BYE for each RTP stream that was being sent by
        //         sender, as specified in [[RFC3550]].
        // Step 5: Stop receiving media with receiver.
        // Step 6: If disappear is false, execute the sub steps that
        //         correspond to the value of receiver.[[ReceiverTrack]].kind:
        //         "audio": end the track with a reason other than stop, and
        //         no more audio is played; "video": end the track and no more
        //         video is rendered.
        // Note: The WebRTC process stops the media once the stopped
        // transceiver is negotiated (`TransceiverSpec::stopped`).
        if !disappear {
            self.receiver
                .track()
                .ended_for_a_reason_other_than_stop(time_millis, ec)?;
        }

        // Step 7: Set transceiver.[[Receptive]] to false.
        // Step 8: Set transceiver.[[Stopped]] to true.
        // Step 9: Set transceiver.[[Stopping]] to true.
        let mut slots = self.slots.borrow_mut();
        slots.receptive = false;
        slots.stopped = true;
        slots.stopping = true;
        Ok(())
    }

    /// The spec of this transceiver for the WebRTC process.
    pub(crate) fn spec(&self) -> TransceiverSpec {
        let slots = self.slots.borrow();
        TransceiverSpec {
            id: slots.id,
            kind: slots.kind,
            direction: if slots.stopping {
                TransceiverDirection::Stopped
            } else {
                slots.direction
            },
            stream_ids: self.sender.associated_media_stream_ids(),
            sender_track_id: self.sender.track_id_at_creation(),
            from_add_track: slots.from_add_track,
            stopped: slots.stopping,
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtpsender-replacetrack>
pub(crate) fn replace_track(
    sender: &RTCRtpSender,
    transceiver: Option<&RTCRtpTransceiver>,
    with_track: Option<MediaStreamTrack>,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<JsObject, Types> {
    // Step 1: Let sender be the RTCRtpSender object on which replaceTrack()
    //         is invoked.
    // Step 2: Let transceiver be the RTCRtpTransceiver object associated
    //         with sender.
    // Step 3: Let connection be the RTCPeerConnection object associated with
    //         sender.
    // Step 4: Let withTrack be the argument to this method.
    // Step 5: If withTrack is non-null and withTrack.kind differs from the
    //         transceiver kind of transceiver, return a promise rejected with
    //         a newly created TypeError.
    if let Some(with_track) = &with_track
        && with_track.kind() != sender.kind()
    {
        let error = ec.new_type_error("the track's kind differs from the sender's kind");
        return rejected_promise(error, ec);
    }

    // Step 6: Return the result of chaining the following steps to
    //         connection's operations chain:
    // Step 6.1: If transceiver.[[Stopping]] is true, return a promise
    //           rejected with a newly created InvalidStateError.
    if transceiver.is_some_and(RTCRtpTransceiver::stopping) {
        let error = invalid_state_error_value(ec);
        return rejected_promise(error, ec);
    }

    // Step 6.2: Let p be a new promise.
    // Step 6.3: Let sending be true if transceiver.[[CurrentDirection]] is
    //           "sendrecv" or "sendonly", and false otherwise.
    // Step 6.4: Run the following steps in parallel:
    // Step 6.4.1: If sending is true, and withTrack is null, have the sender
    //             stop sending.
    // Step 6.4.2: If sending is true, and withTrack is non-null, determine
    //             if withTrack can be sent immediately by the sender without
    //             violating the sender's already-negotiated envelope, and if
    //             it cannot, then reject p with a newly created
    //             InvalidModificationError, and abort these steps.
    // Step 6.4.3: If sending is true, and withTrack is non-null, have the
    //             sender switch seamlessly to transmitting withTrack instead
    //             of the sender's existing track.
    // Step 6.4.4: Queue a task that runs the following steps:
    // Step 6.4.4.1: If connection.[[IsClosed]] is true, abort these steps.
    // Step 6.4.4.2: Set sender.[[SenderTrack]] to withTrack.
    // Step 6.4.4.3: Resolve p with undefined.
    // Note: The steps run without chaining or a task; the WebRTC process
    // sends whatever the track's capture source produces.
    sender.set_track(with_track, ec);
    let undefined = ec.value_undefined();

    // Step 6.5: Return p.
    resolved_promise(undefined, ec)
}
