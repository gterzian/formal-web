//! The media part of RTCPeerConnection: transceivers, tracks and stats.

use ipc_messages::webrtc::{
    OperationId, Request, TrackKind, TransceiverDirection, TransceiverId, TransceiverState,
};
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::fire_event_using;
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::mediacapture_streams::{
    MediaStream, MediaStreamTrack, MediaStreamTrackState, TrackSource,
};
use crate::webidl::bindings::create_interface_instance;
use crate::webidl::{invalid_access_error_value, invalid_state_error_value};
use ipc_messages::graphics::GraphicsCommand;

use super::events::RTCTrackEvent;
use super::rtc_peer_connection::{RTCPeerConnection, StatsRequest, send_request};
use super::rtc_rtp_transceiver::{
    RTCRtpReceiver, RTCRtpSender, RTCRtpTransceiver, direction_receives, direction_sends,
};
use super::rtc_stats_report::RTCStatsReport;

type JsObject = <Types as JsTypes>::JsObject;

/// The first argument of addTransceiver() after overload resolution.
/// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-addtransceiver>
pub(crate) enum TrackOrKind {
    Track(MediaStreamTrack),
    Kind(TrackKind),
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverinit>
pub(crate) struct RTCRtpTransceiverInit {
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverinit-direction>
    pub(crate) direction: TransceiverDirection,
    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiverinit-streams>
    pub(crate) streams: Vec<MediaStream>,
}

/// The same m= section seen from the other peer.
fn invert(direction: TransceiverDirection) -> TransceiverDirection {
    match direction {
        TransceiverDirection::Sendonly => TransceiverDirection::Recvonly,
        TransceiverDirection::Recvonly => TransceiverDirection::Sendonly,
        other => other,
    }
}

impl RTCPeerConnection {
    fn next_transceiver_id(&self) -> TransceiverId {
        let mut slots = self.slots.borrow_mut();
        slots.next_transceiver += 1;
        TransceiverId(slots.next_transceiver)
    }

