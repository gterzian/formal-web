import FormalWeb.EventLoop
import FormalWeb.TransitionTrace

namespace FormalWeb

/--
LTS-style actions for the standalone event-loop task queue model.
-/
inductive EventLoopAction
  | queueTask (task : Task)
  | runNextTask
deriving Repr, DecidableEq

/--
Apply one event-loop transition.
-/
def eventLoopStep
    (eventLoop : EventLoop)
    (action : EventLoopAction) :
    Option EventLoop :=
  match action with
  | .queueTask task =>
      if task.step = .updateTheRendering then
        pure (eventLoop.enqueueUpdateTheRenderingTask)
      else
        pure (eventLoop.enqueueTask task)
  | .runNextTask =>
      Option.map Prod.snd (eventLoop.takeNextTask?)

theorem queueTask_trace
    (eventLoop : EventLoop)
    (task : Task)
    (hnotUpdate : task.step ≠ .updateTheRendering) :
    TransitionTrace
      eventLoopStep
      eventLoop
      [.queueTask task]
      (eventLoop.enqueueTask task) := by
  refine TransitionTrace.single ?_
  simp [eventLoopStep, hnotUpdate]

theorem queueUpdateTheRendering_trace
    (eventLoop : EventLoop) :
    TransitionTrace
      eventLoopStep
      eventLoop
      [.queueTask { step := .updateTheRendering }]
      (eventLoop.enqueueUpdateTheRenderingTask) := by
  refine TransitionTrace.single ?_
  simp [eventLoopStep]

theorem runNextTask_trace
    (eventLoop nextEventLoop : EventLoop)
    (task : Task)
    (htake : eventLoop.takeNextTask? = some (task, nextEventLoop)) :
    TransitionTrace
      eventLoopStep
      eventLoop
      [.runNextTask]
      nextEventLoop := by
  refine TransitionTrace.single ?_
  simp [eventLoopStep, htake]

end FormalWeb
