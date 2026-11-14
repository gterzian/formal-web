---------------------------- MODULE RenderingOpportunity ----------------------------
EXTENDS Naturals, FiniteSets, TLC

(* Validates the render cycle the RenderingOpportunity trace events record: the
   rendering-opportunity gate, update the rendering, and the three composition
   kinds. *)

CONSTANTS
  Frame,        \* the universe of frame ids the trace can reference
  NoParent,     \* no parent (top-level frame)
  BufferCount,  \* updates the UA holds per navigable: the one it waits for and the batched one
  CommandDepth  \* bound on the updates content was sent and has not run (keeps the model finite)

ASSUME
  /\ Frame # {}
  /\ NoParent \notin Frame
  /\ BufferCount > 0
  /\ CommandDepth > BufferCount

VARIABLES
  live,                  \* created frames (the navigable hierarchy grows)
  parent,                \* parent navigable of f (NoParent at the top level)
  queued,                \* update the rendering commands content was sent and has not run yet
  rendering_updated,     \* content ran update the rendering (UpdateTheRendering)
  composed,              \* top-level content compositions that presented a render (GraphicsComputed)
  pending_update,        \* the user agent waits for an update: its pending_update_the_rendering
  cycle,                 \* graphics holds the update's deadline: its cycle_deadline
  ui_state_update_count, \* a batched UI state update (UA queued_ui_state_updates, 0 or 1)
  animating,             \* the last composed frame had animated content
  rendering_opportunity  \* the embedder can present a frame (`frame_needed`, sent at each paint)

vars == <<live, parent, queued, rendering_updated, composed, pending_update, cycle,
          ui_state_update_count, animating, rendering_opportunity>>

\* ---- Helper operators ----

RECURSIVE Ancestors(_)
Ancestors(f) == {f} \cup IF parent[f] = NoParent THEN {} ELSE Ancestors(parent[f])

TopLevel(f) == CHOOSE t \in Ancestors(f): parent[t] = NoParent

IsTopLevel(f) == parent[f] = NoParent

TopLevelFrames(top) == {f \in live: TopLevel(f) = top}

\* ---- Type correctness ----

\* The queued/rendered/composed counters are relative to the last paint (each
\* paint rebases them by the compositions it consumed) and bounded by
\* CommandDepth, the model's bound on what content may owe. Non-live frames
\* keep default values (parent NoParent, zero counters, no flags); the live set
\* and the parent relation carry the actual hierarchy.
TypeOK ==
  /\ live \subseteq Frame
  /\ parent \in [Frame -> Frame \cup {NoParent}]
  /\ \A f \in Frame \ live: parent[f] = NoParent
  /\ \A f \in live: parent[f] # NoParent => parent[f] \in live
  /\ \A f \in live: parent[f] # f
  /\ \A f \in live: parent[f] # NoParent => f \notin Ancestors(parent[f])
  /\ queued \in [Frame -> 0..CommandDepth]
  /\ rendering_updated \in [Frame -> 0..CommandDepth]
  /\ composed \in [Frame -> 0..CommandDepth]
  /\ \A f \in Frame \ live: queued[f] = 0
  /\ \A f \in Frame \ live: rendering_updated[f] = 0
  /\ \A f \in Frame \ live: composed[f] = 0
  /\ pending_update \in [Frame -> BOOLEAN]
  /\ cycle \in [Frame -> BOOLEAN]
  /\ ui_state_update_count \in [Frame -> 0..1]
  /\ animating \in [Frame -> BOOLEAN]
  /\ rendering_opportunity \in [Frame -> BOOLEAN]

\* ---- Init ----

Init ==
  /\ live = {}
  /\ parent = [f \in Frame |-> NoParent]
  /\ queued = [f \in Frame |-> 0]
  /\ rendering_updated = [f \in Frame |-> 0]
  /\ composed = [f \in Frame |-> 0]
  /\ pending_update = [f \in Frame |-> FALSE]
  /\ cycle = [f \in Frame |-> FALSE]
  /\ ui_state_update_count = [f \in Frame |-> 0]
  /\ animating = [f \in Frame |-> FALSE]
  /\ rendering_opportunity = [f \in Frame |-> FALSE]

\* ---- Actions ----

\* A navigable is born: the initial top-level traversable (parent NoParent) or
\* an iframe (parent is an existing frame). New frames start with an empty
\* pipeline and no rendering opportunity.
CreateFrame(f, p) ==
  /\ f \in Frame \ live
  /\ p \in live \cup {NoParent}
  /\ live' = live \cup {f}
  /\ parent' = [parent EXCEPT ![f] = p]
  /\ UNCHANGED <<queued, rendering_updated, composed, pending_update, cycle,
                 ui_state_update_count, animating, rendering_opportunity>>

