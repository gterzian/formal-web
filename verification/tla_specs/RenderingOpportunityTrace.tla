------------------------- MODULE RenderingOpportunityTrace -------------------------
EXTENDS Naturals, Sequences, TLC, RenderingOpportunityTraceData

VARIABLES
    live,
    parent,
    queued,
    rendering_updated,
    composed,
    dropped,
    pending_update,
    cycle,
    ui_state_update_count,
    rendering_opportunity,
    trace_index

vars == <<live, parent, queued, rendering_updated, composed, dropped, pending_update, cycle,
          ui_state_update_count, rendering_opportunity, trace_index>>

Base == INSTANCE RenderingOpportunity WITH
    Frame <- TraceFrame,
    NoParent <- TraceNoParent,
    BufferCount <- TraceBufferCount,
    CommandDepth <- TraceCommandDepth,
    live <- live,
    parent <- parent,
    queued <- queued,
    rendering_updated <- rendering_updated,
    composed <- composed,
    dropped <- dropped,
    pending_update <- pending_update,
    cycle <- cycle,
    ui_state_update_count <- ui_state_update_count,
    rendering_opportunity <- rendering_opportunity

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

\* A navigable was born (CreateNavigable / CreateChildNavigable in the
\* navigation path, traced here too since the frame is the graphics-side
\* effect of navigation): a top-level frame carries one argument, a child
\* frame carries (child, parent).
CreateFrameTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "CreateFrame"
    /\ LET f == EventArg(1)
           parentArg == IF Len(CurrentArgs) = 2 THEN EventArg(2) ELSE TraceNoParent
       IN
       /\ Base!CreateFrame(f, parentArg)
       /\ Advance

NoteUiStateUpdateTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "NoteUiStateUpdate"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!NoteUiStateUpdate(f)
       /\ Advance

RenderingOpportunityTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "RenderingOpportunity"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!RenderingOpportunity(f)
       /\ Advance

UpdateTheRenderingTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "UpdateTheRendering"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!UpdateTheRendering(f)
       /\ Advance

\* A graphics composition. The composition kind travels in the trace's
\* event name: a cycle whose top-level content frame rendered is
\* GraphicsComputed, the render cycle's deadline expiring first is
\* Deadline, and a video frame presented against the committed root with no
\* content cycle in flight is OutOfBandComputed.
\* Graphics stored the top-level frame for the cycle: the deadline graphics held
\* for it is satisfied. The composition that presents the frame follows.
FrameStoredTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "FrameStored"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!FrameStored(f)
       /\ Advance

GraphicsComputedTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "GraphicsComputed"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!GraphicsComputed(f)
       /\ Advance

DeadlineTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "Deadline"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!Deadline(f)
       /\ Advance

FrameDroppedTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "FrameDropped"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!FrameDropped(f)
       /\ Advance

MissedFrameTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "MissedFrame"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!MissedFrame(f)
       /\ Advance

OutOfBandComputedTrace ==
    /\ trace_index \in 1..TraceLength
    /\ CurrentEvent = "OutOfBandComputed"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!OutOfBandComputed(f)
       /\ Advance

Done ==
    /\ trace_index > TraceLength
    /\ UNCHANGED vars

Next ==
    \/ CreateFrameTrace
    \/ NoteUiStateUpdateTrace
    \/ RenderingOpportunityTrace
    \/ UpdateTheRenderingTrace
    \/ FrameStoredTrace
    \/ GraphicsComputedTrace
    \/ DeadlineTrace
    \/ OutOfBandComputedTrace
    \/ FrameDroppedTrace
    \/ MissedFrameTrace
    \/ Done

TypeOK == Base!TypeOK

TraceAccepted == trace_index = TraceLength + 1
=============================================================================
