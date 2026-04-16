import FormalWeb.Proofs.TransitionSystem
import FormalWeb.Timer

namespace FormalWeb

/-- LTS-style actions for the standalone timer worker. -/
inductive TimerAction
  | handleMessage (message : TimerTaskMessage)
deriving Repr, DecidableEq

/-- Relational LTS for the timer worker's pure message handling. -/
def handleTimerMessageState
    (timer : Timer)
    (message : TimerTaskMessage) :
    Timer :=
  (runTimerMessagesMonadic timer [message]).2

def queueTimerMessagesState
    (timer : Timer)
    (messages : List TimerTaskMessage) :
    Timer :=
  (runTimerMessagesMonadic timer messages).2

def timerLTS : TransitionSystem.LTS Timer TimerAction where
  init := fun timer => timer = default
  trans := fun timer action timer' =>
    match action with
    | .handleMessage message =>
        timer' = handleTimerMessageState timer message

theorem handleTimerMessage_trace
    (timer : Timer)
    (message : TimerTaskMessage) :
    TransitionSystem.TransitionTrace
      timerLTS
      timer
      [.handleMessage message]
      (handleTimerMessageState timer message) := by
  exact TransitionSystem.TransitionTrace.single rfl

def timerMessageActions
    (messages : List TimerTaskMessage) :
    List TimerAction :=
  messages.map TimerAction.handleMessage

private theorem handleTimerMessages_trace
    (timer : Timer)
    (messages : List TimerTaskMessage) :
    TransitionSystem.TransitionTrace
      timerLTS
      timer
      (timerMessageActions messages)
      (queueTimerMessagesState timer messages) := by
  induction messages generalizing timer with
  | nil =>
      simpa [timerMessageActions, queueTimerMessagesState, runTimerMessagesMonadic] using
        TransitionSystem.TransitionTrace.nil timer
  | cons message messages ih =>
      have hstep :
          timerLTS.trans
            timer
            (.handleMessage message)
            (handleTimerMessageState timer message) := by
        rfl
      have htrace :=
        ih (handleTimerMessageState timer message)
      simpa [timerMessageActions, handleTimerMessageState, queueTimerMessagesState] using
        TransitionSystem.TransitionTrace.cons hstep htrace

theorem runTimerMessagesMonadic_trace
    (timer : Timer)
    (messages : List TimerTaskMessage) :
    TransitionSystem.TransitionTrace
      timerLTS
      timer
      (timerMessageActions messages)
      (runTimerMessagesMonadic timer messages).2 := by
  simpa [queueTimerMessagesState] using handleTimerMessages_trace timer messages

end FormalWeb
