---------------------------- MODULE RenderingOpportunity ----------------------------
EXTENDS Naturals, FiniteSets, TLC

(* Validates the FrameNeeded-gated render cycle and its double-buffer bound.
   Frames are created dynamically by the CreateFrame action: the initial
   top-level traversable is created with parent NoParent, and child frames
   (iframes) are created at any time with their parent. The embedder paints
   top-level traversables only, so FrameNeeded and GraphicsComputed are
   traced for top-level frames, while NoteRenderingOpportunity and
   UpdateTheRendering are traced for every frame. A top-level paint
   services the batched opportunities of its whole hierarchy, and a child's
   update starts the top-level's update too (their frames compose
   together), mirroring the user-agent scheduling.

   The per-frame state (parent, pending, ...) is a function over the whole
   constant Frame universe, with default values for frames not yet created;
   the live set tracks which frames exist. This keeps every expression
   total (TLC errors on applying a function outside its domain), so the
   temporal formulas can quantify over Frame without special-casing. *)

CONSTANTS
  Frame,       \* the universe of frame ids the trace can reference
  NoParent,    \* no parent (top-level frame)
  BufferCount

ASSUME
  /\ Frame # {}
  /\ NoParent \notin Frame
  /\ BufferCount > 0

VARIABLES
  live,              \* created frames (the navigable hierarchy grows)
  parent,            \* parent navigable of f (NoParent at the top level)
  pending,           \* queued update the rendering for f (UA pending_update_the_rendering)
  rendering_updated, \* content completed the update (UpdateTheRendering)
  composed,          \* graphics sent the pixels (PixelFrameReady)
  op_count,          \* a batched rendering opportunity (UA queued_rendering_opportunities, 0 or 1)
  animating,         \* the last composed frame had animated content
  frame_needed       \* the embedder needs a frame (FrameNeeded, sent at each paint)

vars == <<live, parent, pending, rendering_updated, composed, op_count,
          animating, frame_needed>>

\* ---- Helper operators ----

RECURSIVE Ancestors(_)
Ancestors(f) == {f} \cup IF parent[f] = NoParent THEN {} ELSE Ancestors(parent[f])

TopLevel(f) == CHOOSE t \in Ancestors(f): parent[t] = NoParent

IsTopLevel(f) == parent[f] = NoParent

TopLevelFrames(top) == {f \in live: TopLevel(f) = top}

\* ---- Type correctness ----

\* Counters are relative to the last paint; the pipeline depth is bounded
\* by BufferCount. Non-live frames keep default values (parent NoParent, zero
\* counters, no frame needed); the live set and parent relation carry the
\* actual hierarchy.
TypeOK ==
  /\ live \subseteq Frame
  /\ parent \in [Frame -> Frame \cup {NoParent}]
  /\ \A f \in Frame \ live: parent[f] = NoParent
  /\ \A f \in live: parent[f] # NoParent => parent[f] \in live
  /\ \A f \in live: parent[f] # f
  /\ \A f \in live: parent[f] # NoParent => f \notin Ancestors(parent[f])
  /\ pending \in [Frame -> 0..BufferCount]
  /\ \A f \in Frame \ live: pending[f] = 0
  /\ rendering_updated \in [Frame -> 0..BufferCount]
  /\ composed \in [Frame -> 0..BufferCount]
  /\ op_count \in [Frame -> 0..1]
  /\ animating \in [Frame -> BOOLEAN]
  /\ frame_needed \in [Frame -> BOOLEAN]

\* ---- Init ----

Init ==
  /\ live = {}
  /\ parent = [f \in Frame |-> NoParent]
  /\ pending = [f \in Frame |-> 0]
  /\ rendering_updated = [f \in Frame |-> 0]
  /\ composed = [f \in Frame |-> 0]
  /\ op_count = [f \in Frame |-> 0]
  /\ animating = [f \in Frame |-> FALSE]
  /\ frame_needed = [f \in Frame |-> FALSE]

\* ---- Actions ----

\* A navigable is born: the initial top-level traversable (parent NoParent) or
\* an iframe (parent is an existing frame). New frames start with an empty
\* pipeline and no frame needed.
CreateFrame(f, p) ==
  /\ f \in Frame \ live
  /\ p \in live \cup {NoParent}
  /\ live' = live \cup {f}
  /\ parent' = [parent EXCEPT ![f] = p]
  /\ UNCHANGED <<pending, rendering_updated, composed, op_count, animating,
                 frame_needed>>

\* An update is in flight for f when graphics has not yet computed it.
InFlight(f) == pending[f] > composed[f]

