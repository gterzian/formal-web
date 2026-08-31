//! Per-global channel messaging state: the content-process half of the
//! cross-process MessagePort workflow.  The message task scheduling follows
//! the HTML spec's message port post message steps (step 7 adds a task to
//! the target's port message queue; the substeps run when the target content
//! process handles the message), validated by the coarse `MessagePort.tla`
//! trace consumer; the transfer state machine of
//! `verification/tla_specs/MessagePortExtraFG.tla` models the routing
//! between event loops.
//!
//! Each realm's [`GlobalScope`] lazily creates one [`ChannelMessaging`] on
//! first port use.  It owns the [`PortRecord`]s of the ports managed by the
//! realm's event loop.  The user-agent side
//! (`user_agent/src/channel_messaging.rs`) owns the routing queue and the
//! per-port transfer state needed to route messages between event loops.

use std::collections::VecDeque;

use ipc::IpcSender;
use ipc_messages::content::{
    Event as ContentEvent, EventLoopId, PortId, PortTaskKind, TransferState,
};
use ipc_messages::safe_passing_of_structured_data::PortMessagePayload;
use js_engine::ExecutionContext;
use js_engine::gc::{GcCell, gc_cell_new};
use js_engine::gc_struct;
use log::warn;

use crate::html::event_loop::{Task, TaskQueue};
use crate::html::messageport::MessagePort;
use crate::js::Types;

use verification::{TLATracer, TraceSender};

/// One MessagePort managed by this event loop: the content-process half of
/// the spec's per-port state (its port message queue, its entanglement,
/// its detached flag), keyed by a [`PortId`] shared with the user agent's
/// routing state.
#[gc_struct]
pub(crate) struct PortRecord {
    /// The id under which the user agent's channel messaging state knows
    /// this port.
    #[ignore_trace]
    pub(crate) port_id: PortId,

    /// The port's platform object, held so the port can be resolved by id
    /// (e.g. to dispatch close at the entangled twin).
    pub(crate) object: Option<MessagePort>,

    /// The port's transfer state, mirroring the user agent's per-port
    /// state (`MessagePortExtraFG.tla`'s `ts`).
    #[ignore_trace]
    pub(crate) ts: TransferState,

    /// The port this port is entangled with, if any.
    #[ignore_trace]
    pub(crate) entangled: Option<PortId>,

    /// The port's message queue: messages delivered to the port, fired as
    /// message events while the queue is enabled.
    #[ignore_trace]
    pub(crate) queue: VecDeque<PortMessagePayload>,

    /// Whether the port's message queue is enabled (a port message queue
    /// can be enabled, and is initially disabled).
    #[ignore_trace]
    pub(crate) enabled: bool,

    /// Whether the port was closed: the port's [[Detached]] internal slot.
    #[ignore_trace]
    pub(crate) detached: bool,

    /// Routed messages still in flight toward this port, not yet landed in
    /// its queue.
    #[ignore_trace]
    in_flight: u32,
}

impl PortRecord {
    /// Whether the port is managed by (or completing a transfer to) this
    /// event loop, so messages can be delivered to its queue directly.
    fn is_local(&self) -> bool {
        matches!(
            self.ts,
            TransferState::Managed | TransferState::CompletionInProgress
        )
    }
}

/// Per-global channel messaging state: the records of the ports whose
/// queues are managed by this event loop, plus the IPC wiring (event loop
/// id, trace sender) needed to route messages and report transfer state to
/// the user agent.
#[gc_struct]
pub(crate) struct ChannelMessaging {
    /// The id of the event loop this channel messaging belongs to, reported
    /// to the user agent so it can queue tasks on the right event loop.
    #[ignore_trace]
    event_loop_id: EventLoopId,

    /// TLA trace sender for the MessagePort specs.
    #[ignore_trace]
    trace_sender: Option<TraceSender>,

    /// The records of the ports managed by this event loop.
    ports: GcCell<Vec<PortRecord>>,

    /// <https://html.spec.whatwg.org/#task-queue>
    #[ignore_trace]
    task_queue: TaskQueue,
}