    fn transceiver_by_id(
        &self,
        id: TransceiverId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<RTCRtpTransceiver> {
        self.transceivers
            .borrow(ec)
            .iter()
            .find(|transceiver| transceiver.id() == id)
            .cloned()
    }

    /// Hand a transceiver's current state to the WebRTC process, so the next
    /// offer or answer negotiates it.
    fn upsert_transceiver(
        &self,
        transceiver: &RTCRtpTransceiver,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        send_request(
            Request::UpsertTransceiver {
                peer: self.id,
                spec: transceiver.spec(),
            },
            ec,
        );
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-addtrack>
    pub(crate) fn add_track(
        &self,
        track: MediaStreamTrack,
        streams: Vec<MediaStream>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<RTCRtpSender, Types> {
        // Step 1: Let connection be the RTCPeerConnection object on which this
        //         method was invoked.
        // Step 2: Let track be the MediaStreamTrack object indicated by the
        //         method's first argument.
        // Step 3: Let kind be track.kind.
        let kind = track.kind();
        // Step 4: Let streams be a list of MediaStream objects constructed
        //         from the method's remaining arguments, or an empty list if
        //         the method was called with a single argument.
        let stream_ids: Vec<String> = streams.iter().map(MediaStream::id).collect();
        // Step 5: If connection.[[IsClosed]] is true, throw an
        //         InvalidStateError.
        if self.is_closed() {
            return Err(invalid_state_error_value(ec));
        }
        // Step 6: Let senders be the result of executing the CollectSenders
        //         algorithm.
        // Step 7: If an RTCRtpSender for track already exists in senders,
        //         throw an InvalidAccessError.
        let track_id = track.id();
        let transceivers = self.transceivers.borrow(ec).clone();
        for transceiver in &transceivers {
            if transceiver
                .sender
                .track(ec)
                .is_some_and(|existing| existing.id() == track_id)
            {
                return Err(invalid_access_error_value(
                    String::from("the track is already sent by this connection"),
                    ec,
                ));
            }
        }
        // Step 8: The steps below describe how to determine if an existing
        //         sender can be reused. Doing so will cause future calls to
        //         createOffer and createAnswer to mark the corresponding media
        //         description as sendrecv or sendonly and add the MSID of the
        //         sender's streams, as defined in [[RFC9429]]. If any
        //         RTCRtpSender object in senders matches all the following
        //         criteria, let sender be that object, or null otherwise:
        //         The sender's track is null. The transceiver kind of the
        //         RTCRtpTransceiver, associated with the sender, matches kind.
        //         The [[Stopping]] slot of the RTCRtpTransceiver associated
        //         with the sender is false. The sender has never been used to
        //         send. More precisely, the [[CurrentDirection]] slot of the
        //         RTCRtpTransceiver associated with the sender has never had a
        //         value of "sendrecv" or "sendonly".
        let reusable = transceivers.iter().find(|transceiver| {
            transceiver.sender.track(ec).is_none()
                && transceiver.kind() == kind
                && !transceiver.stopping()
                && !transceiver.slots.borrow().sent_once
        });
        let transceiver = match reusable {
            // Step 9: If sender is not null, run the following steps to use
            //         that sender:
            Some(transceiver) => {
                // Step 9.1: Set sender.[[SenderTrack]] to track.
                transceiver.sender.set_track(Some(track), ec);
                // Step 9.2: Set sender.[[AssociatedMediaStreamIds]] to an
                //           empty set.
                // Step 9.3: For each stream in streams, add stream.id to
                //           [[AssociatedMediaStreamIds]] if it's not already
                //           there.
                let mut ids: Vec<String> = Vec::new();
                for id in stream_ids {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                transceiver.sender.set_streams(ids);
                // Step 9.4: Let transceiver be the RTCRtpTransceiver associated
                //           with sender.
                // Step 9.5: If transceiver.[[Direction]] is "recvonly", set
                //           transceiver.[[Direction]] to "sendrecv".
                // Step 9.6: If transceiver.[[Direction]] is "inactive", set
                //           transceiver.[[Direction]] to "sendonly".
                {
                    let mut slots = transceiver.slots.borrow_mut();
                    slots.direction = match slots.direction {
                        TransceiverDirection::Recvonly => TransceiverDirection::Sendrecv,
                        TransceiverDirection::Inactive => TransceiverDirection::Sendonly,
                        other => other,
                    };
                }
                transceiver.clone()
            }
            // Step 10: If sender is null, run the following steps:
            None => {
                let id = self.next_transceiver_id();
                // Step 10.1: Create an RTCRtpSender with track, kind and
                //            streams, and let sender be the result.
                let sender = RTCRtpSender::create(Some(track), kind, &stream_ids, id, self.id, ec)?;
                // Step 10.2: Create an RTCRtpReceiver with kind, and let
                //            receiver be the result.
                let receiver = RTCRtpReceiver::create(kind, self.id, id, ec)?;
                // Step 10.3: Create an RTCRtpTransceiver with sender, receiver
                //            and an RTCRtpTransceiverDirection value of
                //            "sendrecv", and let transceiver be the result.
                let transceiver = RTCRtpTransceiver::create(
                    id,
                    kind,
                    sender,
                    receiver,
                    TransceiverDirection::Sendrecv,
                    true,
                    ec,
                )?;
                // Step 10.4: Add transceiver to connection's set of
                //            transceivers.
                self.transceivers.borrow_mut(ec).push(transceiver.clone());
                transceiver
            }
        };
        // Step 11: A track could have contents that are inaccessible to the
        //          application. ...
        self.upsert_transceiver(&transceiver, ec);
        // Step 12: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        // Step 13: Return sender.
        Ok(transceiver.sender.clone())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-addtransceiver>
    pub(crate) fn add_transceiver(
        &self,
        track_or_kind: TrackOrKind,
        init: RTCRtpTransceiverInit,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<RTCRtpTransceiver, Types> {
        // Step 1: Let init be the second argument.
        // Step 2: Let streams be init.streams.
        let stream_ids: Vec<String> = init.streams.iter().map(MediaStream::id).collect();
        // Step 3: Let sendEncodings be init.sendEncodings.
        // Step 4: Let direction be init.direction.
        let direction = init.direction;
        // Step 5: If the first argument is a string, let it be kind and run
        //         the following steps:
        // Step 5.1: If kind is not a legal MediaStreamTrack kind, throw a
        //           TypeError.
        // Step 5.2: Let track be null.
        // Step 6: If the first argument is a MediaStreamTrack, let it be
        //         track and let kind be track.kind.
        // Note: The binding resolves the overload and validates the kind.
        let (track, kind) = match track_or_kind {
            TrackOrKind::Track(track) => {
                let kind = track.kind();
                (Some(track), kind)
            }
            TrackOrKind::Kind(kind) => (None, kind),
        };
        // Step 7: If connection.[[IsClosed]] is true, throw an
        //         InvalidStateError.
        if self.is_closed() {
            return Err(invalid_state_error_value(ec));
        }
        // Step 8: Validate sendEncodings by running the following steps: ...
        // Note: Encodings are not modeled.
        let id = self.next_transceiver_id();
        // Step 9: Create an RTCRtpSender with track, kind, streams and
        //         sendEncodings and let sender be the result.
        let sender = RTCRtpSender::create(track, kind, &stream_ids, id, self.id, ec)?;
        // Step 10: Create an RTCRtpReceiver with kind and let receiver be the
        //          result.
        let receiver = RTCRtpReceiver::create(kind, self.id, id, ec)?;
        // Step 11: Create an RTCRtpTransceiver with sender, receiver and
        //          direction, and let transceiver be the result.
        let transceiver =
            RTCRtpTransceiver::create(id, kind, sender, receiver, direction, false, ec)?;
        // Step 12: Add transceiver to connection's set of transceivers.
        self.transceivers.borrow_mut(ec).push(transceiver.clone());
        self.upsert_transceiver(&transceiver, ec);
        // Step 13: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        // Step 14: Return transceiver.
        Ok(transceiver)
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-removetrack>
    pub(crate) fn remove_track(
        &self,
        sender: RTCRtpSender,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let sender be the argument to removeTrack.
        // Step 2: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 3: If connection.[[IsClosed]] is true, throw an
        //         InvalidStateError.
        if self.is_closed() {
            return Err(invalid_state_error_value(ec));
        }
        // Step 4: If sender was not created by connection, throw an
        //         InvalidAccessError.
        let Some(transceiver) = self.transceiver_by_id(sender.transceiver_id(), ec) else {
            return Err(invalid_access_error_value(
                String::from("the sender was not created by this connection"),
                ec,
            ));
        };
        if sender.slots.borrow().peer != self.id {
            return Err(invalid_access_error_value(
                String::from("the sender was not created by this connection"),
                ec,
            ));
        }
        // Step 5: Let transceiver be the RTCRtpTransceiver object
        //         corresponding to sender.
        // Step 6: If transceiver.[[Stopping]] is true, abort these steps.
        if transceiver.stopping() {
            return Ok(());
        }
        // Step 7: If sender.[[SenderTrack]] is null, abort these steps.
        if transceiver.sender.track(ec).is_none() {
            return Ok(());
        }
        // Step 8: Set sender.[[SenderTrack]] to null.
        self.update_capture(&transceiver, false, ec);
        transceiver.sender.set_track(None, ec);
        // Step 9: If transceiver.[[Direction]] is "sendrecv", set
        //         transceiver.[[Direction]] to "recvonly".
        // Step 10: If transceiver.[[Direction]] is "sendonly", set
        //          transceiver.[[Direction]] to "inactive".
        {
            let mut slots = transceiver.slots.borrow_mut();
            slots.direction = match slots.direction {
                TransceiverDirection::Sendrecv => TransceiverDirection::Recvonly,
                TransceiverDirection::Sendonly => TransceiverDirection::Inactive,
                other => other,
            };
        }
        self.upsert_transceiver(&transceiver, ec);
        // Step 11: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-direction>
    pub(crate) fn set_transceiver_direction(
        &self,
        transceiver: &RTCRtpTransceiver,
        new_direction: TransceiverDirection,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let transceiver be the RTCRtpTransceiver object on which
        //         the setter is invoked.
        // Step 2: Let connection be the RTCPeerConnection object associated
        //         with transceiver.
        // Step 3: If transceiver.[[Stopping]] is true, throw an
        //         InvalidStateError.
        if transceiver.stopping() {
            return Err(invalid_state_error_value(ec));
        }
        // Step 4: Let newDirection be the argument to the setter.
        // Step 5: If newDirection is equal to transceiver.[[Direction]],
        //         abort these steps.
        if transceiver.slots.borrow().direction == new_direction {
            return Ok(());
        }
        // Step 6: If newDirection is equal to "stopped", throw a TypeError.
        if new_direction == TransceiverDirection::Stopped {
            return Err(ec.new_type_error("direction cannot be set to \"stopped\""));
        }
        // Step 7: Set transceiver.[[Direction]] to newDirection.
        transceiver.slots.borrow_mut().direction = new_direction;
        self.upsert_transceiver(transceiver, ec);
        // Step 8: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcrtptransceiver-stop>
    pub(crate) fn stop_transceiver(
        &self,
        transceiver: &RTCRtpTransceiver,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let transceiver be the RTCRtpTransceiver object on which
        //         the method is invoked.
        // Step 2: Let connection be the RTCPeerConnection object associated
        //         with transceiver.
        // Step 3: If connection.[[IsClosed]] is true, throw an
        //         InvalidStateError.
        if self.is_closed() {
            return Err(invalid_state_error_value(ec));
        }
        // Step 4: If transceiver.[[Stopping]] is true, abort these steps.
        if transceiver.stopping() {
            return Ok(());
        }
        // Step 5: Stop sending and receiving with transceiver.
        // Stop sending and receiving: let sender be transceiver.[[Sender]];
        // let receiver be transceiver.[[Receiver]]; stop sending media with
        // sender; send an RTCP BYE for each RTP stream that was being sent by
        // sender; stop receiving media with receiver; execute the sub steps
        // that correspond to the value of receiver.[[ReceiverTrack]].kind
        // (end the track); set transceiver.[[Direction]] to "inactive";
        // set transceiver.[[Receptive]] to false; set transceiver.[[Stopping]]
        // to true.
        transceiver
            .receiver
            .track()
            .ended_for_a_reason_other_than_stop(time_millis, ec)?;
        self.update_capture(transceiver, false, ec);
        self.stop_playout(transceiver, ec);
        {
            let mut slots = transceiver.slots.borrow_mut();
            slots.direction = TransceiverDirection::Inactive;
            slots.receptive = false;
            slots.stopping = true;
        }
        self.upsert_transceiver(transceiver, ec);
        // Step 6: Update the negotiation-needed flag for connection.
        self.update_the_negotiation_needed_flag(ec);
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-getsenders>
    pub(crate) fn get_senders(&self, ec: &mut dyn ExecutionContext<Types>) -> Vec<RTCRtpSender> {
        // Returns a sequence of RTCRtpSender objects representing the RTP
        // senders that belong to non-stopped RTCRtpTransceiver objects
        // currently attached to this RTCPeerConnection object. The
        // getSenders method MUST return the result of executing the
        // CollectSenders algorithm.
        self.get_transceivers(ec)
            .into_iter()
            .map(|transceiver| transceiver.sender.clone())
            .collect()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-getreceivers>
    pub(crate) fn get_receivers(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<RTCRtpReceiver> {
        // Returns a sequence of RTCRtpReceiver objects representing the RTP
        // receivers that belong to non-stopped RTCRtpTransceiver objects
        // currently attached to this RTCPeerConnection object.
        self.get_transceivers(ec)
            .into_iter()
            .map(|transceiver| transceiver.receiver.clone())
            .collect()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-gettransceivers>
    pub(crate) fn get_transceivers(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Vec<RTCRtpTransceiver> {
        // Returns a sequence of RTCRtpTransceiver objects representing the
        // RTP transceivers that are currently attached to this
        // RTCPeerConnection object. The getTransceivers method MUST return
        // the result of executing the CollectTransceivers algorithm.
        // CollectTransceivers: Let transceivers be a new sequence; for each
        // transceiver in connection's set of transceivers whose [[Stopped]]
        // is false, add transceiver to transceivers; return transceivers.
        self.transceivers
            .borrow(ec)
            .iter()
            .filter(|transceiver| !transceiver.stopped())
            .cloned()
            .collect()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcpeerconnection-getstats>
    pub(crate) fn get_stats(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<JsObject, Types> {
        // Step 1: Let selectorArg be the method's first argument.
        // Step 2: Let connection be the RTCPeerConnection object on which the
        //         method was invoked.
        // Step 3: If selectorArg is null, let selector be null.
        // Step 4: If selectorArg is a MediaStreamTrack let selector be an
        //         RTCRtpSender or RTCRtpReceiver on connection which track
        //         member matches selectorArg. If no such sender or receiver
        //         exists, or if more than one sender or receiver fit this
        //         criteria, return a promise rejected with a newly created
        //         InvalidAccessError.
        // Note: Only the null selector is implemented: the report covers the
        // connection.
        // Step 5: Let p be a new promise.
        let (promise, resolvers) = ec.new_promise_pending()?;
        let promise = Types::value_as_object(&promise)
            .ok_or_else(|| ec.new_type_error("the new promise is not an object"))?;
        // Step 6: Run the following steps in parallel:
        // Step 6.1: Gather the stats indicated by selector according to the
        //           stats selection algorithm.
        // Step 6.2: Resolve p with the resulting RTCStatsReport object,
        //           containing the gathered stats.
        // Note: The WebRTC process gathers the stats; its report resolves
        // the promise in `stats_completed`.
        let operation = self.next_operation_id_for_stats();
        self.stats_requests.borrow_mut(ec).push(StatsRequest {
            operation,
            resolvers,
        });
        send_request(
            Request::GetStats {
                peer: self.id,
                operation,
            },
            ec,
        );
        // Step 7: Return p.
        Ok(promise)
    }

    fn next_operation_id_for_stats(&self) -> OperationId {
        let mut slots = self.slots.borrow_mut();
        slots.next_operation += 1;
        OperationId(slots.next_operation)
    }

    /// The stats selection algorithm's result arrived from the WebRTC
    /// process: resolve getStats()'s promise (step 6.2).
    pub(super) fn stats_completed(
        &self,
        operation: OperationId,
        report: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let request = {
            let mut requests = self.stats_requests.borrow_mut(ec);
            let index = requests
                .iter()
                .position(|request| request.operation == operation);
            index.map(|index| requests.remove(index))
        };
        let Some(request) = request else {
            return Ok(());
        };
        let report = RTCStatsReport::from_json(report);
        let object = create_interface_instance::<Types, RTCStatsReport>(report, ec)?;
        request
            .resolvers
            .resolve(Types::value_from_object(object), ec)?;
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-check-if-negotiation-is-needed>,
    /// step 5: the checks on each transceiver.
    pub(super) fn transceivers_need_negotiation(
        &self,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> bool {
        let transceivers = self.transceivers.borrow(ec).clone();
        for transceiver in transceivers {
            let slots = transceiver.slots.borrow();
            // Step 5.1: If transceiver.[[Stopping]] is true and
            //           transceiver.[[Stopped]] is false, return true.
            if slots.stopping && !slots.stopped {
                return true;
            }
            // Step 5.2: If transceiver isn't stopped and isn't yet associated
            //           with an m= section in description, return true.
            if slots.stopped {
                continue;
            }
            if slots.mid.is_none() {
                return true;
            }
            // Step 5.3: If transceiver isn't stopped and is associated with an
            //           m= section in description then perform the following
            //           checks:
            // Step 5.3.1: If transceiver.[[Direction]] is "sendrecv" or
            //             "sendonly", and the associated m= section in
            //             description either doesn't contain a single
            //             "a=msid" line, or the number of MSIDs from the
            //             "a=msid" lines in this m= section, or the MSID
            //             values themselves, differ from what is in
            //             transceiver.sender.[[AssociatedMediaStreamIds]],
            //             return true.
            // Step 5.3.2: If description is of type "offer", and the
            //             direction of the associated m= section in neither
            //             the offer nor answer matches
            //             transceiver.[[Direction]], return true.
            // Step 5.3.3: If description is of type "answer", and the
            //             direction of the associated m= section in the
            //             answer does not match transceiver.[[Direction]]
            //             intersected with the offered direction, return
            //             true.
            // Note: The negotiated direction the WebRTC process reported
            // ([[CurrentDirection]]) stands in for the m= section's
            // direction; the msid lines are not compared.
            match slots.current_direction {
                None => return true,
                Some(current) if current != slots.direction => return true,
                Some(_) => {}
            }
        }
        false
    }

    /// Close the connection, step 4: stop every transceiver that is not
    /// stopped, with disappear set to true.
    pub(super) fn stop_transceivers_on_close(&self, ec: &mut dyn ExecutionContext<Types>) {
        let transceivers = self.transceivers.borrow(ec).clone();
        for transceiver in transceivers {
            if transceiver.stopped() {
                continue;
            }
            self.update_capture(&transceiver, false, ec);
            self.stop_playout(&transceiver, ec);
            if let Err(error) = transceiver.stop_the_rtcrtptransceiver(true, 0.0, ec) {
                log::error!("[webrtc] stop transceiver on close: {}", error.display());
            }
        }
    }

    /// The receivers' tracks of the connection leave their muted state once
    /// media can flow.
    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcrtpreceiver>
    pub(super) fn receiving_tracks_unmuted(
        &self,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let transceivers = self.transceivers.borrow(ec).clone();
        for transceiver in transceivers {
            let receives = transceiver
                .current_direction()
                .is_some_and(direction_receives);
            if receives {
                transceiver
                    .receiver
                    .track()
                    .set_a_tracks_muted_state(false, time_millis, ec)?;
            }
        }
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#set-description>, steps 4.7.10 to
    /// 4.7.16: the transceivers a description negotiated.
    pub(super) fn apply_transceiver_states(
        &self,
        states: Vec<TransceiverState>,
        remote: bool,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 4.7.10: Let trackEventInits, muteTracks, addList, removeList
        //              and errorList be empty lists.
        let mut track_events: Vec<(RTCRtpTransceiver, Vec<MediaStream>)> = Vec::new();
        let mut mute_tracks: Vec<MediaStreamTrack> = Vec::new();
        let mut remove_list: Vec<(MediaStream, MediaStreamTrack)> = Vec::new();
        for state in states {
            // Step 4.7.11: If description is of type "answer" or "pranswer",
            //              then run the following steps: ... for each
            //              transceiver ... if the associated m= section is
            //              rejected ... stop the RTCRtpTransceiver.
            // Step 4.7.12: If description is of type "offer" and remote is
            //              true ... for each m= section without a transceiver:
            //              create an RTCRtpReceiver, an RTCRtpSender with a
            //              null track, an RTCRtpTransceiver with direction
            //              "recvonly", and add it to the set of transceivers.
            let transceiver = match self.transceiver_by_id(state.id, ec) {
                Some(transceiver) => transceiver,
                None => {
                    if !state.created_by_remote {
                        continue;
                    }
                    let sender =
                        RTCRtpSender::create(None, state.kind, &[], state.id, self.id, ec)?;
                    let receiver = RTCRtpReceiver::create(state.kind, self.id, state.id, ec)?;
                    let transceiver = RTCRtpTransceiver::create(
                        state.id,
                        state.kind,
                        sender,
                        receiver,
                        TransceiverDirection::Recvonly,
                        false,
                        ec,
                    )?;
                    self.transceivers.borrow_mut(ec).push(transceiver.clone());
                    // The WebRTC process learns the direction content gave the
                    // transceiver ("recvonly") before the answer is created.
                    self.upsert_transceiver(&transceiver, ec);
                    transceiver
                }
            };
            // Step 4.7.13: Set transceiver.[[JsepMid]] to the mid of the m=
            //              section, and set transceiver.[[Mid]] to
            //              transceiver.[[JsepMid]].
            // Step 4.7.14: If description is of type "answer" or "pranswer",
            //              set transceiver.[[CurrentDirection]] and
            //              transceiver.[[FiredDirection]] to the direction the
            //              m= section negotiated.
            let (previously_fired, sent_now) = {
                let mut slots = transceiver.slots.borrow_mut();
                slots.jsep_mid = state.mid.clone();
                slots.mid = state.mid.clone();
                slots.current_direction = state.current_direction;
                if state.current_direction.is_some_and(direction_sends) {
                    slots.sent_once = true;
                }
                if state.stopped && !slots.stopped {
                    slots.stopping = true;
                }
                (slots.fired_direction, state.stopped)
            };
            if sent_now && !transceiver.stopped() {
                // A transceiver the remote description rejected is stopped.
                transceiver.stop_the_rtcrtptransceiver(false, time_millis, ec)?;
                self.update_capture(&transceiver, false, ec);
                self.stop_playout(&transceiver, ec);
                continue;
            }
            // The negotiated direction decides whether the sender's capture
            // track feeds the connection.
            let sends = state.current_direction.is_some_and(direction_sends);
            self.update_capture(&transceiver, sends, ec);
            if !remote {
                continue;
            }
            // Step 4.7.15: If remote is true, for each transceiver whose m=
            //              section direction (from this peer's view) is
            //              "sendrecv" or "recvonly":
            let Some(remote_direction) = state.remote_direction else {
                continue;
            };
            let direction = invert(remote_direction);
            if direction_receives(direction) {
                // Step 4.7.15.1: Let msids be a list of the MSIDs that the
                //                media description indicates transceiver is
                //                to be associated with.
                // Step 4.7.15.2: Process the addition of a remote track with
                //                transceiver and msids, when
                //                transceiver.[[FiredDirection]] is neither
                //                "sendrecv" nor "recvonly", or the msids
                //                changed.
                let known = transceiver.receiver.associated_remote_media_streams();
                let msids = state.remote_stream_ids.clone();
                let fired_receiving = previously_fired.is_some_and(direction_receives);
                if !fired_receiving || known != msids {
                    // Process the addition of a remote track, steps 1 to 6:
                    // for each id in msids, find or create the MediaStream,
                    // add the receiver's track to it and to trackEventInits.
                    let mut streams = Vec::new();
                    for id in &msids {
                        let stream = self.remote_stream(id, ec)?;
                        streams.push(stream);
                    }
                    transceiver
                        .receiver
                        .set_associated_remote_media_streams(msids);
                    track_events.push((transceiver.clone(), streams));
                }
            } else if previously_fired.is_some_and(direction_receives) {
                // Step 4.7.16: If remote is true and the m= section direction
                //              is "sendonly" or "inactive", process the
                //              removal of a remote track: for each stream the
                //              receiver's track is in, add (stream, track) to
                //              removeList; add the track to muteTracks.
                for id in transceiver.receiver.associated_remote_media_streams() {
                    if let Some(stream) = self.existing_remote_stream(&id, ec) {
                        remove_list.push((stream, transceiver.receiver.track()));
                    }
                }
                mute_tracks.push(transceiver.receiver.track());
            }
            // Step 4.7.17: Set transceiver.[[FiredDirection]] to direction.
            transceiver.slots.borrow_mut().fired_direction = Some(direction);
        }
        // Step 4.7.20: For each track in muteTracks, set the muted state of
        //              track to the value true.
        for track in mute_tracks {
            track.set_a_tracks_muted_state(true, time_millis, ec)?;
        }
        // Step 4.7.21: For each stream and track pair in removeList, remove
        //              the track track from stream.
        for (stream, track) in remove_list {
            stream.remove_a_track(track, time_millis, ec)?;
        }
        // Step 4.7.22: For each stream and track pair in addList, add the
        //              track track to stream.
        for (transceiver, streams) in &track_events {
            for stream in streams {
                stream.add_a_track(transceiver.receiver.track(), time_millis, ec)?;
            }
        }
        // Step 4.7.23: For each entry entry in trackEventInits, fire an event
        //              named track using the RTCTrackEvent interface with its
        //              receiver attribute initialized to entry.receiver, its
        //              track attribute initialized to entry.track, its
        //              streams attribute initialized to entry.streams and its
        //              transceiver attribute initialized to
        //              entry.transceiver at the connection object.
        for (transceiver, streams) in track_events {
            let event = RTCTrackEvent::new(
                String::from("track"),
                transceiver.receiver.clone(),
                transceiver.receiver.track(),
                streams,
                transceiver,
                ec,
            );
            fire_event_using(&self.event_target, event, time_millis, ec)?;
        }
        Ok(())
    }

    /// Process the addition of a remote track, step 2: the MediaStream
    /// object with the given id, created when none exists.
    fn remote_stream(
        &self,
        id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<MediaStream, Types> {
        if let Some(stream) = self.existing_remote_stream(id, ec) {
            return Ok(stream);
        }
        let stream = MediaStream::create_with_id(id.to_owned(), Vec::new(), ec)?;
        self.remote_streams.borrow_mut(ec).push(stream.clone());
        Ok(stream)
    }

    fn existing_remote_stream(
        &self,
        id: &str,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<MediaStream> {
        self.remote_streams
            .borrow(ec)
            .iter()
            .find(|stream| stream.id() == id)
            .cloned()
    }
}

/// The audio device paths, in the graphics process: the default input is
/// captured for a sending audio transceiver whose sender holds a capture
/// track, and the decoded audio of a receiving transceiver plays out until
/// the transceiver stops.
impl RTCPeerConnection {
    fn graphics_sender(
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<ipc::IpcSender<GraphicsCommand>> {
        with_global_scope(ec, |global_scope, _ec| Ok(global_scope.graphics_sender()))
            .ok()
            .flatten()
    }

    /// Start or stop capturing for a transceiver, following whether it sends
    /// a live capture track.
    pub(super) fn update_capture(
        &self,
        transceiver: &RTCRtpTransceiver,
        sends: bool,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let has_capture_track = transceiver.sender.track(ec).is_some_and(|track| {
            matches!(track.source(), TrackSource::Capture { .. })
                && track.ready_state() == MediaStreamTrackState::Live
        });
        let wanted = sends && has_capture_track && transceiver.kind() == TrackKind::Audio;
        {
            let mut slots = transceiver.slots.borrow_mut();
            if slots.capturing == wanted {
                return;
            }
            slots.capturing = wanted;
        }
        let Some(sender) = Self::graphics_sender(ec) else {
            return;
        };
        let command = if wanted {
            GraphicsCommand::StartAudioCapture {
                peer: self.id,
                transceiver: transceiver.id(),
            }
        } else {
            GraphicsCommand::StopAudioCapture {
                peer: self.id,
                transceiver: transceiver.id(),
            }
        };
        if let Err(error) = sender.send(command) {
            log::error!("[webrtc] audio capture command: {error}");
        }
    }

    /// End the playout of a transceiver's remote audio.
    pub(super) fn stop_playout(
        &self,
        transceiver: &RTCRtpTransceiver,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        if transceiver.kind() != TrackKind::Audio {
            return;
        }
        let Some(sender) = Self::graphics_sender(ec) else {
            return;
        };
        if let Err(error) = sender.send(GraphicsCommand::StopAudioPlayout {
            peer: self.id,
            transceiver: transceiver.id(),
        }) {
            log::error!("[webrtc] audio playout command: {error}");
        }
    }
}