\* The top-level's paint services the batched opportunities of the frames
\* that compose with it: a frame whose pipeline is drained with a batched
\* opportunity or animated content.
ServicedFrames(top) ==
  IF InFlight(top)
  THEN {}
  ELSE {g \in TopLevelFrames(top): ~InFlight(g) /\ (op_count[g] > 0 \/ animating[g])}

\* A top-level's paint consumes the composed frames of its hierarchy and
\* services the batched opportunities: each serviced frame queues an
\* update, and the top-level queues one too (a child's update propagates
\* to the top-level so their frames compose together).
FrameNeeded(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ LET serviced == ServicedFrames(top)
         any == serviced # {}
     IN
     /\ composed' = [g \in Frame |->
          IF g \in TopLevelFrames(top) THEN 0 ELSE composed[g]]
     /\ rendering_updated' = [g \in Frame |->
          IF g \in TopLevelFrames(top)
          THEN rendering_updated[g] - composed[g]
          ELSE rendering_updated[g]]
     /\ pending' = [g \in Frame |->
          IF g \in TopLevelFrames(top)
          THEN pending[g] - composed[g]
               + IF g \in serviced \/ (g = top /\ any) THEN 1 ELSE 0
          ELSE pending[g]]
     /\ frame_needed' = [frame_needed EXCEPT ![top] =
          IF any THEN FALSE ELSE TRUE]
     /\ op_count' = [g \in Frame |->
          IF g \in serviced THEN 0 ELSE op_count[g]]
  /\ UNCHANGED <<animating, live, parent>>

\* The UA noted a rendering opportunity for f (input, navigation, viewport
\* change, or content request). When an update is in flight or the
\* top-level does not need a frame, the opportunity is batched (set
\* semantics: repeated notes stay one batched opportunity); otherwise
\* update the rendering is queued for f and, when f is a child, for its
\* top-level too — unless the top-level already has an update in flight,
\* in which case the propagation is skipped (the code's
\* queue_update_the_rendering early-returns when the navigable is already
\* pending), and the top-level's batched opportunity is drained only when
\* the propagation actually queues its update.
NoteRenderingOpportunity(f) ==
  /\ f \in live
  /\ IF InFlight(f) \/ ~frame_needed[TopLevel(f)]
     THEN /\ op_count' = [op_count EXCEPT ![f] = 1]
          /\ UNCHANGED <<pending, rendering_updated, composed, animating,
                         frame_needed, live, parent>>
     ELSE /\ pending' = [pending EXCEPT ![f] = pending[f] + 1,
                                        ![TopLevel(f)] =
                                          IF InFlight(TopLevel(f))
                                          THEN pending[TopLevel(f)]
                                          ELSE pending[TopLevel(f)] + 1]
          /\ frame_needed' = [frame_needed EXCEPT ![TopLevel(f)] = FALSE]
          /\ op_count' = [op_count EXCEPT ![f] = 0,
                                        ![TopLevel(f)] =
                                          IF InFlight(TopLevel(f))
                                          THEN op_count[TopLevel(f)]
                                          ELSE 0]
          /\ UNCHANGED <<rendering_updated, composed, animating, live, parent>>

\* Content runs the queued update: rendering_updated catches up to pending.
UpdateTheRendering(f) ==
  /\ f \in live
  /\ rendering_updated[f] < pending[f]
  /\ rendering_updated' = [rendering_updated EXCEPT ![f] = pending[f]]
  /\ UNCHANGED <<pending, composed, op_count, animating, frame_needed, live, parent>>

\* Graphics sent the top-level's frame (PixelFrameReady): composed catches
\* up to rendering_updated for the whole hierarchy (the composed scene
\* includes the child frames), and the in-flight updates of the composed
\* frames are released (pending drops to composed, mirroring the UA
\* clearing pending_update_the_rendering at PixelFrameReady). Animated
\* content batches the next opportunities, and a held frame_needed flag
\* with something to render queues the next updates immediately — the
\* serviced set is computed after the release, so a top-level whose own
\* update just composed can still service its children.
GraphicsComputed(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ rendering_updated[top] > composed[top]
  /\ LET hierarchy == TopLevelFrames(top)
         composed_after == [g \in Frame |->
              IF g \in hierarchy THEN rendering_updated[g] ELSE composed[g]]
         any_animating == \E g \in hierarchy: animating[g]
         serviced == IF frame_needed[top]
                     THEN {g \in hierarchy:
                             composed_after[g] = pending[g]
                               /\ (op_count[g] > 0 \/ animating[g])}
                     ELSE {}
         any == serviced # {}
     IN
     /\ composed' = composed_after
     /\ pending' = [g \in Frame |->
          IF g \in hierarchy
          THEN composed_after[g]
               + IF g \in serviced \/ (g = top /\ any) THEN 1 ELSE 0
          ELSE pending[g]]
     /\ frame_needed' = [frame_needed EXCEPT ![top] =
          IF any THEN FALSE ELSE frame_needed[top]]
     /\ op_count' = [g \in Frame |->
          IF g \in serviced \/ (g = top /\ any) THEN 0
          ELSE IF g \in hierarchy /\ animating[g] THEN 1
          ELSE IF g = top /\ any_animating THEN 1
          ELSE op_count[g]]
  /\ UNCHANGED <<rendering_updated, animating, live, parent>>

