import FormalWeb.EventLoop
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for the standalone event-loop worker. -/
inductive EventLoopAction
  | handleMessage (message : EventLoopTaskMessage)
  | coalesceQueue
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

def coalesceQueueState
    (state : EventLoopTaskState) :
    EventLoopTaskState :=
  coalesceQueuedHighFrequencyWork state

def eventLoopLTS : TransitionSystem.LTS EventLoopTaskState EventLoopAction where
  init := fun state => state = default
  trans := fun state action state' =>
    match action with
    | .handleMessage message =>
        state' = handleMessageState state message
    | .coalesceQueue =>
      state' = coalesceQueueState state
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

theorem coalesceQueue_trace
    (state : EventLoopTaskState) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      [.coalesceQueue]
      (coalesceQueueState state) := by
  exact TransitionSystem.TransitionTrace.single rfl

def eventLoopMessageActions (messages : List EventLoopTaskMessage) : List EventLoopAction :=
  messages.map EventLoopAction.handleMessage ++ [.coalesceQueue, .scheduleNextTask]

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

private theorem runEventLoopMessagesMonadic_state
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    (runEventLoopMessagesMonadic state messages).snd =
      (runNextQueuedTaskM (coalesceQueueState (queueMessagesState state messages))).snd := by
  let step :
      (Array EventLoopEffect × EventLoopTaskState) →
        EventLoopTaskMessage →
        (Array EventLoopEffect × EventLoopTaskState) :=
    fun (effects, currentState) message =>
      let (((), nextEffects), nextState) :=
        (handleEventLoopTaskMessage message).run currentState
      (effects ++ nextEffects, nextState)
  change
    (match List.foldl step (#[], state) messages with
      | (queueEffects, queuedState) =>
          let queuedState := coalesceQueuedHighFrequencyWork queuedState
          match runNextQueuedTaskM queuedState with
          | (((), runEffects), nextState) => (queueEffects ++ runEffects, nextState)).snd =
      (runNextQueuedTaskM (coalesceQueueState (queueMessagesState state messages))).snd
  generalize hfold : List.foldl step (#[], state) messages = folded
  cases folded with
  | mk queueEffects queuedState =>
      have hqueuedState :
          queuedState = queueMessagesState state messages := by
        simpa [step, hfold] using queueEffectsFold_state #[] state messages
      rw [← hqueuedState]
      rfl

theorem runEventLoopMessagesMonadic_trace
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      (eventLoopMessageActions messages)
      (runEventLoopMessagesMonadic state messages).2 := by
  let queuedState := queueMessagesState state messages
  let coalescedState := coalesceQueueState queuedState
  have hqueue :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state
        (messages.map EventLoopAction.handleMessage)
        queuedState := by
    simpa [queuedState] using handleMessages_trace state messages
  have hcoalesce :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        queuedState
        [.coalesceQueue]
        coalescedState := by
    simpa [coalescedState] using coalesceQueue_trace queuedState
  have hschedule :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        coalescedState
        [.scheduleNextTask]
        (runNextQueuedTaskM coalescedState).2 := by
    exact scheduleNextTask_trace coalescedState
  have hqueueCoalesced :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state
        (messages.map EventLoopAction.handleMessage ++ [.coalesceQueue])
        coalescedState := by
    exact TransitionSystem.TransitionTrace.append hqueue hcoalesce
  have hfull :
      TransitionSystem.TransitionTrace
        eventLoopLTS
        state
        ((messages.map EventLoopAction.handleMessage ++ [.coalesceQueue]) ++ [.scheduleNextTask])
        (runNextQueuedTaskM coalescedState).2 := by
    exact TransitionSystem.TransitionTrace.append hqueueCoalesced hschedule
  have hactions :
      eventLoopMessageActions messages =
        ((messages.map EventLoopAction.handleMessage ++ [.coalesceQueue]) ++ [.scheduleNextTask]) := by
    simp [eventLoopMessageActions, List.append_assoc]
  have hstate :
      (runEventLoopMessagesMonadic state messages).2 = (runNextQueuedTaskM coalescedState).2 := by
    simpa [queuedState, coalescedState] using runEventLoopMessagesMonadic_state state messages
  rw [hactions, hstate]
  exact hfull

theorem runEventLoopMonadic_trace
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    TransitionSystem.TransitionTrace
      eventLoopLTS
      state
      (eventLoopMessageActions [message])
      (runEventLoopMonadic state message).2 := by
  simpa [runEventLoopMonadic] using runEventLoopMessagesMonadic_trace state [message]

theorem createLoadedDocument_preserves_navigation_response_metadata :
    (runEventLoopMonadic
      { eventLoop := { id := 3 } }
      (.createLoadedDocument
        5
        { id := 7 }
        {
          url := "https://example.test/final"
          status := 201
          contentType := "text/html; charset=utf-8"
          body := "<p>ok</p>"
        })).1 =
      #[EventLoopEffect.runNextTask
        {
          step := .createLoadedDocument
          documentId := some 7
        }
        (some (.createLoadedDocument
          5
          { id := 7 }
          {
            url := "https://example.test/final"
            status := 201
            contentType := "text/html; charset=utf-8"
            body := "<p>ok</p>"
          }))] := by
  native_decide

theorem queueDocumentFetchCompletion_preserves_fetch_response_metadata :
    (runEventLoopMonadic
      {
        eventLoop := { id := 3 }
        pendingDocumentFetchRequests :=
          [{
            handler := { raw := 9 }
            request := {
              url := "https://example.test/script.js"
            }
          }]
      }
      (.queueDocumentFetchCompletion
        { raw := 9 }
        {
          url := "https://example.test/script.js"
          status := 404
          contentType := "text/html"
          body := "<html>missing</html>".toUTF8
        })).1 =
      #[
        EventLoopEffect.performRuntimeEffect (.clearTimeout 9),
        EventLoopEffect.runNextTask
          {
            step := .completeDocumentFetch
          }
          (some (.documentFetchCompletion
            { raw := 9 }
            {
              url := "https://example.test/script.js"
              status := 404
              contentType := "text/html"
              body := "<html>missing</html>".toUTF8
            }))
      ] := by
  native_decide

end FormalWeb
