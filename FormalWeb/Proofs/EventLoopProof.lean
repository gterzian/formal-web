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

end FormalWeb
