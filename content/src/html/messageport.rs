use ipc::IpcSender;
use ipc_messages::content::{Event as ContentEvent, MessageId, PortId};
use ipc_messages::safe_passing_of_structured_data::{PortMessagePayload, PortTransferData};
use js_engine::gc_struct;
use js_engine::{Completion, ExecutionContext, JsTypes};

use crate::dom::Event;
use crate::dom::dispatch_with_path;
use crate::dom::event::{EventTarget, EventTargetAccess};
use crate::dom::simple_path;
use crate::html::structured_data::safe_passing_of_structured_data::{
    SerializeWithTransferResult, Transferable, structured_deserialize_with_transfer,
    structured_serialize_with_transfer,
};
use crate::js::Types;
use crate::js::platform_objects::with_global_scope;
use crate::webidl::bindings::create_interface_instance;

use super::{MessageEvent, MessageEventInit};

use crate::html::channel_messaging::ChannelMessaging;

type JsValue = <Types as JsTypes>::JsValue;
type JsObject = <Types as JsTypes>::JsObject;

/// <https://html.spec.whatwg.org/#messageport>
#[gc_struct]
pub(crate) struct MessagePort {
    /// The port's EventTarget base; as the port's message event target
    /// defaults to the port itself, message and messageerror events are
    /// dispatched through this target.
    pub(crate) event_target: EventTarget,

    /// The id under which the user agent's channel messaging state and
    /// this realm's ChannelMessaging know the port.
    #[ignore_trace]
    pub(crate) port_id: PortId,
}

impl EventTargetAccess for MessagePort {
    fn get_event_target(&self, _ec: &mut dyn ExecutionContext<Types>) -> EventTarget {
        self.event_target.clone()
    }
}

impl MessagePort {
    /// The port's platform object, resolved through the reflector stored
    /// on the port's event target.
    pub(crate) fn object(&self) -> Option<JsObject> {
        self.event_target.reflector.clone()
    }

    /// Create a new MessagePort platform object in the current realm with a
    /// fresh id not yet registered with the user agent (the "a new
    /// MessagePort in this's relevant realm" of the MessageChannel
    /// constructor steps).
    pub(crate) fn new_port(ec: &mut dyn ExecutionContext<Types>) -> Completion<Self, Types> {
        Self::new_port_with_id(PortId::new(), ec)
    }

