--------------------------- MODULE MessagePortExtraFG ---------------------------
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

VARIABLES
    port_state,
    routing_queue,
    el_tasks

vars == <<port_state, routing_queue, el_tasks>>

RQItemOK(item) ==
    \/ /\ item.kind = "Single"
       /\ item.tgt  \in PortId
       /\ item.msg  \in MessageId
    \/ /\ item.kind = "ReturnedBuffer"
       /\ item.tgt  \in PortId
       /\ item.buf  \in Seq(MessageId)
    \/ /\ item.kind = "Success"
       /\ item.tgt  \in PortId

ELTaskOK(task) ==
    \/ /\ task.kind = "NewTask"
       /\ task.port \in PortId
       /\ task.msg  \in MessageId
    \/ /\ task.kind = "Buffer"
       /\ task.port \in PortId
       /\ task.buf  \in Seq(MessageId)

TypeInvariant ==
    /\ DOMAIN port_state \subseteq PortId
    /\ \A id \in DOMAIN port_state :
        LET p == port_state[id] IN
        /\ p.ts        \in {TS_Managed, TS_TransferInProgress,
                             TS_CompletionInProgress,
                             TS_CompletionFailed, TS_CompletionRequested}
        /\ p.owner     \in EventLoopId \cup {NoEventLoopId}
        /\ p.entangled \in PortId      \cup {NoPortId}
        /\ p.queue     \in Seq(MessageId)
        /\ p.buf       \in Seq(MessageId)
    /\ \A i \in DOMAIN routing_queue : RQItemOK(routing_queue[i])
    /\ DOMAIN el_tasks = EventLoopId
    /\ \A el \in EventLoopId :
       \A i \in DOMAIN el_tasks[el] : ELTaskOK(el_tasks[el][i])

Init ==
    /\ port_state    = [id \in {} |-> 0]
    /\ routing_queue = <<>>
    /\ el_tasks      = [el \in EventLoopId |-> <<>>]

WithPorts(id1, rec1, id2, rec2) ==
    [x \in DOMAIN port_state \cup {id1, id2} |->
        IF x = id1 THEN rec1
        ELSE IF x = id2 THEN rec2
        ELSE port_state[x]]

MsgInFlight(msg, tgt_id) ==
    \/ \E i \in DOMAIN port_state[tgt_id].queue :
           port_state[tgt_id].queue[i] = msg
    \/ \E i \in DOMAIN port_state[tgt_id].buf :
           port_state[tgt_id].buf[i] = msg
    \/ \E i \in DOMAIN routing_queue :
           /\ routing_queue[i].tgt = tgt_id
           /\ \/ /\ routing_queue[i].kind = "Single"
                 /\ routing_queue[i].msg = msg
              \/ /\ routing_queue[i].kind = "ReturnedBuffer"
                 /\ \E j \in DOMAIN routing_queue[i].buf :
                        routing_queue[i].buf[j] = msg
    \/ \E el \in EventLoopId :
       \E i \in DOMAIN el_tasks[el] :
           /\ el_tasks[el][i].port = tgt_id
           /\ \/ /\ el_tasks[el][i].kind = "NewTask"
                 /\ el_tasks[el][i].msg = msg
              \/ /\ el_tasks[el][i].kind = "Buffer"
                 /\ \E j \in DOMAIN el_tasks[el][i].buf :
                        el_tasks[el][i].buf[j] = msg

\* Direct delivery to queue is only valid when no older Singles or NewTasks
\* are pending for this target. This matches Servo's sequential event loop
\* semantics: all prior routing_queue tasks are drained before the next
\* same-EL PostMessage can fire.
NoPendingMsgs(tgt_id) ==
    /\ ~(\E i \in DOMAIN routing_queue :
            routing_queue[i].kind = "Single" /\
            routing_queue[i].tgt  = tgt_id)
    /\ ~(\E el \in EventLoopId :
         \E i \in DOMAIN el_tasks[el] :
            el_tasks[el][i].port = tgt_id)

NewChannel(id1, id2, el) ==
    /\ id1 \in PortId /\ id2 \in PortId /\ id1 /= id2
    /\ el  \in EventLoopId
    /\ id1 \notin DOMAIN port_state
    /\ id2 \notin DOMAIN port_state
    /\ port_state' =
         WithPorts(
           id1, [ts |-> TS_Managed, owner |-> el, entangled |-> id2,
                 queue |-> <<>>, buf |-> <<>>],
           id2, [ts |-> TS_Managed, owner |-> el, entangled |-> id1,
                 queue |-> <<>>, buf |-> <<>>])
    /\ UNCHANGED <<routing_queue, el_tasks>>

