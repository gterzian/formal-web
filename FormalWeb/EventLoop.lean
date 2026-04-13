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
  /-- Model-local task step queued when the user agent requests a paint for the active document. -/
  | paint
  /-- Model-local task step queued when a document-driven fetch result is handed back to Rust. -/
  | completeDocumentFetch
  | opaque
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
  /-- Model-local dedup flag: an UpdateTheRendering task is already queued, so further requests are no-ops until it runs. -/
  hasPendingUpdateTheRendering : Bool := false
deriving Repr, DecidableEq

instance : Inhabited EventLoop where
  default := { id := 0 }

inductive EventLoopTaskMessage where
  | createEmptyDocument (documentId : DocumentId)
  | createLoadedDocument (documentId : DocumentId) (url : String) (body : String)
  | queueUpdateTheRendering (traversableId : Nat) (documentId : DocumentId)
  | queueDispatchEvent (documentId : DocumentId) (event : String)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | queuePaint (documentId : DocumentId)
  | queueDocumentFetchCompletion
      (handler : RustNetHandlerPointer)
      (resolvedUrl : String)
      (body : ByteArray)
deriving Repr, DecidableEq

structure EventLoopTaskCompletion where
  traversableId : Nat
  eventLoopId : Nat
  documentId : DocumentId
deriving Repr, DecidableEq

namespace EventLoop

def enqueueTask
    (eventLoop : EventLoop)
    (task : Task) :
    EventLoop :=
  {
    eventLoop with
      taskQueue := eventLoop.taskQueue.concat task
  }

/-- Enqueue an UpdateTheRendering task, deduplicating: a second enqueue is a no-op if one is already pending. -/
def enqueueUpdateTheRenderingTask (eventLoop : EventLoop) : EventLoop :=
  if eventLoop.hasPendingUpdateTheRendering then
    eventLoop
  else
    let updated := eventLoop.enqueueTask { step := .updateTheRendering }
    { updated with hasPendingUpdateTheRendering := true }

/-- Remove the next task from the queue, clearing the pending-render flag when that task is UpdateTheRendering. -/
def takeNextTask?
    (eventLoop : EventLoop) :
    Option (Task × EventLoop) :=
  match eventLoop.taskQueue with
  | [] => none
  | task :: remainingTasks =>
      let hasPendingUpdateTheRendering :=
        if task.step = .updateTheRendering then
          false
        else
          eventLoop.hasPendingUpdateTheRendering
      some (task, { eventLoop with taskQueue := remainingTasks, hasPendingUpdateTheRendering })

/-- Dequeue the UpdateTheRendering task and clear the pending flag. -/
def dequeueUpdateTheRenderingTask (eventLoop : EventLoop) : EventLoop :=
  { eventLoop with
      taskQueue := eventLoop.taskQueue.filter (fun task => task.step ≠ .updateTheRendering)
      hasPendingUpdateTheRendering := false }

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
  traversableId : Nat
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

structure PendingPaintTask where
  documentId : DocumentId
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
  pendingPaintTasks : List PendingPaintTask := []
  pendingDocumentFetchCompletionTasks : List PendingDocumentFetchCompletionTask := []

instance : Inhabited EventLoopTaskState where
  default := { eventLoop := { id := 0 } }

