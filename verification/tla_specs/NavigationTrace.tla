------------------------- MODULE NavigationTrace -------------------------
EXTENDS Naturals, Sequences, TLC, NavigationTraceData

NONE == "<<NONE>>"

VARIABLES
    navigables,
    navigations,
    freeNavIDs,
    freeNkIDs,
    navigationStartQueue,
    trace_index

vars == <<navigables, navigations, freeNavIDs, freeNkIDs, navigationStartQueue, trace_index>>

Base == INSTANCE Navigation WITH
    NavIDs <- TraceNavIDs,
    NkIDs <- TraceNkIDs,
    NONE <- NONE,
    navigables <- navigables,
    navigations <- navigations,
    freeNavIDs <- freeNavIDs,
    freeNkIDs <- freeNkIDs,
    navigationStartQueue <- navigationStartQueue

TraceLength == Len(Trace)

CurrentEntry ==
    IF trace_index \in 1..TraceLength
    THEN Trace[trace_index]
    ELSE [event |-> "", event_args |-> <<>>]

CurrentEvent == CurrentEntry.event
CurrentArgs == CurrentEntry.event_args

EventArg(position) == CurrentArgs[position]

Advance == trace_index' = trace_index + 1

Init ==
    /\ Base!Init
    /\ trace_index = 1

CreateNavigableTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "CreateNavigable"
    /\ Len(CurrentArgs) = 1
    /\ LET navID == EventArg(1)
       IN
       /\ Base!CreateNavigable
       /\ navID \notin DOMAIN navigables
       /\ navID \in DOMAIN navigables'
       /\ Advance

CreateChildNavigableTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "CreateChildNavigable"
    /\ Len(CurrentArgs) = 2
    /\ LET navID == EventArg(1)
           parentID == EventArg(2)
       IN
       /\ Base!CreateChildNavigable(parentID)
       /\ navID \notin DOMAIN navigables
       /\ navID \in DOMAIN navigables'
       /\ Advance

CreateNavigationTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "CreateNavigation"
    /\ Len(CurrentArgs) = 2
    /\ LET nk == EventArg(1)
           navID == EventArg(2)
       IN
       /\ Base!CreateNavigation(navID)
       /\ nk \notin DOMAIN navigations
       /\ nk \in DOMAIN navigations'
       /\ Advance

StartNavigatingTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "StartNavigating"
    /\ Len(CurrentArgs) = 1
    /\ LET nk == EventArg(1)
       IN
       /\ Base!StartNavigating(nk)
       /\ Advance

RunBeforeUnloadTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "RunBeforeUnload"
    /\ Len(CurrentArgs) = 3
    /\ LET navID == EventArg(1)
           nk == EventArg(2)
           outcome == EventArg(3)
       IN
       /\ Base!RunBeforeUnload(navID, nk, outcome)
       /\ Advance

ContinueNavigationTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "ContinueNavigation"
    /\ Len(CurrentArgs) = 2
    /\ LET nk == EventArg(1)
           status == EventArg(2)
       IN
       /\ Base!ContinueNavigation(nk)
       /\ navigations'[nk].status = status
       /\ Advance

Done ==
    /\ trace_index > TraceLength
    /\ UNCHANGED vars

Next ==
    \/ CreateNavigableTrace
    \/ CreateChildNavigableTrace
    \/ CreateNavigationTrace
    \/ StartNavigatingTrace
    \/ RunBeforeUnloadTrace
    \/ ContinueNavigationTrace
    \/ Done

TypeOK == Base!TypeOK

FinalizedImpliesAllApproved == Base!FinalizedImpliesAllApproved

TraceAccepted == trace_index = TraceLength + 1

=============================================================================