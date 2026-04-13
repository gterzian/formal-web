import Std.Data.TreeMap
import Std.Sync.Channel
import Mathlib.Control.Monad.Writer
import FormalWeb.Document
import FormalWeb.FFI
import FormalWeb.Fetch

namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/#task-source -/
inductive TaskSource
  | generic
deriving Repr, DecidableEq

/-- Model-local summary of the work stored in https://html.spec.whatwg.org/multipage/#concept-task-steps -/
inductive TaskStep
  | createEmptyDocument
  | createLoadedDocument
  | completeNav (navigationId : Nat)
  /-- Model-local UpdateTheRendering task step queued when rendering should be updated. -/
  | updateTheRendering
  /-- Model-local task step queued when the embedder runtime dispatches a serialized UI event. -/
  | dispatchEvent
  /-- Model-local task step queued when the user agent runs the `beforeunload` steps for a document. -/
  | runBeforeUnload
  /-- Model-local task step queued when a document-driven fetch result is handed back to Rust. -/
  | completeDocumentFetch
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#concept-task -/
structure Task where
  /-- Model-local summary of https://html.spec.whatwg.org/multipage/#concept-task-steps -/
  step : TaskStep
  /-- https://html.spec.whatwg.org/multipage/#concept-task-source -/
  source : TaskSource := .generic
  /-- Model-local reference for https://html.spec.whatwg.org/multipage/#concept-task-document -/
  documentId : Option Nat := none
  /-- Model-local placeholder for https://html.spec.whatwg.org/multipage/#script-evaluation-environment-settings-object-set -/
  scriptEvaluationEnvironmentSettingsObjectSet : List Nat := []
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#event-loop -/
structure EventLoop where
  /-- Model-local identifier for https://html.spec.whatwg.org/multipage/#event-loop -/
  id : Nat
  /-- Model-local collapse of https://html.spec.whatwg.org/multipage/#task-queue to a single queue containing https://html.spec.whatwg.org/multipage/#concept-task values. -/
  taskQueue : List Task := []
  /-- https://html.spec.whatwg.org/multipage/#termination-nesting-level -/
  terminationNestingLevel : Nat := 0
  /-- Model-local flag: the content process is still handling the previously emitted task effect, so the event loop must wait for a wake-up message before emitting another one. -/
  awaitingTaskCompletion : Bool := false
deriving Repr, DecidableEq

instance : Inhabited EventLoop where
  default := { id := 0 }

inductive EventLoopTaskMessage where
  | createEmptyDocument (documentId : DocumentId)
  | createLoadedDocument (documentId : DocumentId) (url : String) (body : String)
  | queueUpdateTheRendering (traversableId : Nat) (documentId : DocumentId)
  | queueDispatchEvent (documentId : DocumentId) (event : String)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | runNextTask
  | queueDocumentFetchCompletion
      (handler : RustNetHandlerPointer)
      (resolvedUrl : String)
      (body : ByteArray)
deriving Repr, DecidableEq

namespace EventLoop

private def dropQueuedUpdateTheRenderingTasks
    (tasks : List Task) : List Task :=
  tasks.filter (fun task => task.step ≠ .updateTheRendering)

def enqueueTask
    (eventLoop : EventLoop)
    (task : Task) :
    EventLoop :=
  {
    eventLoop with
      taskQueue := eventLoop.taskQueue.concat task
  }


def wakeForNextTask (eventLoop : EventLoop) : EventLoop :=
  { eventLoop with awaitingTaskCompletion := false }

/-- Remove the next task from the queue. -/
def takeNextTask?
    (eventLoop : EventLoop) :
    Option (Task × EventLoop) :=
  if eventLoop.awaitingTaskCompletion then
    none
  else
    match eventLoop.taskQueue with
    | [] => none
    | task :: remainingTasks =>
        let remainingTasks :=
          if task.step = .updateTheRendering then
            dropQueuedUpdateTheRenderingTasks remainingTasks
          else
            remainingTasks
        some (task, {
          eventLoop with
            taskQueue := remainingTasks
            awaitingTaskCompletion := true
        })

end EventLoop

structure PendingCreateEmptyDocumentTask where
  documentId : DocumentId
deriving DecidableEq

structure PendingCreateLoadedDocumentTask where
  documentId : DocumentId
  url : String
  body : String
deriving DecidableEq

/-- Model-local runtime payload for an UpdateTheRendering task. -/
structure PendingUpdateTheRenderingTask where
  documentId : DocumentId
deriving DecidableEq

structure PendingDispatchEventTask where
  documentId : DocumentId
  event : String
deriving DecidableEq

structure PendingRunBeforeUnloadTask where
  documentId : DocumentId
  checkId : Nat
deriving DecidableEq

structure PendingDocumentFetchCompletionTask where
  handler : RustNetHandlerPointer
  resolvedUrl : String
  body : ByteArray