inductive EventLoopRuntimeEffect where
  | createEmptyDocument (documentId : DocumentId)
  | createLoadedDocument (documentId : DocumentId) (url : String) (body : String)
  | updateTheRendering (completion : EventLoopTaskCompletion)
  | dispatchEvent (documentId : DocumentId) (event : String)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | paint (documentId : DocumentId)
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
      let baseState := { state with eventLoop }
      let (runtimeEffect?, nextState) :=
        match task.step with
        | .createEmptyDocument =>
            match state.pendingCreateEmptyDocumentTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                (some (.createEmptyDocument pendingTask.documentId),
                  { baseState with pendingCreateEmptyDocumentTasks := pendingTasks })
        | .createLoadedDocument =>
            match state.pendingCreateLoadedDocumentTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                (some (.createLoadedDocument pendingTask.documentId pendingTask.url pendingTask.body),
                  { baseState with pendingCreateLoadedDocumentTasks := pendingTasks })
        | .updateTheRendering =>
            match state.pendingUpdateTheRenderingTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                let completion : EventLoopTaskCompletion := {
                  traversableId := pendingTask.traversableId
                  eventLoopId := eventLoop.id
                  documentId := pendingTask.documentId
                }
                (some (.updateTheRendering completion),
                  { baseState with pendingUpdateTheRenderingTasks := pendingTasks })
        | .dispatchEvent =>
            match state.pendingDispatchEventTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                (some (.dispatchEvent pendingTask.documentId pendingTask.event),
                  { baseState with pendingDispatchEventTasks := pendingTasks })
        | .runBeforeUnload =>
          match state.pendingRunBeforeUnloadTasks with
          | [] =>
            (none, baseState)
          | pendingTask :: pendingTasks =>
            (some (.runBeforeUnload pendingTask.documentId pendingTask.checkId),
              { baseState with pendingRunBeforeUnloadTasks := pendingTasks })
        | .paint =>
            match state.pendingPaintTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                (some (.paint pendingTask.documentId),
                  { baseState with pendingPaintTasks := pendingTasks })
        | .completeDocumentFetch =>
            match state.pendingDocumentFetchCompletionTasks with
            | [] =>
                (none, baseState)
            | pendingTask :: pendingTasks =>
                (some (.documentFetchCompletion pendingTask.handler pendingTask.resolvedUrl pendingTask.body),
                  { baseState with pendingDocumentFetchCompletionTasks := pendingTasks })
        | _ =>
            (none, baseState)
      (((), #[EventLoopEffect.runNextTask task runtimeEffect?]), nextState)

def enqueueAndRunNext
    (state : EventLoopTaskState)
    (task : Task) :
    Array EventLoopEffect × EventLoopTaskState :=
  let (((), nextEffects), nextState) := runNextQueuedTaskM state
  (#[EventLoopEffect.queueTask task] ++ nextEffects, nextState)

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
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
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
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
  | .queueUpdateTheRendering traversableId documentId =>
      let task : Task := { step := .updateTheRendering }
      let shouldAppendTask := !state.eventLoop.hasPendingUpdateTheRendering
      let pendingUpdateTheRenderingTasks :=
        if shouldAppendTask then
          state.pendingUpdateTheRenderingTasks.concat { traversableId, documentId }
        else
          state.pendingUpdateTheRenderingTasks
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueUpdateTheRenderingTask
          pendingUpdateTheRenderingTasks
      }
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
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
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
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
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
  | .queuePaint documentId =>
      let task : Task := {
        step := .paint
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingPaintTasks := state.pendingPaintTasks.concat { documentId }
      }
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)
  | .queueDocumentFetchCompletion handler resolvedUrl body =>
      let task : Task := { step := .completeDocumentFetch }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDocumentFetchCompletionTasks :=
            state.pendingDocumentFetchCompletionTasks.concat { handler, resolvedUrl, body }
      }
      let (effects, nextState) := enqueueAndRunNext state task
      (((), effects), nextState)

def runEventLoopMonadic
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    Array EventLoopEffect × EventLoopTaskState :=
  let (((), effects), nextState) :=
    (handleEventLoopTaskMessage message).run state
  (effects, nextState)

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

def runEventLoopMessage
    (reportCompletion : EventLoopTaskCompletion → IO Unit)
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    IO EventLoopTaskState := do
  let (effects, nextState) := runEventLoopMonadic state message
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
        | some (.updateTheRendering completion) =>
            reportCompletion completion
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
        | some (.paint documentId) =>
            contentProcessUpdateTheRendering
              nextState.contentProcess.raw
              (USize.ofNat documentId.id)
        | some (.documentFetchCompletion handler resolvedUrl body) =>
            contentProcessCompleteDocumentFetch
              nextState.contentProcess.raw
              handler.raw
              resolvedUrl
              body
  pure nextState

partial def runEventLoopLoop
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (reportCompletion : EventLoopTaskCompletion → IO Unit)
    (state : EventLoopTaskState) :
    IO Unit := do
  let some message ← recvCloseableChannel? channel | pure ()
  let state ← runEventLoopMessage reportCompletion state message
  runEventLoopLoop channel reportCompletion state

def runEventLoop
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (reportCompletion : EventLoopTaskCompletion → IO Unit)
    (state : EventLoopTaskState) :
    IO Unit := do
  let contentProcess ← contentProcessStart (USize.ofNat state.eventLoop.id)
  let state := { state with contentProcess }
  try
    runEventLoopLoop channel reportCompletion state
  finally
    if contentProcess.raw ≠ 0 then
      contentProcessStop contentProcess.raw

end FormalWeb