\* The embedder started a paint: the top-level navigable has a rendering
\* opportunity. The paint presents what was composed since the last paint, so
\* the counters of the hierarchy rebase by that count. The batched UI state
\* updates are then serviced: every navigable of the hierarchy with a batched
\* note or animated content that is not already waiting queues an update, and
\* the top-level queues one too (a child's update propagates to the top-level
\* so their frames compose together). A top-level that is already waiting, or
\* a navigable the model's CommandDepth bound leaves no room for, keeps its
\* opportunity and its batch, so the next paint can serve them.
RenderingOpportunity(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ LET hierarchy == TopLevelFrames(top)
         serviced == {g \in hierarchy:
                        ~pending_update[g]
                          /\ (ui_state_update_count[g] > 0 \/ animating[g])}
         queues_update(g) == g \in serviced \/ (g = top /\ serviced # {})
         fits == \A g \in Frame: queues_update(g) => queued[g] < CommandDepth
         any == \E g \in Frame: queues_update(g)
         queued_after == [g \in Frame |->
              IF g \in hierarchy THEN queued[g] - composed[g] ELSE queued[g]]
         rendered_after == [g \in Frame |->
              IF g \in hierarchy THEN rendering_updated[g] - composed[g] ELSE rendering_updated[g]]
     IN
     /\ fits
     /\ composed' = [g \in Frame |->
          IF g \in hierarchy THEN 0 ELSE composed[g]]
     /\ queued' = [g \in Frame |->
          queued_after[g] + IF queues_update(g) THEN 1 ELSE 0]
     /\ rendering_updated' = rendered_after
     /\ pending_update' = [g \in Frame |->
          IF queues_update(g) THEN TRUE ELSE pending_update[g]]
     /\ cycle' = [g \in Frame |->
          IF queues_update(g) THEN TRUE ELSE cycle[g]]
     /\ rendering_opportunity' = [rendering_opportunity EXCEPT ![top] =
          IF any THEN FALSE ELSE TRUE]
     /\ ui_state_update_count' = [g \in Frame |->
          IF queues_update(g) THEN 0 ELSE ui_state_update_count[g]]
     /\ UNCHANGED <<animating, live, parent>>

\* The UA noted a UI state update for f (input, navigation, viewport change,
\* content request, or the graphics process reporting a changed embedded
\* layer). While the UA already waits for an update of f or the top-level has
\* no rendering opportunity, the note is batched (set semantics: repeated
\* notes stay one batched update); otherwise update the rendering is queued
\* for f and, when f is a child, for its top-level too — unless the top-level
\* already waits for one, in which case the propagation is skipped (the code's
\* queue_update_the_rendering early-returns), and a drained batched update of
\* the top-level is cleared only when the propagation actually queues.
NoteUiStateUpdate(f) ==
  /\ f \in live
  /\ LET top == TopLevel(f)
         propagate == f # top /\ ~pending_update[top]
         queues_update(g) == g = f \/ (propagate /\ g = top)
         fits == \A g \in Frame: queues_update(g) => queued[g] < CommandDepth
     IN
     IF pending_update[f] \/ ~rendering_opportunity[top]
     THEN /\ ui_state_update_count' = [ui_state_update_count EXCEPT ![f] = 1]
          /\ UNCHANGED <<queued, rendering_updated, composed, pending_update, cycle,
                         animating, rendering_opportunity, live, parent>>
     ELSE /\ fits
          /\ queued' = [g \in Frame |->
               queued[g] + IF queues_update(g) THEN 1 ELSE 0]
          /\ pending_update' = [g \in Frame |->
               IF queues_update(g) THEN TRUE ELSE pending_update[g]]
          /\ cycle' = [g \in Frame |->
               IF queues_update(g) THEN TRUE ELSE cycle[g]]
          /\ rendering_opportunity' = [rendering_opportunity EXCEPT ![top] = FALSE]
          /\ ui_state_update_count' = [g \in Frame |->
               IF queues_update(g) THEN 0 ELSE ui_state_update_count[g]]
          /\ UNCHANGED <<rendering_updated, composed, animating, live, parent>>