deriving DecidableEq

structure EventLoopTaskState where
  eventLoop : EventLoop
  contentProcess : RustContentProcessPointer := { raw := 0 }
  pendingCreateEmptyDocumentTasks : List PendingCreateEmptyDocumentTask := []
  pendingCreateLoadedDocumentTasks : List PendingCreateLoadedDocumentTask := []
  pendingUpdateTheRenderingTasks : List PendingUpdateTheRenderingTask := []
  pendingDispatchEventTasks : List PendingDispatchEventTask := []
  pendingRunBeforeUnloadTasks : List PendingRunBeforeUnloadTask := []
  pendingDocumentFetchCompletionTasks : List PendingDocumentFetchCompletionTask := []

instance : Inhabited EventLoopTaskState where
  default := { eventLoop := { id := 0 } }

inductive EventLoopRuntimeEffect where
  | createEmptyDocument (documentId : DocumentId)
  | createLoadedDocument (documentId : DocumentId) (url : String) (body : String)
  | updateTheRendering (documentId : DocumentId)
  | dispatchEvent (documentId : DocumentId) (event : String)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | documentFetchCompletion
      (handler : RustNetHandlerPointer)
      (resolvedUrl : String)
      (body : ByteArray)
deriving Repr, DecidableEq

inductive EventLoopEffect where
  | queueTask (task : Task)
  | runNextTask (task : Task) (runtimeEffect? : Option EventLoopRuntimeEffect)
deriving Repr, DecidableEq

abbrev EventLoopM := WriterT (Array EventLoopEffect) (StateM EventLoopTaskState)

namespace EventLoopM

def emit (effect : EventLoopEffect) : EventLoopM Unit :=
  tell #[effect]

def queueTask (task : Task) : EventLoopM Unit :=
  emit (.queueTask task)

def runNextTask (task : Task) (runtimeEffect? : Option EventLoopRuntimeEffect) : EventLoopM Unit :=
  emit (.runNextTask task runtimeEffect?)

end EventLoopM

