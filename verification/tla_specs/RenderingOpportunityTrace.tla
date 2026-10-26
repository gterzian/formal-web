------------------------- MODULE RenderingOpportunityTrace -------------------------
EXTENDS Naturals, Sequences, TLC, RenderingOpportunityTraceData

VARIABLES
    live,
    parent,
    pending,
    rendering_updated,
    composed,
    ui_state_update_count,
    animating,
    rendering_opportunity,
    trace_index

vars == <<live, parent, pending, rendering_updated, composed,
          ui_state_update_count, animating, rendering_opportunity, trace_index>>

Base == INSTANCE RenderingOpportunity WITH
    Frame <- TraceFrame,
    NoParent <- TraceNoParent,
    BufferCount <- TraceBufferCount,
    live <- live,
    parent <- parent,
    pending <- pending,
    rendering_updated <- rendering_updated,
    composed <- composed,
    ui_state_update_count <- ui_state_update_count,
    animating <- animating,
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

\* A graphics composition. Normally the top-level content rendered first
\* (GraphicsComputed); when the render cycle's deadline expired first, the
\* composition is the Deadline action instead. The two are distinguished by
\* their guards, not by the event name (both are traced as GraphicsComputed).
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
    /\ CurrentEvent = "GraphicsComputed"
    /\ Len(CurrentArgs) = 1
    /\ LET f == EventArg(1)
       IN
       /\ f \in live
       /\ Base!Deadline(f)
       /\ Advance

Done ==
    /\ trace_index > TraceLength
    /\ UNCHANGED vars

Next ==
    \/ CreateFrameTrace
    \/ NoteUiStateUpdateTrace
    \/ RenderingOpportunityTrace
    \/ UpdateTheRenderingTrace
    \/ GraphicsComputedTrace
    \/ DeadlineTrace
    \/ Done

TypeOK == Base!TypeOK

TraceAccepted == trace_index = TraceLength + 1
=============================================================================