\* Content runs one queued update the rendering: one queued command becomes one
\* render, and the frame it produces is on its way to the compositor, so
\* `cycle` is FALSE for it while `pending_update` still holds.
UpdateTheRendering(f) ==
  /\ f \in live
  /\ rendering_updated[f] < queued[f]
  /\ rendering_updated' = [g \in Frame |->
       IF g = f THEN rendering_updated[g] + 1 ELSE rendering_updated[g]]
  /\ cycle' = [g \in Frame |->
       IF g = f THEN FALSE ELSE cycle[g]]
  /\ UNCHANGED <<queued, composed, pending_update, ui_state_update_count,
                 animating, rendering_opportunity, live, parent>>

\* Graphics sent the top-level's frame (PixelFrameReady, composition kind
\* ContentUpdate): the composition presents one rendered top-level frame and
\* the latest frame of every child (a child's frames share one layer, so the
\* newest replaces the older). The user agent releases the update it waited
\* for, for the whole hierarchy, since the composed scene includes the
\* children. Animated content batches the next UI state updates, and a held
\* rendering opportunity with something to render queues the next updates
\* immediately — the serviced set is computed after the release, so a
\* navigable whose update just composed can still service a batched note.
GraphicsComputed(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ rendering_updated[top] > composed[top]
  /\ LET hierarchy == TopLevelFrames(top)
         released == pending_update[top]
         serviced == IF released /\ rendering_opportunity[top]
                     THEN {g \in hierarchy:
                             ui_state_update_count[g] > 0 \/ animating[g]}
                     ELSE {}
         queues_update(g) == g \in serviced \/ (g = top /\ serviced # {})
         fits == \A g \in Frame: queues_update(g) => queued[g] < CommandDepth
         any == \E g \in Frame: queues_update(g)
         any_animating == \E g \in hierarchy: animating[g]
     IN
     /\ fits
     /\ composed' = [g \in Frame |->
          IF g = top THEN composed[g] + 1
          ELSE IF g \in hierarchy THEN rendering_updated[g]
          ELSE composed[g]]
     /\ queued' = [g \in Frame |->
          queued[g] + IF queues_update(g) THEN 1 ELSE 0]
     /\ pending_update' = [g \in Frame |->
          IF queues_update(g) THEN TRUE
          ELSE IF g \in hierarchy /\ released THEN FALSE
          ELSE pending_update[g]]
     /\ cycle' = [g \in Frame |->
          IF queues_update(g) THEN TRUE
          ELSE IF g \in hierarchy THEN FALSE
          ELSE cycle[g]]
     /\ rendering_opportunity' = [rendering_opportunity EXCEPT ![top] =
          IF any THEN FALSE ELSE rendering_opportunity[top]]
     /\ ui_state_update_count' = [g \in Frame |->
          IF queues_update(g) THEN 0
          ELSE IF g \in hierarchy /\ animating[g] THEN 1
          ELSE IF g = top /\ any_animating THEN 1
          ELSE ui_state_update_count[g]]
     /\ UNCHANGED <<rendering_updated, animating, live, parent>>

\* The render cycle's deadline expired before the top-level frame arrived (the
\* content event loop is blocked, so the frame is late): graphics composes the
\* committed hierarchy against the last committed root, presenting a worker's
\* OffscreenCanvas commit or a video frame. The composition presents no new
\* content state, so no render counter moves; the late top-level frame
\* composes separately when it arrives (GraphicsComputed). Like a content
\* composition it releases the update the user agent waited for, so the next
\* note queues an update (and dispatches the worker's animation frame callback)
\* while content is blocked, and it services the batched UI state updates under
\* the same gate.
Deadline(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ cycle[top]
  /\ LET hierarchy == TopLevelFrames(top)
         serviced == IF rendering_opportunity[top]
                     THEN {g \in hierarchy:
                             ui_state_update_count[g] > 0 \/ animating[g]}
                     ELSE {}
         queues_update(g) == g \in serviced \/ (g = top /\ serviced # {})
         fits == \A g \in Frame: queues_update(g) => queued[g] < CommandDepth
         any == \E g \in Frame: queues_update(g)
         any_animating == \E g \in hierarchy: animating[g]
     IN
     /\ fits
     /\ queued' = [g \in Frame |->
          queued[g] + IF queues_update(g) THEN 1 ELSE 0]
     /\ pending_update' = [g \in Frame |->
          IF queues_update(g) THEN TRUE
          ELSE IF g \in hierarchy THEN FALSE
          ELSE pending_update[g]]
     /\ cycle' = [g \in Frame |->
          IF g \in hierarchy THEN FALSE ELSE cycle[g]]
     /\ rendering_opportunity' = [rendering_opportunity EXCEPT ![top] =
          IF any THEN FALSE ELSE rendering_opportunity[top]]
     /\ ui_state_update_count' = [g \in Frame |->
          IF queues_update(g) THEN 0
          ELSE IF g \in hierarchy /\ animating[g] THEN 1
          ELSE IF g = top /\ any_animating THEN 1
          ELSE ui_state_update_count[g]]
     /\ UNCHANGED <<rendering_updated, composed, animating, live, parent>>

