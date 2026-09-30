use std::cell::RefCell;
use std::rc::Rc;

use ipc_messages::webrtc::{
    DataChannelHandle, DataChannelInit, DataChannelProperties, Payload, PeerConnectionId, Request,
};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::fire_event;
use crate::html::{MessageEvent, MessageEventInit};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;

use crate::dom::fire_event_using;

type JsObject = <Types as JsTypes>::JsObject;
type JsValue = <Types as JsTypes>::JsValue;

/// The largest send buffer a channel accepts, the [[MaxBufferSize]] the send
/// steps compare against. Implementation-defined; 16 MiB.
const MAX_BUFFER_SIZE: u64 = 16 * 1024 * 1024;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannelstate>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RTCDataChannelState {
    Connecting,
    Open,
    Closing,
    Closed,
}

impl RTCDataChannelState {
    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Open => "open",
            Self::Closing => "closing",
            Self::Closed => "closed",
        }
    }
}

/// <https://w3c.github.io/webrtc-pc/#dom-binarytype>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryType {
    Blob,
    ArrayBuffer,
}

impl BinaryType {
    pub(crate) fn as_idl(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::ArrayBuffer => "arraybuffer",
        }
    }
}

/// The data argument of send(), after Web IDL overload resolution.
pub(crate) enum SendData {
    /// The USVString overload.
    Text(String),
    /// The ArrayBuffer and ArrayBufferView overloads: a copy of the bytes.
    /// Note: Blob is not implemented, so there is no Blob overload.
    Bytes(Vec<u8>),
}

/// The internal slots of an RTCDataChannel, shared by every clone of it.
#[derive(Debug)]
pub(crate) struct DataChannelSlots {
    /// <https://w3c.github.io/webrtc-pc/#dfn-readystate>
    pub(crate) ready_state: RTCDataChannelState,
    /// <https://w3c.github.io/webrtc-pc/#dfn-bufferedamount>
    pub(crate) buffered_amount: u64,
    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-bufferedamountlowthreshold>
    pub(crate) buffered_amount_low_threshold: u64,
    /// <https://w3c.github.io/webrtc-pc/#dfn-datachannellabel>
    pub(crate) label: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-ordered>
    pub(crate) ordered: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-maxpacketlifetime>
    pub(crate) max_packet_life_time: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-maxretransmits>
    pub(crate) max_retransmits: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-datachannelprotocol>
    pub(crate) protocol: String,
    /// <https://w3c.github.io/webrtc-pc/#dfn-negotiated>
    pub(crate) negotiated: bool,
    /// <https://w3c.github.io/webrtc-pc/#dfn-datachannelid>
    pub(crate) id: Option<u16>,
    /// <https://w3c.github.io/webrtc-pc/#dfn-istransferable>
    pub(crate) is_transferable: bool,
    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-binarytype>
    pub(crate) binary_type: BinaryType,
    /// Whether the closing procedure was started by close().
    pub(crate) closing_initiated_by_close: bool,
    /// Whether the channel was announced as open.
    pub(crate) open_announced: bool,
    /// Whether the underlying data transport reported an error, for the
    /// closed steps' step 5 ("If the transport was closed with an error").
    pub(crate) transport_error: bool,
    /// The bytes this channel has handed to the WebRTC process in total, and
    /// the last report of what the underlying data transport still buffers:
    /// its amount once content had sent `through` bytes.
    pub(crate) sent_total: u64,
    pub(crate) reported_through: u64,
    pub(crate) reported_amount: u64,
}

/// <https://w3c.github.io/webrtc-pc/#dom-rtcdatachannel>
#[gc_struct]
pub(crate) struct RTCDataChannel {
    /// The channel's EventTarget base.
    pub(crate) event_target: EventTarget,

    /// The internal slots.
    #[ignore_trace]
    pub(crate) slots: Rc<RefCell<DataChannelSlots>>,

    /// The peer connection whose underlying data transport carries the
    /// channel.
    #[ignore_trace]
    pub(crate) peer: PeerConnectionId,

