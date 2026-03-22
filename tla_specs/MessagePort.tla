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

VARIABLES ports

----------------------------------------------------------------------------
TypeInvariant ==
    ports \subseteq [
        id            : PortId,
        entangled_with : PortId \cup {NoPortId},
        event_loop    : EventLoopId \cup {NoEventLoopId},
        queue         : Seq(MessageId)
    ]

----------------------------------------------------------------------------
\* Replace one port record in the set with an updated version
UpdatePort(old, new) == (ports \ {old}) \cup {new}

----------------------------------------------------------------------------
Init == ports = {}

----------------------------------------------------------------------------

\* https://html.spec.whatwg.org/multipage/l#message-port-post-message-steps
NewMessageChannel(id1, id2, el) ==
    /\ id1 /= id2
    /\ el \in EventLoopId
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

\* https://html.spec.whatwg.org/multipage/#message-ports:transfer-steps
Transfer(id, el) ==
    \E p \in ports :
        /\ p.id = id
        /\ p.event_loop = el
        /\ ports' = UpdatePort(p, [p EXCEPT !.event_loop = NoEventLoopId])

\* https://html.spec.whatwg.org/multipage/#message-ports:transfer-receiving-steps
TransferReceive(id, el) ==
    /\ el \in EventLoopId
    /\ \E p \in ports :
        /\ p.id = id
        /\ p.event_loop = NoEventLoopId
        /\ ports' = UpdatePort(p, [p EXCEPT !.event_loop = el])


\* https://html.spec.whatwg.org/multipage/l#message-port-post-message-steps
PostMessage(src_id, el, msg) ==
    \E src, tgt \in ports :
        /\ src.id           = src_id
        /\ src.event_loop   = el
        /\ tgt.id           = src.entangled_with
        /\ SelectSeq(tgt.queue, LAMBDA e : e = msg) = <<>>
        /\ ports' = UpdatePort(tgt, [tgt EXCEPT !.queue = Append(@, msg)])

\* https://html.spec.whatwg.org/multipage/l#message-port-post-message-steps
\* The part inside `Add a task that runs the following steps`.
ReceiveMessage(port_id) ==
    \E p \in ports :
        /\ p.id = port_id
        /\ p.queue /= <<>> 
        /\ p.event_loop /= NoEventLoopId 
        /\ ports' = UpdatePort(p, [p EXCEPT !.queue = Tail(@)])

----------------------------------------------------------------------------
Next ==
    \/ \E id1, id2 \in PortId, el \in EventLoopId :
           NewMessageChannel(id1, id2, el)
    \/ \E id \in PortId, el \in EventLoopId :
           \/ TransferReceive(id, el)
           \/ Transfer(id, el)
    \/ \E pid \in PortId, mid \in MessageId, el \in EventLoopId : 
           PostMessage(pid, el, mid)
    \/ \E pid \in PortId :
           ReceiveMessage(pid)

Spec == Init /\ [][Next]_ports

============================================================================