PostMessage(src_id, el, msg) ==
    /\ src_id \in DOMAIN port_state
    /\ port_state[src_id].ts \in {TS_Managed}
    /\ port_state[src_id].owner = el
    /\ LET tgt_id == port_state[src_id].entangled IN
       /\ tgt_id \in DOMAIN port_state
       /\ ~MsgInFlight(msg, tgt_id)
       /\ IF /\ port_state[tgt_id].owner = el
          /\ NoPendingMsgs(tgt_id)
          /\ port_state[tgt_id].ts \in {TS_Managed}
          THEN /\ port_state' = [port_state EXCEPT
                      ![tgt_id].queue = Append(@, msg)]
               /\ UNCHANGED <<routing_queue, el_tasks>>
          ELSE /\ routing_queue' = Append(routing_queue,
                      [kind |-> "Single", tgt |-> tgt_id, msg |-> msg])
               /\ UNCHANGED <<port_state, el_tasks>>

ReceiveMessage(port_id, el) ==
    /\ port_id \in DOMAIN port_state
    /\ port_state[port_id].ts = TS_Managed
    /\ port_state[port_id].owner = el
    /\ port_state[port_id].queue /= <<>>
    /\ port_state' = [port_state EXCEPT
           ![port_id].queue = Tail(@)]
    /\ UNCHANGED <<routing_queue, el_tasks>>

Transfer(id, el) ==
    /\ id \in DOMAIN port_state
    /\ port_state[id].owner = el
    /\ ~(\E i \in DOMAIN el_tasks[el] :
            el_tasks[el][i].port = id)
    /\ \/ /\ port_state[id].ts = TS_Managed
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_TransferInProgress,
                 ![id].owner = NoEventLoopId,
                 ![id].buf   = <<>>]
          /\ UNCHANGED <<routing_queue, el_tasks>>
       \/ /\ port_state[id].ts = TS_CompletionInProgress
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionFailed,
                 ![id].owner = NoEventLoopId]
          /\ UNCHANGED <<el_tasks, routing_queue>>

TransferReceive(id, el) ==
    /\ id \in DOMAIN port_state
    /\ el \in EventLoopId
    /\ \/ /\ port_state[id].ts = TS_TransferInProgress
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionInProgress,
                 ![id].owner = el,
                 ![id].buf   = <<>>]
          /\ el_tasks' = [el_tasks EXCEPT
                 ![el] = Append(@, [kind |-> "Buffer", port |-> id,
                                    buf  |-> port_state[id].buf])]
          /\ UNCHANGED routing_queue
       \/ /\ port_state[id].ts \in {TS_CompletionFailed, TS_CompletionRequested}
          /\ port_state' = [port_state EXCEPT
                 ![id].ts    = TS_CompletionRequested,
                 ![id].owner = el]
          /\ UNCHANGED <<routing_queue, el_tasks>>

RouteMessage ==
    /\ routing_queue /= <<>>
    /\ LET item   == Head(routing_queue)
           tgt_id == item.tgt IN
       /\ routing_queue' = Tail(routing_queue)
       /\ tgt_id \in DOMAIN port_state
       /\ LET p == port_state[tgt_id] IN
          IF item.kind = "Success" /\ p.ts = TS_CompletionInProgress
          THEN /\ port_state' = [port_state EXCEPT
                      ![tgt_id].ts = TS_Managed]
               /\ UNCHANGED el_tasks
          ELSE IF p.ts \in {TS_Managed, TS_CompletionInProgress}
               /\ item.kind = "Single"
          THEN /\ el_tasks' = [el_tasks EXCEPT
                      ![p.owner] = Append(@,
                          [kind |-> "NewTask", port |-> tgt_id, msg |-> item.msg])]
               /\ UNCHANGED port_state
          ELSE IF p.ts = TS_CompletionFailed /\ item.kind = "ReturnedBuffer"
          THEN /\ port_state' = [port_state EXCEPT
                      ![tgt_id].ts  = TS_TransferInProgress,
                      ![tgt_id].buf = item.buf \o p.buf]
               /\ UNCHANGED el_tasks
          ELSE IF p.ts = TS_CompletionRequested /\ item.kind = "ReturnedBuffer"
          THEN /\ port_state' = [port_state EXCEPT
                      ![tgt_id].ts  = TS_CompletionInProgress,
                      ![tgt_id].buf = <<>>]
               /\ el_tasks' = [el_tasks EXCEPT
                      ![p.owner] = Append(@,
                          [kind |-> "Buffer", port |-> tgt_id,
                           buf  |-> item.buf \o p.buf])]
          ELSE
               /\ p.ts \in {TS_TransferInProgress, TS_CompletionFailed,
                             TS_CompletionRequested}
               /\ item.kind = "Single"
               /\ port_state' = [port_state EXCEPT
                      ![tgt_id].buf = Append(p.buf, item.msg)]
               /\ UNCHANGED el_tasks

