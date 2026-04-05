import FormalWeb.EventLoop
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for the standalone event-loop task queue model. -/
inductive EventLoopAction
  | queueTask (task : Task)
  | runNextTask
deriving Repr, DecidableEq

/-- Relational LTS for the standalone event-loop task queue model. -/
def eventLoopLTS : TransitionSystem.LTS EventLoop EventLoopAction where
  init := fun eventLoop => eventLoop = default
  trans := fun eventLoop action eventLoop' =>
    match action with
    | .queueTask task =>
        if task.step = .updateTheRendering then
          eventLoop' = eventLoop.enqueueUpdateTheRenderingTask
        else
          eventLoop' = eventLoop.enqueueTask task
    | .runNextTask =>
        ∃ task, eventLoop.takeNextTask? = some (task, eventLoop')

theorem queueTask_trace
    (eventLoop : EventLoop)
    (task : Task)
    (hnotUpdate : task.step ≠ .updateTheRendering) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      eventLoop
      [.queueTask task]
      (eventLoop.enqueueTask task) := by
  refine TransitionSystem.TransitionTrace.single ?_
  simp [eventLoopLTS, hnotUpdate]

theorem queueUpdateTheRendering_trace
    (eventLoop : EventLoop) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      eventLoop
      [.queueTask { step := .updateTheRendering }]
      (eventLoop.enqueueUpdateTheRenderingTask) := by
  refine TransitionSystem.TransitionTrace.single ?_
  simp [eventLoopLTS]

theorem runNextTask_trace
    (eventLoop nextEventLoop : EventLoop)
    (task : Task)
    (htake : eventLoop.takeNextTask? = some (task, nextEventLoop)) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      eventLoop
      [.runNextTask]
      nextEventLoop := by
  refine TransitionSystem.TransitionTrace.single ?_
  exact ⟨task, htake⟩

def interpretEffect : EventLoopEffect → List EventLoopAction
  | .queueTask task =>
      [.queueTask task]
  | .runNextTask _ _ =>
      [.runNextTask]

def interpretEffects (effects : Array EventLoopEffect) : List EventLoopAction :=
  effects.toList.flatMap interpretEffect

theorem interpretEffects_queueTask_cons
    (task : Task)
    (effects : Array EventLoopEffect) :
    interpretEffects (#[.queueTask task] ++ effects) =
      [.queueTask task] ++ interpretEffects effects := by
  simp [interpretEffects, interpretEffect]

theorem enqueueAndRunNext_eventLoop
  (state : EventLoopTaskState)
  (task : Task) :
  (enqueueAndRunNext state task).2.eventLoop = (runNextQueuedTaskM state).2.eventLoop := by
  unfold enqueueAndRunNext
  cases hrun : runNextQueuedTaskM state with
  | mk result nextState =>
    rfl

theorem enqueueAndRunNext_effects
  (state : EventLoopTaskState)
  (task : Task) :
  interpretEffects (enqueueAndRunNext state task).1 =
    [.queueTask task] ++ interpretEffects (runNextQueuedTaskM state).1.2 := by
  unfold enqueueAndRunNext
  cases hrun : runNextQueuedTaskM state with
  | mk result nextState =>
    cases result with
      | mk runtimeUnit nextEffects =>
      simp [interpretEffects_queueTask_cons]

theorem runNextQueuedTaskM_eventLoop
    (state : EventLoopTaskState)
    (task : Task)
    (nextEventLoop : EventLoop)
    (htake : state.eventLoop.takeNextTask? = some (task, nextEventLoop)) :
    (runNextQueuedTaskM state).2.eventLoop = nextEventLoop := by
  unfold runNextQueuedTaskM
  cases hstep : task.step <;> simp [htake, hstep] <;>
    first
    | cases state.pendingCreateEmptyDocumentTasks <;> rfl
    | cases state.pendingCreateLoadedDocumentTasks <;> rfl
    | cases state.pendingUpdateTheRenderingTasks <;> rfl
    | cases state.pendingDispatchEventTasks <;> rfl
    | cases state.pendingPaintTasks <;> rfl
    | cases state.pendingDocumentFetchCompletionTasks <;> rfl

theorem runNextQueuedTaskM_full_refinement
    (state : EventLoopTaskState) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state.eventLoop
        actions
        (runNextQueuedTaskM state).2.eventLoop ∧
      interpretEffects (runNextQueuedTaskM state).1.2 = actions := by
  cases htake : state.eventLoop.takeNextTask? with
  | none =>
      refine ⟨[], ?_, ?_⟩
      · have hstate : (runNextQueuedTaskM state).2.eventLoop = state.eventLoop := by
          simp [runNextQueuedTaskM, htake]
        simpa [hstate] using (TransitionSystem.TransitionTrace.nil state.eventLoop)
      · simp [runNextQueuedTaskM, interpretEffects, htake]
  | some result =>
      rcases result with ⟨task, nextEventLoop⟩
      refine ⟨[.runNextTask], ?_, ?_⟩
      · have heventLoop : (runNextQueuedTaskM state).2.eventLoop = nextEventLoop := by
          exact runNextQueuedTaskM_eventLoop state task nextEventLoop htake
        simpa [heventLoop] using runNextTask_trace state.eventLoop nextEventLoop task htake
      · simp [runNextQueuedTaskM, interpretEffects, interpretEffect, htake]

theorem handleEventLoopTaskMessage_full_refinement
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state.eventLoop
        actions
        (runEventLoopMonadic state message).2.eventLoop ∧
      interpretEffects (runEventLoopMonadic state message).1 = actions := by
  cases message with
  | createEmptyDocument documentId =>
      let task : Task := {
        step := .createEmptyDocument
        documentId := some documentId.id
      }
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCreateEmptyDocumentTasks :=
            state.pendingCreateEmptyDocumentTasks.concat { documentId }
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        exact queueTask_trace state.eventLoop task (by simp [task])
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
  | createLoadedDocument documentId url body =>
      let task : Task := {
        step := .createLoadedDocument
        documentId := some documentId.id
      }
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCreateLoadedDocumentTasks :=
            state.pendingCreateLoadedDocumentTasks.concat { documentId, url, body }
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        exact queueTask_trace state.eventLoop task (by simp [task])
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
  | queueUpdateTheRendering traversableId documentId =>
      let task : Task := { step := .updateTheRendering }
      let shouldAppendTask := !state.eventLoop.hasPendingUpdateTheRendering
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueUpdateTheRenderingTask
          pendingUpdateTheRenderingTasks :=
            if shouldAppendTask then
              state.pendingUpdateTheRenderingTasks.concat { traversableId, documentId }
            else
              state.pendingUpdateTheRenderingTasks
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        simpa [task, queuedState, shouldAppendTask] using queueUpdateTheRendering_trace state.eventLoop
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState, shouldAppendTask] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
  | queueDispatchEvent documentId event =>
      let task : Task := {
        step := .dispatchEvent
        documentId := some documentId.id
      }
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDispatchEventTasks := state.pendingDispatchEventTasks.concat { documentId, event }
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        exact queueTask_trace state.eventLoop task (by simp [task])
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
  | queuePaint documentId =>
      let task : Task := {
        step := .paint
        documentId := some documentId.id
      }
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingPaintTasks := state.pendingPaintTasks.concat { documentId }
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        exact queueTask_trace state.eventLoop task (by simp [task])
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
  | queueDocumentFetchCompletion handler resolvedUrl body =>
      let task : Task := { step := .completeDocumentFetch }
      let queuedState : EventLoopTaskState := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDocumentFetchCompletionTasks :=
            state.pendingDocumentFetchCompletionTasks.concat { handler, resolvedUrl, body }
      }
      have hqueue :
          TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            [.queueTask task]
            queuedState.eventLoop := by
        exact queueTask_trace state.eventLoop task (by simp [task])
      rcases runNextQueuedTaskM_full_refinement queuedState with ⟨actions₂, htrace₂, hinterp₂⟩
      refine ⟨[.queueTask task] ++ actions₂, ?_, ?_⟩
      · change TransitionSystem.TransitionTrace
            eventLoopLTS
            state.eventLoop
            ([.queueTask task] ++ actions₂)
            (enqueueAndRunNext queuedState task).2.eventLoop
        rw [enqueueAndRunNext_eventLoop]
        simpa [runEventLoopMonadic, handleEventLoopTaskMessage, task, queuedState] using
          TransitionSystem.TransitionTrace.append hqueue htrace₂
      · change interpretEffects (enqueueAndRunNext queuedState task).1 = [.queueTask task] ++ actions₂
        rw [enqueueAndRunNext_effects, hinterp₂]
