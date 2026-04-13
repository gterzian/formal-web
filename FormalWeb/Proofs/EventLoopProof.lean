import FormalWeb.EventLoop
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for the standalone event-loop worker. -/
inductive EventLoopAction
  | handleMessage (message : EventLoopTaskMessage)
  | scheduleNextTask
deriving Repr, DecidableEq

/-- Relational LTS for the event-loop worker's pure message-drain phase and single scheduling step. -/
def handleMessageState
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    EventLoopTaskState :=
  ((handleEventLoopTaskMessage message).run state).2

def queueMessagesState
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    EventLoopTaskState :=
  messages.foldl handleMessageState state

def eventLoopLTS : TransitionSystem.LTS EventLoopTaskState EventLoopAction where
  init := fun state => state = default
  trans := fun state action state' =>
    match action with
    | .handleMessage message =>
        state' = handleMessageState state message
    | .scheduleNextTask =>
        state' = (runNextQueuedTaskM state).2

theorem handleMessage_trace
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      [.handleMessage message]
      (handleMessageState state message) := by
  exact TransitionSystem.TransitionTrace.single rfl

theorem scheduleNextTask_trace
    (state : EventLoopTaskState) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      [.scheduleNextTask]
      (runNextQueuedTaskM state).2 := by
  exact TransitionSystem.TransitionTrace.single rfl

def eventLoopMessageActions (messages : List EventLoopTaskMessage) : List EventLoopAction :=
  messages.map EventLoopAction.handleMessage ++ [.scheduleNextTask]

private theorem handleMessages_trace
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      (messages.map EventLoopAction.handleMessage)
      (queueMessagesState state messages) := by
  induction messages generalizing state with
  | nil =>
      simpa using TransitionSystem.TransitionTrace.nil state
  | cons message messages ih =>
      have hstep :
          eventLoopLTS.trans
            state
            (.handleMessage message)
            (handleMessageState state message) := by
        rfl
      have htrace :=
        ih (handleMessageState state message)
      simpa [queueMessagesState, handleMessageState, List.map, List.foldl] using
        TransitionSystem.TransitionTrace.cons hstep htrace

private theorem queueEffectsFold_state
    (initialEffects : Array EventLoopEffect)
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    (messages.foldl
      (fun (effects, currentState) message =>
        let (((), nextEffects), nextState) :=
          (handleEventLoopTaskMessage message).run currentState
        (effects ++ nextEffects, nextState))
      (initialEffects, state)).2 =
    queueMessagesState state messages := by
  induction messages generalizing initialEffects state with
  | nil =>
      rfl
  | cons message messages ih =>
      simpa [queueMessagesState, handleMessageState, List.foldl] using
        ih
          (initialEffects ++ ((handleEventLoopTaskMessage message).run state).1.2)
          (handleMessageState state message)

theorem runEventLoopMessagesMonadic_trace
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      (eventLoopMessageActions messages)
      (runEventLoopMessagesMonadic state messages).2 := by
  let queuedState := queueMessagesState state messages
  have hqueue :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state
        (messages.map EventLoopAction.handleMessage)
        queuedState := by
    simpa [queuedState] using handleMessages_trace state messages
  have hschedule :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        queuedState
        [.scheduleNextTask]
        (runNextQueuedTaskM queuedState).2 := by
    exact scheduleNextTask_trace queuedState
  have hqueuedState :
      (messages.foldl
        (fun (effects, currentState) message =>
          let (((), nextEffects), nextState) :=
            (handleEventLoopTaskMessage message).run currentState
          (effects ++ nextEffects, nextState))
        (#[], state)).2 = queuedState := by
    simpa [queuedState] using queueEffectsFold_state #[] state messages
  simpa [eventLoopMessageActions, runEventLoopMessagesMonadic, queuedState] using
    hqueuedState ▸ TransitionSystem.TransitionTrace.append hqueue hschedule

theorem runEventLoopMonadic_trace
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      (eventLoopMessageActions [message])
      (runEventLoopMonadic state message).2 := by
  simpa [runEventLoopMonadic] using runEventLoopMessagesMonadic_trace state [message]

end FormalWeb