    /// The channel's handle in the WebRTC process.
    #[ignore_trace]
    pub(crate) handle: DataChannelHandle,

    /// The origin of the connection's [[DocumentOrigin]], serialized, for
    /// the message events.
    #[ignore_trace]
    pub(crate) document_origin: String,
}

impl EventTargetAccess for RTCDataChannel {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl RTCDataChannel {
    /// <https://w3c.github.io/webrtc-pc/#dfn-create-an-rtcdatachannel>
    /// The platform object is created in the current realm and returned
    /// re-read from its wrapper, so its reflector is set.
    pub(crate) fn create(
        peer: PeerConnectionId,
        handle: DataChannelHandle,
        document_origin: String,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        // Step 1: Let channel be a newly created RTCDataChannel object.
        // Step 2: Let channel have a [[ReadyState]] internal slot initialized
        //         to "connecting".
        // Step 3: Let channel have a [[BufferedAmount]] internal slot
        //         initialized to 0.
        // Step 4: Let channel have internal slots named [[DataChannelLabel]],
        //         [[Ordered]], [[MaxPacketLifeTime]], [[MaxRetransmits]],
        //         [[DataChannelProtocol]], [[Negotiated]], and
        //         [[DataChannelId]].
        // Step 5: Let channel have a [[IsTransferable]] internal slot
        //         initialized to true.
        let channel = Self {
            event_target: EventTarget::new(ec),
            slots: Rc::new(RefCell::new(DataChannelSlots {
                ready_state: RTCDataChannelState::Connecting,
                buffered_amount: 0,
                buffered_amount_low_threshold: 0,
                label: String::new(),
                ordered: true,
                max_packet_life_time: None,
                max_retransmits: None,
                protocol: String::new(),
                negotiated: false,
                id: None,
                is_transferable: true,
                binary_type: BinaryType::ArrayBuffer,
                closing_initiated_by_close: false,
                open_announced: false,
                transport_error: false,
                sent_total: 0,
                reported_through: 0,
                reported_amount: 0,
            })),
            peer,
            handle,
            document_origin,
        };
        // Step 6: Queue a task to run the following step:
        // Step 6.1: Set channel.[[IsTransferable]] to false.
        // Note: RTCDataChannel is not transferable here (it is not
        // [Transferable] in this implementation), so [[IsTransferable]] has
        // no reader; the slot is set to false by send() as the spec says.
        // Step 7: Return channel.
        let object = create_interface_instance::<Types, RTCDataChannel>(channel, ec)?;
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<RTCDataChannel>().cloned())
            .ok_or_else(|| ec.new_type_error("RTCDataChannel instance is not an RTCDataChannel"))
    }

    /// The channel's platform object.
    pub(crate) fn object(&self) -> Option<JsObject> {
        self.event_target.reflector.clone()
    }

    /// The properties of a channel the remote peer opened
    /// (<https://w3c.github.io/webrtc-pc/#announcing-a-data-channel-instance>,
    /// steps 4-6).
    pub(crate) fn initialize_from(&self, properties: &DataChannelProperties) {
        let mut slots = self.slots.borrow_mut();
        slots.label = properties.label.clone();
        slots.ordered = properties.ordered;
        slots.max_packet_life_time = properties.max_packet_life_time;
        slots.max_retransmits = properties.max_retransmits;
        slots.protocol = properties.protocol.clone();
        slots.id = properties.id;
        slots.negotiated = false;
    }

    /// The init the WebRTC process needs to create the underlying data
    /// transport (createDataChannel step 23).
    pub(crate) fn init_for_transport(&self) -> DataChannelInit {
        let slots = self.slots.borrow();
        DataChannelInit {
            ordered: slots.ordered,
            max_packet_life_time: slots.max_packet_life_time,
            max_retransmits: slots.max_retransmits,
            protocol: slots.protocol.clone(),
            negotiated: slots.negotiated,
            id: slots.id,
        }
    }

