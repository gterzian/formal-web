--------------------------- MODULE MessagePort ---------------------------
EXTENDS Sequences, Integers

CONSTANTS
    PortId,        \* Set of port identifiers (model values)
    EventLoopId,   \* Set of event loop identifiers (model values)
    MessageId,     \* Set of message identifiers (model values)
    NoPortId,      \* Sentinel value: "no entangled port"
    NoEventLoopId  \* Sentinel value: "port is currently transferred (in transit)"

ASSUME NoPortId \notin PortId
ASSUME NoEventLoopId \notin EventLoopId

VARIABLES ports  \* Set of port records

----------------------------------------------------------------------------
\* Type invariant

TypeInvariant ==
    ports \subseteq [
        id            : PortId,
        entangled_with : PortId \cup {NoPortId},
        event_loop    : EventLoopId \cup {NoEventLoopId},
        queue         : Seq(MessageId)
    ]

----------------------------------------------------------------------------
\* Helpers

\* Replace one port record in the set with an updated version
UpdatePort(old, new) == (ports \ {old}) \cup {new}

----------------------------------------------------------------------------
\* Initial state

Init == ports = {}

----------------------------------------------------------------------------
\* Actions

\* Create a new MessageChannel: two fresh, entangled ports placed in event_loop
NewMessageChannel(id1, id2, el) ==
    /\ id1 /= id2
    /\ el \in EventLoopId
    \* Both ids must not already exist
    /\ ~\E p \in ports : p.id = id1 \/ p.id = id2
    /\ ports' = ports \cup {
           [ id            |-> id1,
             entangled_with |-> id2,
             event_loop    |-> el,
             queue         |-> <<>> ],
           [ id            |-> id2,
             entangled_with |-> id1,
             event_loop    |-> el,
             queue         |-> <<>> ]
       }

\* Begin transferring a port: detach it from its current event loop
Transfer(id) ==
    \E p \in ports :
        /\ p.id = id
        /\ p.event_loop /= NoEventLoopId   \* Only enabled if currently in an event loop
        /\ ports' = UpdatePort(p, [p EXCEPT !.event_loop = NoEventLoopId])

\* Complete a transfer: attach the port to a new event loop
TransferReceive(id, el) ==
    /\ el \in EventLoopId
    /\ \E p \in ports :
        /\ p.id = id
        /\ p.event_loop = NoEventLoopId    \* Only enabled if currently transferred
        /\ ports' = UpdatePort(p, [p EXCEPT !.event_loop = el])

\* Post a message to a port's queue (no precondition on transfer state)
PostMessage(port_id, message_id) ==
    /\ \E p \in ports : /\ p.id = port_id
                        /\ SelectSeq(p.queue, LAMBDA e : e = message_id) = <<>>
                        /\ ports' = UpdatePort(p, [p EXCEPT !.queue = Append(@, message_id)])

\* Deliver the next queued message; only possible when the port is live in an event loop
ReceiveMessage(port_id) ==
    \E p \in ports :
        /\ p.id = port_id
        /\ p.queue /= <<>>                 \* Non-empty queue
        /\ p.event_loop /= NoEventLoopId   \* Port must be in an event loop
        /\ ports' = UpdatePort(p, [p EXCEPT !.queue = Tail(@)])

----------------------------------------------------------------------------
\* Spec

Next ==
    \/ \E id1, id2 \in PortId, el \in EventLoopId :
           NewMessageChannel(id1, id2, el)
    \/ \E id \in PortId :
           Transfer(id)
    \/ \E id \in PortId, el \in EventLoopId :
           TransferReceive(id, el)
    \/ \E pid \in PortId, mid \in MessageId :
           PostMessage(pid, mid)
    \/ \E pid \in PortId :
           ReceiveMessage(pid)

Spec == Init /\ [][Next]_ports

============================================================================