\* Graphics presented an out-of-band layer — a video frame — against the
\* committed root. Content is not involved: the document's own scene is
\* unchanged, so no counter and no flag moves, and the user agent keeps waiting
\* for the update in flight. Graphics composes it while `cycle` is FALSE, i.e.
\* while content produced every queued update's frame; whether an out-of-band
\* layer actually changed is environment state the model does not track.
OutOfBandComputed(top) ==
  /\ top \in live
  /\ IsTopLevel(top)
  /\ ~cycle[top]
  /\ UNCHANGED vars

\* ---- Next-state relation ----

\* Every queued counter is at the model's cap, so no update can be queued and
\* no batch can drain. The stutter keeps every execution infinite where the
\* bounded model has run out of room; the pipeline itself has no such cap.
Done ==
  /\ live # {}
  /\ \A f \in live: queued[f] = CommandDepth
  /\ UNCHANGED vars

Next ==
  \/ \E f \in Frame, p \in live \cup {NoParent}: CreateFrame(f, p)
  \/ \E f \in live: NoteUiStateUpdate(f)
  \/ \E f \in live: IsTopLevel(f) /\ RenderingOpportunity(f)
  \/ \E f \in live: UpdateTheRendering(f)
  \/ \E f \in live: IsTopLevel(f) /\ GraphicsComputed(f)
  \/ \E f \in live: IsTopLevel(f) /\ Deadline(f)
  \/ \E f \in live: IsTopLevel(f) /\ OutOfBandComputed(f)
  \/ Done

\* ---- Fairness ----

\* TLC cannot quantify temporal formulas over the dynamic frame set, so the
\* fairness and liveness formulas quantify over the constant universe Frame.
\* The per-frame state is total over Frame (defaults for non-live frames),
\* and the actions guard f \in live, so non-live frames keep their actions
\* disabled and their liveness antecedents false — every subexpression is
\* defined for every frame.
Fairness ==
  /\ \A f \in Frame: WF_vars(RenderingOpportunity(f))
  /\ \A f \in Frame: WF_vars(UpdateTheRendering(f))
  /\ \A f \in Frame: WF_vars(GraphicsComputed(f))

Spec == Init /\ [][Next]_vars /\ Fairness

\* ---- Invariants ----

\* Content runs an update only after the user agent queued it.
QueuedLeadsRendering ==
  \A f \in live: rendering_updated[f] <= queued[f]

\* A composition presents a rendered frame.
RenderingLeadsComposed ==
  \A f \in live: composed[f] <= rendering_updated[f]

\* The user agent holds at most BufferCount updates per navigable: the one it
\* waits for and the one batched for the next rendering opportunity. Content
\* may owe more than that — a deadline composition releases the update the
\* user agent stopped waiting for.
DoubleBufferBound ==
  \A f \in live:
    (IF pending_update[f] THEN 1 ELSE 0) + ui_state_update_count[f] <= BufferCount

\* An update the user agent waits for, and a cycle graphics armed, always have
\* an uncomposed update behind them: the flags are set by queueing an update
\* and cleared by a composition that presents it.
PendingUpdateLeadsComposition ==
  \A f \in live:
    /\ pending_update[f] => queued[f] > composed[f]
    /\ cycle[f] => pending_update[f]
    /\ cycle[f] => queued[f] > rendering_updated[f]

\* ---- Liveness ----

\* Batched UI state updates are serviced when the top-level has a rendering
\* opportunity: once one exists, the navigable is not already waiting for an
\* update, and content has room for another queued one, the batch drains to
\* zero.
UiStateUpdatesServiced ==
  \A f \in Frame:
    ((f \in live /\ ui_state_update_count[f] > 0 /\ rendering_opportunity[TopLevel(f)]
        /\ ~pending_update[f] /\ queued[f] < CommandDepth
        /\ queued[TopLevel(f)] < CommandDepth)
       ~> (ui_state_update_count[f] = 0))

THEOREM
    Spec => (TypeOK /\ QueuedLeadsRendering /\ RenderingLeadsComposed
             /\ DoubleBufferBound /\ PendingUpdateLeadsComposition)
=============================================================================