\* The render cycle's deadline expired with no top-level content update (the
\* content event loop is blocked, so no UpdateTheRendering ran): graphics
\* composes the committed hierarchy anyway, presenting a worker's
\* OffscreenCanvas commit or a video frame against the last committed root.
\* The deadline completes the cycle without the content update, so
\* rendering_updated and composed catch up to pending (the queued update is
\* treated as consumed). This mirrors GraphicsComputed with the "top-level
\* rendered" precondition replaced by "a top-level update is queued but has
\* not rendered": the deadline is the render cycle's bounded wait for the
\* top-level frame, after which the embedded layers are presented.
Deadline(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ rendering_updated[top] = composed[top]
  /\ pending[top] > composed[top]
  /\ LET hierarchy == TopLevelFrames(top)
         composed_after == [g \in Frame |->
              IF g \in hierarchy THEN pending[g] ELSE composed[g]]
         any_animating == \E g \in hierarchy: animating[g]
         serviced == IF frame_needed[top]
                     THEN {g \in hierarchy:
                             composed_after[g] = pending[g]
                               /\ (op_count[g] > 0 \/ animating[g])}
                     ELSE {}
         any == serviced # {}
     IN
     /\ rendering_updated' = [g \in Frame |->
          IF g \in hierarchy THEN pending[g] ELSE rendering_updated[g]]
     /\ composed' = composed_after
     /\ pending' = [g \in Frame |->
          IF g \in hierarchy
          THEN composed_after[g]
               + IF g \in serviced \/ (g = top /\ any) THEN 1 ELSE 0
          ELSE pending[g]]
     /\ frame_needed' = [frame_needed EXCEPT ![top] =
          IF any THEN FALSE ELSE frame_needed[top]]
     /\ op_count' = [g \in Frame |->
          IF g \in serviced \/ (g = top /\ any) THEN 0
          ELSE IF g \in hierarchy /\ animating[g] THEN 1
          ELSE IF g = top /\ any_animating THEN 1
          ELSE op_count[g]]
  /\ UNCHANGED <<animating, live, parent>>

\* ---- Next-state relation ----

Next ==
  \/ \E f \in Frame, p \in live \cup {NoParent}: CreateFrame(f, p)
  \/ \E f \in live: NoteRenderingOpportunity(f)
  \/ \E f \in live: IsTopLevel(f) /\ FrameNeeded(f)
  \/ \E f \in live: UpdateTheRendering(f)
  \/ \E f \in live: IsTopLevel(f) /\ GraphicsComputed(f)
  \/ \E f \in live: IsTopLevel(f) /\ Deadline(f)

\* ---- Fairness ----

\* TLC cannot quantify temporal formulas over the dynamic frame set, so the
\* fairness and liveness formulas quantify over the constant universe Frame.
\* The per-frame state is total over Frame (defaults for non-live frames),
\* and the actions guard f \in live, so non-live frames keep their actions
\* disabled and their liveness antecedents false — every subexpression is
\* defined for every frame.
Fairness ==
  /\ \A f \in Frame: WF_vars(FrameNeeded(f))
  /\ \A f \in Frame: WF_vars(UpdateTheRendering(f))
  /\ \A f \in Frame: WF_vars(GraphicsComputed(f))

Spec == Init /\ [][Next]_vars /\ Fairness

\* ---- Invariants ----

\* A queued update is completed by content before graphics computes it.
PendingLeadsRendering ==
  \A f \in live: pending[f] >= rendering_updated[f]

RenderingLeadsComposed ==
  \A f \in live: rendering_updated[f] >= composed[f]

\* The pipeline never holds more than BufferCount updates queued but not yet
\* consumed by the embedder's paint (the double buffer).
DoubleBufferBound ==
  \A f \in live: pending[f] <= BufferCount

\* ---- Liveness ----

\* Batched opportunities are serviced when the top-level needs a frame: once
\* a frame is needed and a render can start, the batch drains to zero.
OpportunitiesServiced ==
  \A f \in Frame:
    ((f \in live /\ op_count[f] > 0 /\ frame_needed[TopLevel(f)]
        /\ pending[f] < BufferCount)
       ~> (op_count[f] = 0))

THEOREM
    Spec => (TypeOK /\ PendingLeadsRendering /\ RenderingLeadsComposed
             /\ DoubleBufferBound)
=============================================================================