impl ChannelMessaging {
    /// Create the channel messaging state for an event loop.
    pub(crate) fn new(
        event_loop_id: EventLoopId,
        trace_sender: Option<TraceSender>,
        task_queue: TaskQueue,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Self {
        Self {
            event_loop_id,
            trace_sender,
            task_queue,
            ports: gc_cell_new(Vec::new(), ec),
        }
    }

    /// Emit a MessagePort trace event (the actions of the MessagePort TLA
    /// specs).
    fn trace(&self, event: &str, args: Vec<String>) {
        let Some(sender) = &self.trace_sender else {
            return;
        };
        let mut tracer = TLATracer::new("MessagePort", "formal-web:content", Some(sender.clone()));
        tracer.log_with_location(Some("MessagePort"), event, args, file!(), line!());
    }

    /// <https://html.spec.whatwg.org/#entangle>
    /// Entangle a port managed by this event loop with a port managed by
    /// another of this process's realms (the worker channel: the outside
    /// port lives in the owner realm, the inside port in the worker realm).
    /// The record for the remote port lives in the other realm's
    /// ChannelMessaging, created by its own call to this method; the user
    /// agent is told about the pair separately (`PortChannelCreated`), so
    /// its routing can deliver messages to either event loop.
    pub(crate) fn entangle_remote(
        &self,
        port: MessagePort,
        remote_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut ports = self.ports.borrow_mut(ec);
        ports.push(PortRecord {
            port_id: port.port_id,
            object: Some(port),
            ts: TransferState::Managed,
            entangled: Some(remote_id),
            queue: VecDeque::new(),
            enabled: false,
            detached: false,
            in_flight: 0,
        });
    }

    /// <https://html.spec.whatwg.org/#entangle>
    /// Register a port managed by this event loop without an entanglement
    /// yet.  Used by the Worker constructor for the outside port, whose
    /// entanglement is set by run a worker's step 12.8 once the inside port
    /// exists.
    pub(crate) fn register_port(&self, port: MessagePort, ec: &mut dyn ExecutionContext<Types>) {
        let mut ports = self.ports.borrow_mut(ec);
        if ports.iter().any(|record| record.port_id == port.port_id) {
            return;
        }
        ports.push(PortRecord {
            port_id: port.port_id,
            object: Some(port),
            ts: TransferState::Managed,
            entangled: None,
            queue: VecDeque::new(),
            enabled: false,
            detached: false,
            in_flight: 0,
        });
    }

    /// <https://html.spec.whatwg.org/#entangle>
    /// Set the entanglement of a port managed by this event loop to a port
    /// managed by another of this process's realms.
    pub(crate) fn set_entanglement(
        &self,
        port_id: PortId,
        remote_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        let mut ports = self.ports.borrow_mut(ec);
        if let Some(record) = ports.iter_mut().find(|record| record.port_id == port_id) {
            record.entangled = Some(remote_id);
        }
    }

    /// <https://html.spec.whatwg.org/#entangle>
    pub(crate) fn entangle_pair(
        &self,
        port1: MessagePort,
        port2: MessagePort,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 1: If one of the ports is already entangled, then disentangle
        //         it and the port that it was entangled with.
        // Note: The ports are always fresh here (the MessageChannel
        // constructor creates them immediately before entangling), so there
        // is no prior entanglement to disentangle.
        // Step 2: Associate the two ports to be entangled, so that they form
        //         the two parts of a new channel.
        let mut ports = self.ports.borrow_mut(ec);
        ports.push(PortRecord {
            port_id: port1.port_id,
            object: Some(port1.clone()),
            ts: TransferState::Managed,
            entangled: Some(port2.port_id),
            queue: VecDeque::new(),
            enabled: false,
            detached: false,
            in_flight: 0,
        });
        ports.push(PortRecord {
            port_id: port2.port_id,
            object: Some(port2.clone()),
            ts: TransferState::Managed,
            entangled: Some(port1.port_id),
            queue: VecDeque::new(),
            enabled: false,
            detached: false,
            in_flight: 0,
        });
        drop(ports);
        self.trace(
            "NewChannel",
            vec![
                port1.port_id.to_string(),
                port2.port_id.to_string(),
                self.event_loop_id.to_string(),
            ],
        );
    }

    /// <https://html.spec.whatwg.org/#message-ports:transfer-receiving-steps>
    pub(crate) fn receive_transferred_port(
        &self,
        port: MessagePort,
        remote_port: Option<PortId>,
        queue: Vec<PortMessagePayload>,
        in_flight: u32,
        ec: &mut dyn ExecutionContext<Types>,
    ) {
        // Step 1: Set value's has been shipped flag to true.
        // Note: The port is registered with a `CompletionInProgress`
        // transfer state and the user agent is told the port was received
        // (`PortTransferReceived`, sent by the caller), so the user agent
        // stops buffering messages for it; the shipped flag itself is not
        // modelled.
        // Step 2: Move all the tasks that are to fire message events in
        //         dataHolder.[[PortMessageQueue]] to the port message queue
        //         of value, if any, leaving value's port message queue in
        //         its initial disabled state, and, if value's relevant
        //         global object is a Window, associating the moved tasks
        //         with value's relevant global object's associated Document.
        // Note: The queue that moved with the transfer lands in the new
        // record's queue, left disabled until start() or an onmessage
        // handler enables it.  The Window-document association of the moved
        // tasks is not modelled; the receiving realm's document is the
        // port's document.
        // Step 3: If dataHolder.[[RemotePort]] is not null, then entangle
        //         dataHolder.[[RemotePort]] and value. (This will disentangle
        //         dataHolder.[[RemotePort]] from the original port that was
        //         transferred.)
        // Note: The record is created entangled with the remote port; the
        // original port's record was removed by the transfer steps, so the
        // remote port is no longer entangled with it.
        let mut ports = self.ports.borrow_mut(ec);
        ports.push(PortRecord {
            port_id: port.port_id,
            object: Some(port.clone()),
            ts: TransferState::CompletionInProgress,
            entangled: remote_port,
            queue: queue.into(),
            enabled: false,
            detached: false,
            in_flight,
        });
        drop(ports);
        self.trace(
            "TransferReceive",
            vec![port.port_id.to_string(), self.event_loop_id.to_string()],
        );
    }

    /// <https://html.spec.whatwg.org/#message-ports:transfer-steps>
    pub(crate) fn transfer_port(
        &self,
        port_id: PortId,
        event_sender: &IpcSender<ContentEvent>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<(Vec<PortMessagePayload>, u32), String> {
        // Step 1: Set value's has been shipped flag to true.
        // Step 2: Set dataHolder.[[PortMessageQueue]] to value's port
        //         message queue.
        // Note: The record is removed (the port leaves this realm, so its
        // queue stops being a task source here) and its queue is drained
        // into the transfer data holder returned to the caller.  This is
        // the cross-process equivalent of `MessagePortExtraFG.tla`'s
        // `Transfer` (which keeps the record with `owner` set to
        // `NoEventLoopId`): the user agent is informed below so it can
        // buffer or re-route messages for the port while it is in transit
        // (the shipped flag's effect, covering step 3.1's remote port).
        // Steps 3-4 (the dataHolder's [[RemotePort]]) run in the caller,
        // which reads the record's entanglement before this function
        // removes it.
        let (queue, in_flight) = {
            let mut ports = self.ports.borrow_mut(ec);
            let Some(index) = ports.iter().position(|record| record.port_id == port_id) else {
                return Err(format!("transfer: unknown port {port_id}"));
            };
            if ports[index].detached {
                return Err(String::from("transfer: port is detached"));
            }
            if !matches!(
                ports[index].ts,
                TransferState::Managed | TransferState::CompletionInProgress
            ) {
                return Err(format!("transfer: port {port_id} is already in transit"));
            }
            let queue: Vec<PortMessagePayload> = ports[index].queue.drain(..).collect();
            let in_flight = ports[index].in_flight;
            ports.remove(index);
            (queue, in_flight)
        };
        self.trace(
            "Transfer",
            vec![port_id.to_string(), self.event_loop_id.to_string()],
        );
        event_sender
            .send(ContentEvent::PortTransferStarted { port: port_id })
            .map_err(|error| {
                format!("failed to notify the user agent of port transfer: {error}")
            })?;
        Ok((queue, in_flight))
    }

    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    pub(crate) fn post_message(
        &self,
        src_id: PortId,
        target_id: Option<PortId>,
        msg: PortMessagePayload,
        event_sender: &IpcSender<ContentEvent>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<(), String> {
        // Step 6: If targetPort is null, or if doomed is true, then return.
        let Some(target_id) = target_id else {
            return Ok(());
        };
        let (direct, target_index) = {
            let ports = self.ports.borrow_mut(ec);
            let Some(src_index) = ports.iter().position(|record| record.port_id == src_id) else {
                // The source port is detached (transferred or closed); its
                // entanglement was severed, so targetPort is null.
                return Ok(());
            };
            if ports[src_index].entangled != Some(target_id) {
                return Ok(());
            }
            let target_index = ports.iter().position(|record| record.port_id == target_id);
            // The port message queue is FIFO, so the message may be delivered
            // directly only when nothing older is still in flight toward the
            // target (routed messages that have not landed in its queue yet);
            // otherwise it is appended after them in the routing queue.
            let direct = ports[src_index].ts == TransferState::Managed
                && target_index.is_some_and(|index| {
                    ports[index].ts == TransferState::Managed && ports[index].in_flight == 0
                });
            (direct, target_index)
        };
        // Step 7: Add a task that runs the following steps to the port
        //         message queue of targetPort.
        // Note: The source is managed by this event loop whenever its record
        // is held here, so the action is recorded for both direct and
        // routed delivery (`MessagePort.tla`'s `PostMessage`).
        self.trace(
            "PostMessage",
            vec![
                src_id.to_string(),
                self.event_loop_id.to_string(),
                msg.message_id.to_string(),
            ],
        );
        if direct {
            let mut ports = self.ports.borrow_mut(ec);
            let Some(target_index) = ports.iter().position(|record| record.port_id == target_id)
            else {
                return Ok(());
            };
            ports[target_index].queue.push_back(msg);
            drop(ports);
            self.request_message_tasks(target_id, ec)?;
        } else {
            // `MessagePortExtraFG.tla`'s `PostMessage` routed branch: append a "Single"
            // item to the user agent's routing queue.
            if let Some(target_index) = target_index {
                let mut ports = self.ports.borrow_mut(ec);
                if let Some(record) = ports.get_mut(target_index) {
                    record.in_flight = record.in_flight.saturating_add(1);
                }
            }
            event_sender
                .send(ContentEvent::PortMessageRouted {
                    tgt: target_id,
                    msg,
                })
                .map_err(|error| format!("failed to route port message: {error}"))?;
        }
        Ok(())
    }

    /// <https://html.spec.whatwg.org/#dom-messageport-start>
    pub(crate) fn start(&self, port_id: PortId, ec: &mut dyn ExecutionContext<Types>) {
        // Step 1: The start() method steps are to enable this's port
        //         message queue, if it is not already enabled.
        // Note: Enabling the queue makes the event loop use it as a task
        // source; here that also requests message tasks for any pending
        // messages.
        let was_enabled = {
            let mut ports = self.ports.borrow_mut(ec);
            let Some(index) = ports.iter().position(|record| record.port_id == port_id) else {
                return;
            };
            let was_enabled = ports[index].enabled;
            ports[index].enabled = true;
            was_enabled
        };
        if !was_enabled && let Err(error) = self.request_message_tasks(port_id, ec) {
            warn!("failed to request port message tasks after start: {error}");
        }
    }

    /// <https://html.spec.whatwg.org/#dom-messageport-close>
    pub(crate) fn close(
        &self,
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<PortId> {
        let mut ports = self.ports.borrow_mut(ec);
        let index = ports.iter().position(|record| record.port_id == port_id)?;
        // Step 1: Set this's [[Detached]] internal slot value to true.
        ports[index].detached = true;
        // Step 2: If this is entangled, disentangle it.
        // Note: The disentangle steps run here: step 1 (let otherPort be
        // the port this was entangled with) is the `other` returned below,
        // step 3 (disentangle the pair) clears both records, and step 4
        // (fire an event named close at otherPort) runs in the caller.
        let other = ports[index].entangled.take();
        if let Some(other) = other
            && let Some(other_index) = ports.iter().position(|record| record.port_id == other)
        {
            ports[other_index].entangled = None;
        }
        other
    }

    /// Enable a port's message queue (once enabled it stays enabled) and
    /// request message tasks for its pending messages.
    pub(crate) fn enable_queue(&self, port_id: PortId, ec: &mut dyn ExecutionContext<Types>) {
        let was_enabled = {
            let mut ports = self.ports.borrow_mut(ec);
            let Some(index) = ports.iter().position(|record| record.port_id == port_id) else {
                return;
            };
            let was_enabled = ports[index].enabled;
            ports[index].enabled = true;
            was_enabled
        };
        if !was_enabled && let Err(error) = self.request_message_tasks(port_id, ec) {
            warn!("failed to request port message tasks after enabling: {error}");
        }
    }

    /// Handle a port task queued by the user agent: deliver a routed
    /// message to the port's queue or land a transfer buffer, returning
    /// whether a message task should fire.  When the port is no longer
    /// managed by this event loop the task is left for the caller to
    /// return to the routing queue.
    pub(crate) fn handle_port_task(
        &self,
        port_id: PortId,
        task: PortTaskKind,
        event_sender: &IpcSender<ContentEvent>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<bool, String> {
        match task {
            PortTaskKind::NewTask { msg } => {
                let local = {
                    let ports = self.ports.borrow_mut(ec);
                    let Some(index) = ports.iter().position(|record| record.port_id == port_id)
                    else {
                        // The port was transferred away (or closed) before the
                        // routed task ran; the task is returned to the routing
                        // queue by the caller.
                        return Ok(false);
                    };
                    ports[index].is_local()
                };
                if local {
                    let mut ports = self.ports.borrow_mut(ec);
                    let Some(index) = ports.iter().position(|record| record.port_id == port_id)
                    else {
                        return Ok(false);
                    };
                    ports[index].queue.push_back(msg);
                    ports[index].in_flight = ports[index].in_flight.saturating_sub(1);
                    let fire = ports[index].enabled && !ports[index].queue.is_empty();
                    drop(ports);
                    self.trace(
                        "RunTask",
                        vec![
                            self.event_loop_id.to_string(),
                            port_id.to_string(),
                            String::from("NewTask"),
                        ],
                    );
                    Ok(fire)
                } else {
                    self.trace(
                        "RunTask",
                        vec![
                            self.event_loop_id.to_string(),
                            port_id.to_string(),
                            String::from("NewTask"),
                        ],
                    );
                    event_sender
                        .send(ContentEvent::PortMessageRouted { tgt: port_id, msg })
                        .map_err(|error| format!("failed to return port message: {error}"))?;
                    Ok(false)
                }
            }
            PortTaskKind::Buffer { buf } => {
                let completed = {
                    let mut ports = self.ports.borrow_mut(ec);
                    let Some(index) = ports.iter().position(|record| record.port_id == port_id)
                    else {
                        // The port was transferred away before the completion
                        // task ran; the buffer is returned to the routing queue.
                        return Ok(false);
                    };
                    // Mirror the user agent's `RouteMessage` transition for a
                    // "ReturnedBuffer" item against a `CompletionRequested`
                    // port: the transfer completion now proceeds.
                    if ports[index].ts == TransferState::CompletionRequested {
                        ports[index].ts = TransferState::CompletionInProgress;
                    }
                    ports[index].is_local()
                };
                if completed {
                    let mut ports = self.ports.borrow_mut(ec);
                    let Some(index) = ports.iter().position(|record| record.port_id == port_id)
                    else {
                        return Ok(false);
                    };
                    let landed = buf.len() as u32;
                    for msg in buf {
                        ports[index].queue.push_back(msg);
                    }
                    ports[index].in_flight = ports[index].in_flight.saturating_sub(landed);
                    ports[index].ts = TransferState::Managed;
                    let fire = ports[index].enabled && !ports[index].queue.is_empty();
                    drop(ports);
                    self.trace(
                        "RunTask",
                        vec![
                            self.event_loop_id.to_string(),
                            port_id.to_string(),
                            String::from("Buffer"),
                        ],
                    );
                    // `MessagePortExtraFG.tla`'s `RunTask` appends a "Success" routing item
                    // when the completion task runs on the port's owner.  The
                    // user agent completes the transfer; the message tasks for
                    // the moved messages are requested here (or run inline by
                    // the caller).
                    event_sender
                        .send(ContentEvent::PortTransferCompleted { tgt: port_id })
                        .map_err(|error| {
                            format!("failed to notify port transfer completion: {error}")
                        })?;
                    Ok(fire)
                } else {
                    self.trace(
                        "RunTask",
                        vec![
                            self.event_loop_id.to_string(),
                            port_id.to_string(),
                            String::from("Buffer"),
                        ],
                    );
                    event_sender
                        .send(ContentEvent::PortBufferReturned { tgt: port_id, buf })
                        .map_err(|error| format!("failed to return port buffer: {error}"))?;
                    Ok(false)
                }
            }
        }
    }

    /// Return a port task to the user agent's routing queue when the port
    /// is no longer managed by this event loop.
    pub(crate) fn return_task_to_ua(
        &self,
        port_id: PortId,
        task: PortTaskKind,
        event_sender: &IpcSender<ContentEvent>,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<(), String> {
        let _ = ec;
        let (kind, result) = match task {
            PortTaskKind::NewTask { msg } => (
                "NewTask",
                event_sender
                    .send(ContentEvent::PortMessageRouted { tgt: port_id, msg })
                    .map_err(|error| format!("failed to return port message: {error}")),
            ),
            PortTaskKind::Buffer { buf } => (
                "Buffer",
                event_sender
                    .send(ContentEvent::PortBufferReturned { tgt: port_id, buf })
                    .map_err(|error| format!("failed to return port buffer: {error}")),
            ),
        };
        self.trace(
            "RunTask",
            vec![
                self.event_loop_id.to_string(),
                port_id.to_string(),
                String::from(kind),
            ],
        );
        result
    }

    /// <https://html.spec.whatwg.org/#message-port-post-message-steps>
    pub(crate) fn pop_queued_message(
        &self,
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<Option<PortMessagePayload>, String> {
        // Step 7: Add a task that runs the following steps to the port
        //         message queue of targetPort.
        // Note: The task's queue removal runs here: the message at the head
        // of the port's queue is popped (only while the queue is enabled,
        // i.e. while the event loop uses it as a task source) so
        // `run_message_task` can run steps 7.1-7.7 with it.  When the queue
        // still holds messages another message task is requested; each
        // message fires in its own task.
        let popped = {
            let mut ports = self.ports.borrow_mut(ec);
            let Some(index) = ports.iter().position(|record| record.port_id == port_id) else {
                return Ok(None);
            };
            if !ports[index].enabled {
                return Ok(None);
            }
            ports[index].queue.pop_front()
        };
        if let Some(payload) = &popped {
            // `MessagePort.tla`'s `ReceiveMessage` action: the message task ran and
            // the queue popped.  The message id is recorded so the trace
            // consumer can check the pop against the abstract queue head.
            self.trace(
                "ReceiveMessage",
                vec![
                    port_id.to_string(),
                    self.event_loop_id.to_string(),
                    payload.message_id.to_string(),
                ],
            );
            // The queue may hold further messages; each fires in its own task.
            self.request_message_tasks(port_id, ec)?;
        }
        Ok(popped)
    }

    /// The record of a port managed by this event loop, if any.
    pub(crate) fn port_record(
        &self,
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<PortRecord> {
        self.ports
            .borrow(ec)
            .iter()
            .find(|record| record.port_id == port_id)
            .cloned()
    }

    /// The platform object of a port managed by this event loop, if any.
    pub(crate) fn port_object(
        &self,
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Option<MessagePort> {
        self.ports
            .borrow(ec)
            .iter()
            .find(|record| record.port_id == port_id)
            .and_then(|record| record.object.clone())
    }

    /// Whether a port is managed by this event loop.
    pub(crate) fn has_port(&self, port_id: PortId, ec: &mut dyn ExecutionContext<Types>) -> bool {
        self.ports
            .borrow(ec)
            .iter()
            .any(|record| record.port_id == port_id)
    }

    /// <https://html.spec.whatwg.org/#port-message-queue>
    fn request_message_tasks(
        &self,
        port_id: PortId,
        ec: &mut dyn ExecutionContext<Types>,
    ) -> Result<(), String> {
        let pending = {
            let ports = self.ports.borrow(ec);
            ports
                .iter()
                .find(|record| record.port_id == port_id)
                .map(|record| record.enabled && !record.queue.is_empty())
                .unwrap_or(false)
        };
        if !pending {
            return Ok(());
        }
        self.task_queue
            .queue_a_task(Task::RunPortMessage { port: port_id });
        Ok(())
    }
}