    fn webrtc_sender(ec: &mut dyn ExecutionContext<Types>) -> Option<ipc::IpcSender<Request>> {
        with_global_scope(ec, |global_scope, _ec| Ok(global_scope.webrtc_sender()))
            .ok()
            .flatten()
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-send>
    pub(crate) fn send(
        &self,
        data: SendData,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let channel be the RTCDataChannel object on which data is
        //         to be sent.
        // Step 2: Set channel.[[IsTransferable]] to false.
        self.slots.borrow_mut().is_transferable = false;
        // Step 3: If channel.[[ReadyState]] is not "open", throw an
        //         InvalidStateError.
        if self.slots.borrow().ready_state != RTCDataChannelState::Open {
            return Err(crate::webidl::invalid_state_error_value(ec));
        }
        // Step 4: Execute the sub step that corresponds to the type of the
        //         methods argument:
        let payload = match data {
            // string object: Let data be a byte buffer that represents the
            // result of encoding the method's argument as UTF-8.
            SendData::Text(text) => Payload::Text(text),
            // ArrayBuffer object: Let data be the data stored in the buffer
            // described by the ArrayBuffer object.
            // ArrayBufferView object: Let data be the data stored in the
            // section of the buffer described by the ArrayBuffer object that
            // the ArrayBufferView object references.
            // Blob object: Let data be the raw data represented by the Blob
            // object.
            // Note: Blob is not implemented in this user agent.
            SendData::Bytes(bytes) => Payload::Binary(bytes),
        };
        let size = match &payload {
            Payload::Text(text) => text.len() as u64,
            Payload::Binary(bytes) => bytes.len() as u64,
        };
        // Step 5: If the byte size of data exceeds the value of
        //         maxMessageSize on channel's associated RTCSctpTransport,
        //         throw a TypeError.
        // Note: RTCSctpTransport is not implemented; the WebRTC process
        // enforces the negotiated maximum message size.
        // Step 6: If the value of [[BufferedAmount]] increased by the byte
        //         size of data is above [[MaxBufferSize]], throw an
        //         OperationError.
        if self.slots.borrow().buffered_amount + size > MAX_BUFFER_SIZE {
            return Err(crate::webidl::operation_error_value(
                String::from("send buffer is full"),
                ec,
            ));
        }
        // Step 7: In parallel, transmit data on channel's underlying data
        //         transport.
        // Note: The bufferedAmount attribute's getter steps increase
        // [[BufferedAmount]] by the byte size of data here, before the
        // transmission; the WebRTC process reports what is still buffered.
        let through = {
            let mut slots = self.slots.borrow_mut();
            slots.buffered_amount += size;
            slots.sent_total += size;
            slots.sent_total
        };
        if let Some(sender) = Self::webrtc_sender(ec)
            && let Err(error) = sender.send(Request::DataChannelSend {
                peer: self.peer,
                channel: self.handle,
                payload,
                through,
            })
        {
            log::error!("[webrtc] send: {error}");
        }
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-close>
    pub(crate) fn close(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: Let channel be the RTCDataChannel object which is about to
        //         be closed.
        // Step 2: If channel.[[ReadyState]] is "closing" or "closed", then
        //         abort these steps.
        {
            let mut slots = self.slots.borrow_mut();
            if matches!(
                slots.ready_state,
                RTCDataChannelState::Closing | RTCDataChannelState::Closed
            ) {
                return;
            }
            // Step 3: Set channel.[[ReadyState]] to "closing".
            slots.ready_state = RTCDataChannelState::Closing;
            slots.closing_initiated_by_close = true;
        }
        // Step 4: If the closing procedure has not started yet, start it.
        self.closing_procedure(0.0, ec);
    }

    /// <https://w3c.github.io/webrtc-pc/#data-transport-closing-procedure>
    fn closing_procedure(&self, time_millis: f64, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: Let channel be the RTCDataChannel object whose underlying
        //         data transport was closed.
        // Step 2: Let connection be the RTCPeerConnection object associated
        //         with channel.
        // Step 3: Remove channel from connection.[[DataChannels]].
        let peer = self.peer;
        let handle = self.handle;
        if let Err(error) = with_global_scope(ec, move |global_scope, ec| {
            if let Some(connection) = global_scope.peer_connection(peer, ec) {
                connection.remove_from_data_channels(handle, ec);
            }
            Ok(())
        }) {
            log::error!(
                "failed to remove the data channel from its connection: {}",
                error.display()
            );
        }
        // Step 4: Unless the procedure was initiated by channel.close, set
        //         channel.[[ReadyState]] to "closing" and fire an event named
        //         closing at channel.
        let initiated_by_close = self.slots.borrow().closing_initiated_by_close;
        if !initiated_by_close {
            self.slots.borrow_mut().ready_state = RTCDataChannelState::Closing;
            if let Err(error) = fire_event(ec, self, "closing", time_millis, false) {
                log::error!("[webrtc] closing event: {error:?}");
            }
        }
        // Step 5: Run the following steps in parallel:
        // Step 5.1: Finish sending all currently pending messages of the
        //           channel.
        // Step 5.2: Follow the closing procedure defined for the channel's
        //           underlying data transport:
        // Step 5.2.1: In the case of an SCTP-based transport, follow
        //             [[RFC8831]], section 6.7.
        // Step 5.3: Close the channel's data transport by following the
        //           associated procedure.
        // Note: These run in the WebRTC process, which reports the closed
        // transport as `PeerEvent::DataChannelClosed`.
        if let Some(sender) = Self::webrtc_sender(ec)
            && let Err(error) = sender.send(Request::DataChannelClose {
                peer: self.peer,
                channel: self.handle,
            })
        {
            log::error!("[webrtc] close: {error}");
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#announce-datachannel-open>
    pub(crate) fn announce_open(
        &self,
        connection_is_closed: bool,
        id: Option<u16>,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: If the associated RTCPeerConnection object's [[IsClosed]]
        //         slot is true, abort these steps.
        if connection_is_closed {
            return Ok(());
        }
        // Step 2: Let channel be the RTCDataChannel object to be announced.
        {
            let mut slots = self.slots.borrow_mut();
            // A channel is announced once: a channel the remote peer opened
            // is announced when it is announced (see
            // `RTCPeerConnection::announce_data_channel`), before the
            // WebRTC process reports it open.
            if std::mem::replace(&mut slots.open_announced, true) {
                return Ok(());
            }
            // Step 3: If channel.[[ReadyState]] is "closing" or "closed",
            //         abort these steps.
            if matches!(
                slots.ready_state,
                RTCDataChannelState::Closing | RTCDataChannelState::Closed
            ) {
                return Ok(());
            }
            // Step 4: Set channel.[[ReadyState]] to "open".
            slots.ready_state = RTCDataChannelState::Open;
            // Note: The SCTP stream id is known once the channel is open.
            if slots.id.is_none() {
                slots.id = id;
            }
        }
        // Step 5: Fire an event named open at channel.
        fire_event(ec, self, "open", time_millis, false).map(|_| ())
    }

    /// <https://w3c.github.io/webrtc-pc/#dfn-receiving-messages-on-a-data-channel>
    pub(crate) fn receive_message(
        &self,
        payload: Payload,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let channel be the RTCDataChannel object for which the user
        //         agent has received a message.
        // Step 2: Let connection be the RTCPeerConnection object associated
        //         with channel.
        // Step 3: If channel.[[ReadyState]] is not "open", abort these steps
        //         and discard rawData.
        if self.slots.borrow().ready_state != RTCDataChannelState::Open {
            return Ok(());
        }
        // Step 4: Execute the sub step by switching on type and
        //         channel.binaryType:
        let data: JsValue = match payload {
            // If type indicates that rawData is a string: Let data be a
            // DOMString that represents the result of decoding rawData as
            // UTF-8.
            Payload::Text(text) => {
                let string = ec.js_string_from_str(&text);
                ec.value_from_string(string)
            }
            // If type indicates that rawData is binary and binaryType is
            // "arraybuffer": Let data be a new ArrayBuffer object whose
            // contents are rawData.
            // If type indicates that rawData is binary and binaryType is
            // "blob": Let data be a new Blob object containing rawData as its
            // raw data source.
            // Note: Blob is not implemented; binary data is delivered as an
            // ArrayBuffer whatever binaryType says.
            Payload::Binary(bytes) => {
                let buffer = crate::webidl::create_array_buffer(&bytes, ec)?;
                Types::value_from_object(buffer)
            }
        };
        // Step 5: Fire an event named message using the MessageEvent
        //         interface with its origin attribute initialized to the
        //         serialization of an origin of connection.[[DocumentOrigin]],
        //         and the data attribute initialized to data at channel.
        let event = MessageEvent::new(
            String::from("message"),
            MessageEventInit {
                bubbles: false,
                cancelable: false,
                composed: false,
                data,
                origin: self.document_origin.clone(),
                last_event_id: String::new(),
                source: None,
                ports: Vec::new(),
            },
            ec,
        );
        fire_event_using(&self.event_target, event, time_millis, ec).map(|_| ())
    }

    /// A report from the WebRTC process of what the underlying data
    /// transport still buffers.
    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-bufferedamount>
    pub(crate) fn update_buffered_amount(
        &self,
        through: u64,
        amount: u64,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // "This value is decreased by the user agent in a task, after data
        // has been sent" (the attribute's description); a
        // bufferedamountlow event fires when it decreases "from above the
        // bufferedAmountLowThreshold to less than or equal to it".
        let (before, after, threshold) = {
            let mut slots = self.slots.borrow_mut();
            if through < slots.reported_through {
                return Ok(());
            }
            slots.reported_through = through;
            slots.reported_amount = amount;
            let before = slots.buffered_amount;
            let after = amount + (slots.sent_total - through);
            slots.buffered_amount = after;
            (before, after, slots.buffered_amount_low_threshold)
        };
        if before > threshold && after <= threshold {
            fire_event(ec, self, "bufferedamountlow", time_millis, false)?;
        }
        Ok(())
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-datachannel-bufferedamountlowthreshold>
    pub(crate) fn set_buffered_amount_low_threshold(
        &self,
        threshold: u64,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        self.slots.borrow_mut().buffered_amount_low_threshold = threshold;
        if let Some(sender) = Self::webrtc_sender(ec)
            && let Err(error) = sender.send(Request::DataChannelSetBufferedAmountLowThreshold {
                peer: self.peer,
                channel: self.handle,
                threshold,
            })
        {
            log::error!("[webrtc] threshold: {error}");
        }
    }

    /// <https://w3c.github.io/webrtc-pc/#data-transport-closed>
    pub(crate) fn transport_closed(
        &self,
        with_error: bool,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Let channel be the RTCDataChannel object whose underlying
        //         data transport was closed.
        // Step 2: If channel.[[ReadyState]] is "closed", abort these steps.
        if self.slots.borrow().ready_state == RTCDataChannelState::Closed {
            return Ok(());
        }
        // Step 3: Set channel.[[ReadyState]] to "closed".
        self.slots.borrow_mut().ready_state = RTCDataChannelState::Closed;
        // Step 4: Remove channel from connection.[[DataChannels]] if it is
        //         still there.
        let peer = self.peer;
        let handle = self.handle;
        with_global_scope(ec, move |global_scope, ec| {
            if let Some(connection) = global_scope.peer_connection(peer, ec) {
                connection.remove_from_data_channels(handle, ec);
            }
            Ok(())
        })?;
        // Step 5: If the transport was closed with an error, fire an event
        //         named error using the RTCErrorEvent interface with its
        //         errorDetail attribute set to "sctp-failure" at channel.
        // Note: RTCErrorEvent is not implemented; a plain Event named error
        // fires.
        if with_error {
            fire_event(ec, self, "error", time_millis, false)?;
        }
        // Step 6: Fire an event named close at channel.
        fire_event(ec, self, "close", time_millis, false).map(|_| ())
    }

    /// Set [[ReadyState]] to "closed" without events (close the connection,
    /// step 5).
    pub(crate) fn set_closed(&self) {
        self.slots.borrow_mut().ready_state = RTCDataChannelState::Closed;
    }
}
