--------------------------- MODULE MessagePortFG ---------------------------
EXTENDS Sequences, Integers

CONSTANTS
    PortId,
    EventLoopId,
    MessageId,
    NoPortId,
    NoEventLoopId

ASSUME NoPortId      \notin PortId
ASSUME NoEventLoopId \notin EventLoopId

TS_Managed              == "Managed"
TS_TransferInProgress   == "TransferInProgress"
TS_CompletionInProgress == "CompletionInProgress"
TS_CompletionFailed     == "CompletionFailed"
TS_CompletionRequested  == "CompletionRequested"

VARIABLE port_state

TypeInvariant ==
    /\ DOMAIN port_state \subseteq PortId
    /\ \A id \in DOMAIN port_state :
        LET p == port_state[id] IN
        /\ p.ts       \in {TS_Managed, TS_TransferInProgress,
                            TS_CompletionInProgress,
                            TS_CompletionFailed, TS_CompletionRequested}
        /\ p.owner    \in EventLoopId \cup {NoEventLoopId}
        /\ p.entangled \in PortId     \cup {NoPortId}
        /\ p.queue    \in Seq(MessageId)

Init == port_state = [id \in {} |-> 0]

WithPorts(id1, rec1, id2, rec2) ==
    [x \in DOMAIN port_state \cup {id1, id2} |->
        IF x = id1 THEN rec1
        ELSE IF x = id2 THEN rec2
        ELSE port_state[x]]

NewChannel(id1, id2, el) ==
    /\ id1 \in PortId /\ id2 \in PortId /\ id1 /= id2
    /\ el  \in EventLoopId
    /\ id1 \notin DOMAIN port_state
    /\ id2 \notin DOMAIN port_state
    /\ port_state' =
         WithPorts(
           id1, [ts |-> TS_Managed, owner |-> el,
                 entangled |-> id2, queue |-> <<>>],
           id2, [ts |-> TS_Managed, owner |-> el,
                 entangled |-> id1, queue |-> <<>>])

PostMessage(src_id, el, msg) ==
    /\ src_id \in DOMAIN port_state
    /\ port_state[src_id].ts \in {TS_Managed, TS_CompletionInProgress}
    /\ port_state[src_id].owner = el
    /\ LET tgt_id == port_state[src_id].entangled IN
       /\ tgt_id \in DOMAIN port_state
       /\ SelectSeq(port_state[tgt_id].queue, LAMBDA e : e = msg) = <<>>
       /\ port_state' = [port_state EXCEPT
              ![tgt_id].queue = Append(@, msg)]

Deliver(port_id, el) ==
    /\ port_id \in DOMAIN port_state
    /\ port_state[port_id].owner = el
    /\ port_state[port_id].ts \in {TS_Managed, TS_CompletionInProgress}
    /\ port_state[port_id].queue /= <<>>
    /\ port_state' = [port_state EXCEPT
           ![port_id].queue = Tail(@)]

Transfer(id, el) ==
    /\ id \in DOMAIN port_state
    /\ port_state[id].owner = el
    /\ \/ /\ port_state[id].ts = TS_Managed
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_TransferInProgress,
                 ![id].owner = NoEventLoopId]
       \/ /\ port_state[id].ts = TS_CompletionInProgress
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionFailed,
                 ![id].owner = NoEventLoopId]

TransferReceive(id, el) ==
    /\ id \in DOMAIN port_state
    /\ el \in EventLoopId
    /\ \/ /\ port_state[id].ts = TS_TransferInProgress
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionInProgress,
                 ![id].owner = el]
       \/ /\ port_state[id].ts \in {TS_CompletionFailed, TS_CompletionRequested}
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionRequested,
                 ![id].owner = el]

AckSuccess(id) ==
    /\ id \in DOMAIN port_state
    /\ port_state[id].ts = TS_CompletionInProgress
    /\ port_state' = [port_state EXCEPT
           ![id].ts = TS_Managed]

ReturnBuffer(id) ==
    /\ id \in DOMAIN port_state
    /\ \/ /\ port_state[id].ts = TS_CompletionFailed
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_TransferInProgress,
                 ![id].owner = NoEventLoopId]
       \/ /\ port_state[id].ts = TS_CompletionRequested
          /\ port_state' = [port_state EXCEPT
                 ![id].ts = TS_CompletionInProgress]
             \* owner already holds the new target EL from TransferReceive.

Next ==
    \/ \E id1, id2 \in PortId, el \in EventLoopId : NewChannel(id1, id2, el)
    \/ \E pid \in PortId, mid \in MessageId, el \in EventLoopId : PostMessage(pid, el, mid)
    \/ \E pid \in PortId, el \in EventLoopId : Deliver(pid, el)
    \/ \E id \in PortId, el \in EventLoopId : Transfer(id, el)
    \/ \E id \in PortId, el \in EventLoopId : TransferReceive(id, el)
    \/ \E id \in PortId : AckSuccess(id)
    \/ \E id \in PortId : ReturnBuffer(id)

Spec == Init /\ [][Next]_port_state

AbstractPorts ==
    { [id             |-> id,
       entangled_with |-> port_state[id].entangled,
       event_loop     |->
           IF port_state[id].ts \in {TS_Managed, TS_CompletionInProgress}
           THEN port_state[id].owner
           ELSE NoEventLoopId,
       queue          |-> port_state[id].queue]
      : id \in DOMAIN port_state }

AP == INSTANCE MessagePort WITH ports <- AbstractPorts

AbstractTypeInvariant == AP!TypeInvariant

RefinementProperty == AP!Spec

THEOREM Spec => AP!Spec

================================================================================