def runNextQueuedTaskM : EventLoopM Unit := fun state =>
  match state.eventLoop.takeNextTask? with
  | none =>
      (((), #[]), state)
  | some (task, eventLoop) =>
      let blockedState := { state with eventLoop }
      let readyState := {
        blockedState with
          eventLoop := blockedState.eventLoop.wakeForNextTask
      }
      let (runtimeEffect?, nextState) :=
        match task.step with
        | .createEmptyDocument =>
            match state.pendingCreateEmptyDocumentTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.createEmptyDocument pendingTask.documentId),
                  { blockedState with pendingCreateEmptyDocumentTasks := pendingTasks })
        | .createLoadedDocument =>
            match state.pendingCreateLoadedDocumentTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.createLoadedDocument pendingTask.documentId pendingTask.url pendingTask.body),
                  { blockedState with pendingCreateLoadedDocumentTasks := pendingTasks })
        | .updateTheRendering =>
            match state.pendingUpdateTheRenderingTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: _ =>
                (some (.updateTheRendering pendingTask.documentId),
                  { blockedState with pendingUpdateTheRenderingTasks := [] })
        | .dispatchEvent =>
            match state.pendingDispatchEventTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.dispatchEvent pendingTask.documentId pendingTask.event),
                  { blockedState with pendingDispatchEventTasks := pendingTasks })
        | .runBeforeUnload =>
          match state.pendingRunBeforeUnloadTasks with
          | [] =>
            (none, readyState)
          | pendingTask :: pendingTasks =>
            (some (.runBeforeUnload pendingTask.documentId pendingTask.checkId),
              { blockedState with pendingRunBeforeUnloadTasks := pendingTasks })
        | .completeDocumentFetch =>
            match state.pendingDocumentFetchCompletionTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.documentFetchCompletion pendingTask.handler pendingTask.resolvedUrl pendingTask.body),
                  { blockedState with pendingDocumentFetchCompletionTasks := pendingTasks })
        | _ =>
            (none, readyState)
      (((), #[EventLoopEffect.runNextTask task runtimeEffect?]), nextState)

def handleEventLoopTaskMessage
    (message : EventLoopTaskMessage) :
    EventLoopM Unit := fun state =>
  match message with
  | .createEmptyDocument documentId =>
      let task : Task := {
        step := .createEmptyDocument
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCreateEmptyDocumentTasks :=
            state.pendingCreateEmptyDocumentTasks.concat { documentId }
      }
      (((), #[EventLoopEffect.queueTask task]), state)
  | .createLoadedDocument documentId url body =>
      let task : Task := {
        step := .createLoadedDocument
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCreateLoadedDocumentTasks :=
            state.pendingCreateLoadedDocumentTasks.concat { documentId, url, body }
      }
      (((), #[EventLoopEffect.queueTask task]), state)
  | .queueUpdateTheRendering _ documentId =>
      let task : Task := { step := .updateTheRendering }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingUpdateTheRenderingTasks :=
            state.pendingUpdateTheRenderingTasks.concat { documentId }
      }
      (((), #[EventLoopEffect.queueTask task]), state)
  | .queueDispatchEvent documentId event =>
      let task : Task := {
        step := .dispatchEvent
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDispatchEventTasks := state.pendingDispatchEventTasks.concat { documentId, event }
      }
      (((), #[EventLoopEffect.queueTask task]), state)
  | .runBeforeUnload documentId checkId =>
      let task : Task := {
        step := .runBeforeUnload
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingRunBeforeUnloadTasks :=
            state.pendingRunBeforeUnloadTasks.concat { documentId, checkId }
      }
      (((), #[EventLoopEffect.queueTask task]), state)
  | .runNextTask =>
      let state := {
        state with
          eventLoop := state.eventLoop.wakeForNextTask
      }
      (((), #[]), state)
  | .queueDocumentFetchCompletion handler resolvedUrl body =>
      let task : Task := { step := .completeDocumentFetch }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDocumentFetchCompletionTasks :=
            state.pendingDocumentFetchCompletionTasks.concat { handler, resolvedUrl, body }
      }
      (((), #[EventLoopEffect.queueTask task]), state)

def runEventLoopMessagesMonadic
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    Array EventLoopEffect × EventLoopTaskState :=
  let (queueEffects, queuedState) :=
    messages.foldl
      (fun (effects, currentState) message =>
        let (((), nextEffects), nextState) :=
          (handleEventLoopTaskMessage message).run currentState
        (effects ++ nextEffects, nextState))
      (#[], state)
  let (((), runEffects), nextState) := runNextQueuedTaskM queuedState
  (queueEffects ++ runEffects, nextState)

def runEventLoopMonadic
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    Array EventLoopEffect × EventLoopTaskState :=
  runEventLoopMessagesMonadic state [message]

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private partial def drainCloseableChannel
    (channel : Std.CloseableChannel α)
    (reversedMessages : List α := []) :
    IO (List α) := do
  match ← channel.tryRecv with
  | some message =>
      drainCloseableChannel channel (message :: reversedMessages)
  | none =>
      pure reversedMessages.reverse

private def recvDrainedMessages?
    (channel : Std.CloseableChannel α) :
    IO (Option (List α)) := do
  let some firstMessage ← recvCloseableChannel? channel | pure none
  let drainedMessages ← drainCloseableChannel channel
  pure (some (firstMessage :: drainedMessages))

def runEventLoopMessages
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    IO EventLoopTaskState := do
  let (effects, nextState) := runEventLoopMessagesMonadic state messages
  for effect in effects do
    match effect with
    | .queueTask _ =>
        pure ()
    | .runNextTask _ runtimeEffect? =>
        match runtimeEffect? with
        | none =>
            pure ()
        | some (.createEmptyDocument documentId) =>
            contentProcessCreateEmptyDocument nextState.contentProcess.raw (USize.ofNat documentId.id)
        | some (.createLoadedDocument documentId url body) =>
            contentProcessCreateLoadedDocument
              nextState.contentProcess.raw
              (USize.ofNat documentId.id)
              url
              body
        | some (.updateTheRendering documentId) =>
            contentProcessUpdateTheRendering
              nextState.contentProcess.raw
              (USize.ofNat documentId.id)
        | some (.dispatchEvent documentId event) =>
            contentProcessDispatchEvent
              nextState.contentProcess.raw
              (USize.ofNat documentId.id)
              event
        | some (.runBeforeUnload documentId checkId) =>
            contentProcessRunBeforeUnload
              nextState.contentProcess.raw
              (USize.ofNat documentId.id)
              (USize.ofNat checkId)
        | some (.documentFetchCompletion handler resolvedUrl body) =>
            contentProcessCompleteDocumentFetch
              nextState.contentProcess.raw
              handler.raw
              resolvedUrl
              body
  pure nextState

def runEventLoopMessage
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    IO EventLoopTaskState := do
  runEventLoopMessages state [message]

partial def runEventLoopLoop
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (state : EventLoopTaskState) :
    IO Unit := do
  let some messages ← recvDrainedMessages? channel | pure ()
  let state ← runEventLoopMessages state messages
  runEventLoopLoop channel state

def runEventLoop
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (state : EventLoopTaskState) :
    IO Unit := do
  let contentProcess ← contentProcessStart (USize.ofNat state.eventLoop.id)
  let state := { state with contentProcess }
  try
    runEventLoopLoop channel state
  finally
    if contentProcess.raw ≠ 0 then
      contentProcessStop contentProcess.raw

end FormalWeb