    /// Create a new MessagePort platform object in the current realm with
    /// the given id (the "a new MessagePort in targetRealm" of
    /// StructuredDeserializeWithTransfer step 3.2), returning the port
    /// re-read from its wrapper.
    pub(crate) fn new_port_with_id(
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<Self, Types> {
        let port = Self {
            event_target: EventTarget::new(ec),
            port_id,
        };
        let object = create_interface_instance::<Types, MessagePort>(port, ec)?;
        // The port returned to the caller is re-read from its wrapper,
        // whose reflector was set by the interface instance creation.
        ec.with_object_any(&object)
            .and_then(|data| data.downcast_ref::<MessagePort>().cloned())
            .ok_or_else(|| ec.new_type_error("MessagePort instance is not a MessagePort"))
    }

    /// The current realm's ChannelMessaging, created on first use; `None`
    /// when the realm has no event loop yet.  Port operations all run in
    /// the port's own realm (the bindings run in the creation realm, and a
    /// transferred port is created in the receiving realm before use), so
    /// the current realm is the port's realm.
    fn messaging(ec: &mut dyn ExecutionContext<Types>) -> Option<ChannelMessaging> {
        with_global_scope(
            ec,
            |global_scope, ec| Ok(global_scope.channel_messaging(ec)),
        )
        .ok()
        .flatten()
    }

    /// The current realm's content-to-user-agent event sender, if set.
    fn event_sender(ec: &mut dyn ExecutionContext<Types>) -> Option<IpcSender<ContentEvent>> {
        with_global_scope(ec, |global_scope, _ec| Ok(global_scope.event_sender()))
            .ok()
            .flatten()
    }

    /// Whether `transfer` contains a MessagePort wrapping `port_id`: the
    /// platform-object equivalent of the spec's JS object identity
    /// membership, sound because each port id is unique to one platform
    /// object in a realm.
    fn transfer_contains_port(
        transfer: &[JsValue],
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> bool {
        transfer
            .iter()
            .filter_map(Types::value_as_object)
            .any(|object| {
                ec.with_object_any(&object)
                    .and_then(|data| data.downcast_ref::<MessagePort>().cloned())
                    .is_some_and(|port| port.port_id == port_id)
            })
    }

    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    pub(crate) fn post_message(
        &self,
        message: JsValue,
        transfer: Vec<JsValue>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some(messaging) = Self::messaging(ec) else {
            return Ok(());
        };
        let Some(event_sender) = Self::event_sender(ec) else {
            return Ok(());
        };

        // Step 1: Let targetPort be the port with which this is entangled,
        //         if any; otherwise let it be null.
        let target_port: Option<PortId> = messaging
            .port_record(self.port_id, ec)
            .and_then(|record| record.entangled);

        // Step 2: If transfer contains sourcePort, then throw a
        //         "DataCloneError" DOMException.
        if Self::transfer_contains_port(&transfer, self.port_id, ec) {
            return Err(crate::webidl::data_clone_error_value(ec));
        }

        // Step 3: Let doomed be false.
        // Step 4: If targetPort is not null and transfer contains targetPort,
        //         then set doomed to true and optionally report to a developer
        //         console that the target port was posted to itself, causing
        //         the communication channel to be lost.
        // Note: Membership in `transfer` is decided by the transferred
        // objects' platform objects (a MessagePort matches when it carries
        // the target port's id), the platform-object equivalent of the
        // spec's JS identity comparison.
        let doomed = target_port
            .is_some_and(|target_id| Self::transfer_contains_port(&transfer, target_id, ec));

        // Step 5: Let serializeWithTransferResult be
        //         StructuredSerializeWithTransfer(message, transfer). Rethrow
        //         any exceptions.
        let serialize_result = structured_serialize_with_transfer(&message, transfer, ec)?;

        // Step 6: If targetPort is null, or if doomed is true, then return.
        if target_port.is_none() || doomed {
            return Ok(());
        }

        // Step 7: Add a task that runs the following steps to the port
        //         message queue of targetPort.
        // Note: The delivery is `MessagePort.tla`'s `PostMessage` (step 7): the message goes
        // straight into the target's queue when the target is managed by
        // this same event loop, and is appended to the user agent's routing
        // queue otherwise.  The task's substeps (7.1-7.7) run when the
        // message event fires (`run_message_task`): for a routed message the
        // delivering task itself runs them, and for a directly queued
        // message the queued message task does.
        let payload = PortMessagePayload {
            message_id: MessageId::new(),
            serialized: serialize_result.serialized,
            transfer_data_holders: serialize_result.transfer_data_holders,
        };
        let Some(target_port) = target_port else {
            return Ok(());
        };
        messaging
            .post_message(self.port_id, Some(target_port), payload, &event_sender, ec)
            .map_err(|error| ec.new_type_error(&format!("postMessage: {error}")))
    }

    /// <https://html.spec.whatwg.org/#dom-messageport-start>
    pub(crate) fn start(&self, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: The start() method steps are to enable this's port
        //         message queue, if it is not already enabled.
        // Note: The enabling runs in the per-global ChannelMessaging
        // (`messaging.start`), which also requests message tasks for any
        // pending messages.
        let Some(messaging) = Self::messaging(ec) else {
            return;
        };
        messaging.start(self.port_id, ec);
    }

    /// <https://html.spec.whatwg.org/#dom-messageport-close>
    pub(crate) fn close(&self, ec: &mut dyn ExecutionContext<Types>) -> Completion<(), Types> {
        // Step 1: Set this's [[Detached]] internal slot value to true.
        // Step 2: If this is entangled, disentangle it.
        // Note: Steps 1-3 run in the per-global ChannelMessaging
        // (`messaging.close`), which detaches the record and returns the
        // entangled twin; step 4 of the disentangle steps (fire an event
        // named close at otherPort) runs below.
        let Some(messaging) = Self::messaging(ec) else {
            return Ok(());
        };
        let other = messaging.close(self.port_id, ec);
        if let Some(other) = other {
            // Step 4: Fire an event named close at otherPort.
            if let Some(other_port) = messaging.port_object(other, ec) {
                fire_close_event(&other_port, ec)?;
            }
        }
        Ok(())
    }

    /// Enable the port's message queue, as when start() is called or the
    /// first onmessage handler is set.
    pub(crate) fn enable_queue(&self, ec: &mut dyn ExecutionContext<Types>) {
        let Some(messaging) = Self::messaging(ec) else {
            return;
        };
        messaging.enable_queue(self.port_id, ec);
    }

    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    pub(crate) fn run_message_task(
        &self,
        time_millis: f64,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some(messaging) = Self::messaging(ec) else {
            return Ok(());
        };
        let payload = match messaging.pop_queued_message(self.port_id, ec) {
            Ok(Some(payload)) => payload,
            Ok(None) => return Ok(()),
            Err(error) => return Err(ec.new_type_error(&format!("port message task: {error}"))),
        };

        // Step 7.1: Let finalTargetPort be the MessagePort in whose port
        //           message queue the task now finds itself.
        // Note: The task runs on the port's own event loop, so
        // finalTargetPort is this port.
        // Step 7.2: Let messageEventTarget be finalTargetPort's message
        //           event target.
        // Note: The message event target defaults to the port itself.
        // Step 7.3: Let targetRealm be finalTargetPort's relevant realm.
        // Note: The current realm is the port's realm.
        let serialize_result = SerializeWithTransferResult {
            serialized: payload.serialized,
            transfer_data_holders: payload.transfer_data_holders,
        };
        // Steps 7.4-7.7: deserialize the message and fire it at the message
        // event target (the port, or the target the port's message event
        // target was set to).
        deliver_serialized_message(&self.event_target, &serialize_result, time_millis, ec)
    }
}

/// The delivery steps of the message port post message steps' message task
/// (steps 7.4-7.7): deserialize the message and fire a `message` event (or
/// a `messageerror` event when the deserialization throws) at the given
/// message event target.  Shared by the port machinery and the dedicated
/// worker channels, whose implicit ports are bypassed (direct channels) but
/// whose message delivery follows the same steps.
/// <https://html.spec.whatwg.org/#message-port-post-message-steps>
pub(crate) fn deliver_serialized_message(
    target: &EventTarget,
    serialize_result: &SerializeWithTransferResult,
    time_millis: f64,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 7.4: Let deserializeRecord be
    //           StructuredDeserializeWithTransfer(serializeWithTransferResult,
    //           targetRealm).
    let deserialize_outcome =
        structured_deserialize_with_transfer(serialize_result, &ec.value_undefined(), ec);
    let deserialize_result = match deserialize_outcome {
        Ok(result) => result,
        Err(_) => {
            // If this throws an exception, catch it, fire an event named
            // messageerror at messageEventTarget, using MessageEvent, and
            // then return.
            let message_event = MessageEvent::new(
                String::from("messageerror"),
                MessageEventInit {
                    bubbles: false,
                    cancelable: false,
                    composed: false,
                    data: ec.value_null(),
                    origin: String::new(),
                    last_event_id: String::new(),
                    source: None,
                    ports: Vec::new(),
                },
                ec,
            );
            fire_message_event(target, message_event, time_millis, ec)?;
            return Ok(());
        }
    };

    // Step 7.5: Let messageClone be deserializeRecord.[[Deserialized]].
    let message_clone = deserialize_result.deserialized;

    // Step 7.6: Let newPorts be a new frozen array consisting of all
    //           MessagePort objects in deserializeRecord.[[TransferredValues]],
    //           if any, maintaining their relative order.
    let new_ports: Vec<JsObject> = deserialize_result
        .transferred_values
        .iter()
        .filter_map(Types::value_as_object)
        .collect();

    // Step 7.7: Fire an event named message at messageEventTarget, using
    //           MessageEvent, with the data attribute initialized to
    //           messageClone and the ports attribute initialized to
    //           newPorts.
    let message_event = MessageEvent::new(
        String::from("message"),
        MessageEventInit {
            bubbles: false,
            cancelable: false,
            composed: false,
            data: message_clone,
            origin: String::new(),
            last_event_id: String::new(),
            source: None,
            ports: new_ports,
        },
        ec,
    );
    fire_message_event(target, message_event, time_millis, ec)
}

/// <https://html.spec.whatwg.org/#messagechannel>
#[gc_struct]
pub(crate) struct MessageChannel {
    /// <https://html.spec.whatwg.org/#dom-messagechannel-port1>
    pub(crate) port1: MessagePort,

    /// <https://html.spec.whatwg.org/#dom-messagechannel-port2>
    pub(crate) port2: MessagePort,
}

impl MessageChannel {
    /// <https://html.spec.whatwg.org/#dom-messagechannel>
    pub(crate) fn new_channel(ec: &mut dyn ExecutionContext<Types>) -> Completion<Self, Types> {
        // Step 1: Set this's port 1 to a new MessagePort in this's relevant
        //         realm.
        let port1 = MessagePort::new_port(ec)?;
        // Step 2: Set this's port 2 to a new MessagePort in this's relevant
        //         realm.
        let port2 = MessagePort::new_port(ec)?;
        // Step 3: Entangle this's port 1 and this's port 2.
        let Some(messaging) = MessagePort::messaging(ec) else {
            return Err(ec.new_type_error("MessageChannel: no event loop"));
        };
        messaging.entangle_pair(port1.clone(), port2.clone(), ec);
        // The user agent must know both ports to route messages to either
        // one's owning event loop (`MessagePortExtraFG.tla`'s `NewChannel`).
        if let Some(event_sender) = MessagePort::event_sender(ec)
            && let Err(error) = event_sender.send(ContentEvent::PortChannelCreated {
                event_loop: messaging.event_loop_id(),
                port1: port1.port_id,
                port2: port2.port_id,
            })
        {
            return Err(ec.new_type_error(&format!("MessageChannel: {error}")));
        }
        Ok(Self { port1, port2 })
    }
}

impl Transferable for MessagePort {
    type TransferDataHolder = PortTransferData;

    /// <https://html.spec.whatwg.org/#message-ports:transfer-steps>
    fn transfer_steps(
        &self,
        data_holder: &mut PortTransferData,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        let Some(messaging) = MessagePort::messaging(ec) else {
            return Err(crate::webidl::data_clone_error_value(ec));
        };
        let Some(event_sender) = MessagePort::event_sender(ec) else {
            return Err(crate::webidl::data_clone_error_value(ec));
        };
        // Step 3: If value is entangled with another port remotePort:
        // Step 3.1: Set remotePort's has been shipped flag to true.
        // Note: The user agent, informed of the transfer, routes messages for
        // the port away while it is in transit, which covers the remote port
        // as well.
        // Step 3.2: Set dataHolder.[[RemotePort]] to remotePort.
        // Step 4: Otherwise, set dataHolder.[[RemotePort]] to null.
        // Note: The entanglement is read before `transfer_port` removes the
        // record (steps 1-2 below), since the record no longer exists after.
        data_holder.remote_port = messaging
            .port_record(self.port_id, ec)
            .and_then(|record| record.entangled);
        // Step 1: Set value's has been shipped flag to true.
        // Step 2: Set dataHolder.[[PortMessageQueue]] to value's port message
        //         queue.
        // Note: These run in `transfer_port` (`MessagePortExtraFG.tla`'s
        // `Transfer`): the record leaves this realm and its queue is drained
        // into the transfer data holder.  The user agent is informed there so
        // it buffers or re-routes messages while the port is in transit (the
        // shipped flag's cross-process effect; step 3.1's remote port is
        // covered by the same notification).
        let (queue, in_flight) = messaging
            .transfer_port(self.port_id, &event_sender, ec)
            .map_err(|_error| crate::webidl::data_clone_error_value(ec))?;
        data_holder.queue = queue;
        data_holder.in_flight = in_flight;
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#message-ports:transfer-receiving-steps>
    fn transfer_receiving_steps(
        &self,
        data_holder: &PortTransferData,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Completion<(), Types> {
        // Step 1: Set value's has been shipped flag to true.
        // Note: The user agent is told the port was received
        // (`PortTransferReceived`) so it stops buffering messages for the
        // port; the shipped flag itself is not modelled, and the record
        // registered by `receive_transferred_port` tracks the hand-over
        // (`CompletionInProgress`).
        // Step 2: Move all the tasks that are to fire message events in
        //         dataHolder.[[PortMessageQueue]] to the port message queue
        //         of value, if any, leaving value's port message queue in
        //         its initial disabled state, and, if value's relevant
        //         global object is a Window, associating the moved tasks
        //         with value's relevant global object's associated Document.
        // Step 3: If dataHolder.[[RemotePort]] is not null, then entangle
        //         dataHolder.[[RemotePort]] and value. (This will disentangle
        //         dataHolder.[[RemotePort]] from the original port that was
        //         transferred.)
        // Note: Steps 2-3 run in `receive_transferred_port`, which moves the
        // transferred queue into the new port's record (left disabled) and
        // entangles the record with the remote port.  The new port (value, "a
        // new MessagePort in targetRealm" of StructuredDeserializeWithTransfer
        // step 3.2) was created by the deserializer before the steps ran on
        // it.
        let Some(messaging) = MessagePort::messaging(ec) else {
            return Err(ec.new_type_error("transfer receive: no event loop"));
        };
        let event_sender = MessagePort::event_sender(ec);
        messaging.receive_transferred_port(
            self.clone(),
            data_holder.remote_port,
            data_holder.queue.clone(),
            data_holder.in_flight,
            ec,
        );
        if let Some(event_sender) = event_sender
            && let Err(error) = event_sender.send(ContentEvent::PortTransferReceived {
                event_loop: messaging.event_loop_id(),
                port: self.port_id,
            })
        {
            log::error!("failed to notify the user agent of port receive: {error}");
        }
        Ok(())
    }
}

/// <https://dom.spec.whatwg.org/#concept-event-fire>
fn fire_message_event(
    target: &EventTarget,
    message_event: MessageEvent,
    time_millis: f64,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 2: Let event be the result of creating an event given
    //         eventConstructor, in the relevant realm of target.
    // Note: eventConstructor (MessageEvent) is given, so step 1 does not
    // apply; the event's type, data, and ports attributes were initialized
    // by the caller (`run_message_task`'s step 7.7, the fire algorithm's
    // steps 3-4).  Creating the event also initializes its isTrusted
    // attribute to true and its timeStamp attribute to the time of the
    // occurrence.
    let event_object = create_interface_instance::<Types, MessageEvent>(message_event, ec)?;
    let message_event: MessageEvent = ec
        .with_object_any(&event_object)
        .and_then(|data| data.downcast_ref::<MessageEvent>().cloned())
        .ok_or_else(|| ec.new_type_error("event_object is not a MessageEvent"))?;
    *message_event.event.is_trusted.borrow_mut(ec) = true;
    *message_event.event.time_stamp.borrow_mut(ec) = time_millis;
    // Step 5: Return the result of dispatching event at target, with
    //         legacy target override flag set if set.
    let path = simple_path(target, ec);
    dispatch_with_path(ec, &path, &message_event.event)
        .map(|_| ())
        .map_err(|error| ec.new_type_error(&format!("failed to dispatch event: {error:?}")))
}

/// <https://html.spec.whatwg.org/#disentangle>
fn fire_close_event(
    other_port: &MessagePort,
    ec: &mut dyn ExecutionContext<Types>,
) -> Completion<(), Types> {
    // Step 4: Fire an event named close at otherPort.
    // Note: Steps 1-3 of the disentangle steps (otherPort, the assertion,
    // and the disentangling of the pair) run in the caller
    // (`MessagePort::close` via `ChannelMessaging::close`).
    let event = Event::new(String::from("close"), false, false, false, true, 0.0, ec);
    let path = simple_path(&other_port.event_target, ec);
    dispatch_with_path(ec, &path, &event)
        .map(|_| ())
        .map_err(|error| ec.new_type_error(&format!("failed to dispatch close event: {error:?}")))
}