RunTask(el) ==
    /\ el_tasks[el] /= <<>>
    /\ LET task    == Head(el_tasks[el])
           port_id == task.port IN
       /\ el_tasks' = [el_tasks EXCEPT ![el] = Tail(@)]
       /\ port_id \in DOMAIN port_state
       /\ IF port_state[port_id].owner = el
          THEN IF task.kind = "Buffer"
               THEN /\ port_state' = [port_state EXCEPT
                           ![port_id].queue = @ \o task.buf]
                    /\ routing_queue' = Append(routing_queue,
                           [kind |-> "Success", tgt |-> port_id])
               ELSE /\ port_state' = [port_state EXCEPT
                           ![port_id].queue = Append(@, task.msg)]
                    /\ UNCHANGED routing_queue
          ELSE /\ routing_queue' = Append(routing_queue,
                      IF task.kind = "NewTask"
                      THEN [kind |-> "Single",         tgt |-> port_id, msg |-> task.msg]
                      ELSE [kind |-> "ReturnedBuffer", tgt |-> port_id, buf |-> task.buf])
               /\ UNCHANGED port_state

Next ==
    \/ \E id1, id2 \in PortId, el \in EventLoopId : NewChannel(id1, id2, el)
    \/ \E pid \in PortId, mid \in MessageId, el \in EventLoopId : PostMessage(pid, el, mid)
    \/ \E pid \in PortId, el \in EventLoopId : ReceiveMessage(pid, el)
    \/ \E id  \in PortId, el \in EventLoopId : Transfer(id, el)
    \/ \E id  \in PortId, el \in EventLoopId : TransferReceive(id, el)
    \/ RouteMessage
    \/ \E el \in EventLoopId : RunTask(el)

Spec == Init /\ [][Next]_vars

RECURSIVE ExtractRQReturnedBufs(_, _)
ExtractRQReturnedBufs(seq, id) ==
    IF seq = <<>> THEN <<>>
    ELSE LET h == Head(seq) IN
         IF h.tgt = id /\ h.kind = "ReturnedBuffer"
         THEN h.buf \o ExtractRQReturnedBufs(Tail(seq), id)
         ELSE ExtractRQReturnedBufs(Tail(seq), id)

RECURSIVE ExtractELBufs(_, _)
ExtractELBufs(seq, id) ==
    IF seq = <<>> THEN <<>>
    ELSE LET h == Head(seq) IN
         IF h.port = id /\ h.kind = "Buffer"
         THEN h.buf \o ExtractELBufs(Tail(seq), id)
         ELSE ExtractELBufs(Tail(seq), id)

RECURSIVE ExtractELSingles(_, _)
ExtractELSingles(seq, id) ==
    IF seq = <<>> THEN <<>>
    ELSE LET h == Head(seq) IN
         IF h.port = id /\ h.kind = "NewTask"
         THEN <<h.msg>> \o ExtractELSingles(Tail(seq), id)
         ELSE ExtractELSingles(Tail(seq), id)

RECURSIVE ExtractRQSingles(_, _)
ExtractRQSingles(seq, id) ==
    IF seq = <<>> THEN <<>>
    ELSE LET h == Head(seq) IN
         IF h.tgt = id /\ h.kind = "Single"
         THEN <<h.msg>> \o ExtractRQSingles(Tail(seq), id)
         ELSE ExtractRQSingles(Tail(seq), id)

RECURSIVE AllELBufs(_, _)
AllELBufs(els, id) ==
    IF els = {} THEN <<>>
    ELSE LET el == CHOOSE e \in els : TRUE IN
         ExtractELBufs(el_tasks[el], id) \o AllELBufs(els \ {el}, id)

RECURSIVE AllELSingles(_, _)
AllELSingles(els, id) ==
    IF els = {} THEN <<>>
    ELSE LET el == CHOOSE e \in els : TRUE IN
         ExtractELSingles(el_tasks[el], id) \o AllELSingles(els \ {el}, id)

AbstractQueue(id) ==
    LET p == port_state[id] IN
    IF p.ts = TS_Managed
    THEN p.queue
         \o AllELSingles(EventLoopId, id)
         \o ExtractRQSingles(routing_queue, id)
    ELSE IF p.ts = TS_CompletionInProgress
    THEN p.queue
         \o AllELBufs(EventLoopId, id)
         \o AllELSingles(EventLoopId, id)
         \o ExtractRQSingles(routing_queue, id)
    ELSE p.queue
         \o AllELBufs(EventLoopId, id)
         \o AllELSingles(EventLoopId, id)
         \o ExtractRQReturnedBufs(routing_queue, id)
         \o p.buf
         \o ExtractRQSingles(routing_queue, id)

DebugAlias ==
    [port_states     |-> port_state,
     routing_queue   |-> routing_queue,
     el_tasks        |-> el_tasks,
     abstract_queues |-> [id \in DOMAIN port_state |-> AbstractQueue(id)]]

AbstractPorts ==
    { [id             |-> id,
       entangled_with |-> port_state[id].entangled,
       event_loop     |->
           IF port_state[id].ts \in {TS_Managed, TS_CompletionInProgress}
           THEN port_state[id].owner
           ELSE NoEventLoopId,
       queue          |-> AbstractQueue(id)]
      : id \in DOMAIN port_state }

AP == INSTANCE MessagePort WITH ports <- AbstractPorts

AbstractTypeInvariant == AP!TypeInvariant

RefinementProperty == AP!Spec

THEOREM Spec => AP!Spec

================================================================================