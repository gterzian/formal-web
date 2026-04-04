import Std.Data.TreeMap
import Std.Sync.Channel
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
  | createEmptyDocument (documentId : RustDocumentHandle)
  | createLoadedDocument (documentId : RustDocumentHandle) (url : String) (body : String)
  | queueUpdateTheRendering (traversableId : Nat) (documentId : RustDocumentHandle)
  | queueDispatchEvent (documentId : RustDocumentHandle) (event : String)
  | queuePaint (documentId : RustDocumentHandle)
  | queueDocumentFetchCompletion
      (handler : RustNetHandlerPointer)
      (resolvedUrl : String)
      (body : ByteArray)
deriving Repr, DecidableEq

structure EventLoopTaskCompletion where
  traversableId : Nat
  eventLoopId : Nat
  documentId : RustDocumentHandle
deriving DecidableEq

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
  documentId : RustDocumentHandle
deriving DecidableEq

structure PendingCreateLoadedDocumentTask where
  documentId : RustDocumentHandle
  url : String
  body : String
deriving DecidableEq

/-- Model-local runtime payload for an UpdateTheRendering task. -/
structure PendingUpdateTheRenderingTask where
  traversableId : Nat
  documentId : RustDocumentHandle
deriving DecidableEq

structure PendingDispatchEventTask where
  documentId : RustDocumentHandle
  event : String
deriving DecidableEq

structure PendingPaintTask where
  documentId : RustDocumentHandle
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
  pendingPaintTasks : List PendingPaintTask := []
  pendingDocumentFetchCompletionTasks : List PendingDocumentFetchCompletionTask := []

instance : Inhabited EventLoopTaskState where
  default := { eventLoop := { id := 0 } }

structure CreateEmptyDocumentEffect where
  documentId : RustDocumentHandle
deriving DecidableEq

structure CreateLoadedDocumentEffect where
  documentId : RustDocumentHandle
  url : String
  body : String
deriving DecidableEq

structure UpdateTheRenderingEffect where
  traversableId : Nat
  eventLoopId : Nat
  documentId : RustDocumentHandle
deriving DecidableEq

structure DispatchEventEffect where
  documentId : RustDocumentHandle
  event : String
deriving DecidableEq

structure PaintEffect where
  documentId : RustDocumentHandle
deriving DecidableEq

structure DocumentFetchCompletionEffect where
  handler : RustNetHandlerPointer
  resolvedUrl : String
  body : ByteArray
deriving DecidableEq

structure EventLoopTaskResult where
  state : EventLoopTaskState
  createEmptyDocumentEffects : List CreateEmptyDocumentEffect := []
  createLoadedDocumentEffects : List CreateLoadedDocumentEffect := []
  updateTheRenderingEffects : List UpdateTheRenderingEffect := []
  dispatchEventEffects : List DispatchEventEffect := []
  paintEffects : List PaintEffect := []
  documentFetchCompletionEffects : List DocumentFetchCompletionEffect := []

private def runNextQueuedTask
    (state : EventLoopTaskState) :
    EventLoopTaskResult :=
  match state.eventLoop.takeNextTask? with
  | none =>
      { state }
  | some (task, eventLoop) =>
      let state := { state with eventLoop }
      match task.step with
      | .createEmptyDocument =>
          match state.pendingCreateEmptyDocumentTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingCreateEmptyDocumentTasks := pendingTasks
                }
                createEmptyDocumentEffects := [{ documentId := pendingTask.documentId }]
              }
      | .createLoadedDocument =>
          match state.pendingCreateLoadedDocumentTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingCreateLoadedDocumentTasks := pendingTasks
                }
                createLoadedDocumentEffects := [{
                  documentId := pendingTask.documentId
                  url := pendingTask.url
                  body := pendingTask.body
                }]
              }
      | .updateTheRendering =>
          match state.pendingUpdateTheRenderingTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingUpdateTheRenderingTasks := pendingTasks
                }
                updateTheRenderingEffects := [{
                  traversableId := pendingTask.traversableId
                  eventLoopId := eventLoop.id
                  documentId := pendingTask.documentId
                }]
              }
      | .dispatchEvent =>
          match state.pendingDispatchEventTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingDispatchEventTasks := pendingTasks
                }
                dispatchEventEffects := [{
                  documentId := pendingTask.documentId
                  event := pendingTask.event
                }]
              }
      | .paint =>
          match state.pendingPaintTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingPaintTasks := pendingTasks
                }
                paintEffects := [{ documentId := pendingTask.documentId }]
              }
      | .completeDocumentFetch =>
          match state.pendingDocumentFetchCompletionTasks with
          | [] =>
              { state }
          | pendingTask :: pendingTasks =>
              {
                state := {
                  state with
                    pendingDocumentFetchCompletionTasks := pendingTasks
                }
                documentFetchCompletionEffects := [{
                  handler := pendingTask.handler
                  resolvedUrl := pendingTask.resolvedUrl
                  body := pendingTask.body
                }]
              }
      | _ =>
          { state }

def handleEventLoopTaskMessagePure
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    EventLoopTaskResult :=
  match message with
  | .createEmptyDocument documentId =>
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask {
            step := .createEmptyDocument
            documentId := some documentId.id
          }
          pendingCreateEmptyDocumentTasks :=
            state.pendingCreateEmptyDocumentTasks.concat { documentId }
      }
      runNextQueuedTask state
  | .createLoadedDocument documentId url body =>
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask {
            step := .createLoadedDocument
            documentId := some documentId.id
          }
          pendingCreateLoadedDocumentTasks :=
            state.pendingCreateLoadedDocumentTasks.concat { documentId, url, body }
      }
      runNextQueuedTask state
  | .queueUpdateTheRendering traversableId documentId =>
      let shouldAppendTask := !state.eventLoop.hasPendingUpdateTheRendering
      let pendingUpdateTheRenderingTasks :=
        if shouldAppendTask then
          state.pendingUpdateTheRenderingTasks.concat {
            traversableId
            documentId
          }
        else
          state.pendingUpdateTheRenderingTasks
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueUpdateTheRenderingTask
          pendingUpdateTheRenderingTasks
      }
      runNextQueuedTask state
  | .queueDispatchEvent documentId event =>
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask {
            step := .dispatchEvent
            documentId := some documentId.id
          }
          pendingDispatchEventTasks := state.pendingDispatchEventTasks.concat { documentId, event }
      }
      runNextQueuedTask state
  | .queuePaint documentId =>
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask {
            step := .paint
            documentId := some documentId.id
          }
          pendingPaintTasks := state.pendingPaintTasks.concat { documentId }
      }
      runNextQueuedTask state
  | .queueDocumentFetchCompletion handler resolvedUrl body =>
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask { step := .completeDocumentFetch }
          pendingDocumentFetchCompletionTasks :=
            state.pendingDocumentFetchCompletionTasks.concat { handler, resolvedUrl, body }
      }
      runNextQueuedTask state

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
  let result := handleEventLoopTaskMessagePure state message
  let mut nextState := result.state
  for effect in result.createEmptyDocumentEffects do
    contentProcessCreateEmptyDocument nextState.contentProcess.raw (USize.ofNat effect.documentId.id)
  for effect in result.createLoadedDocumentEffects do
    contentProcessCreateLoadedDocument
      nextState.contentProcess.raw
      (USize.ofNat effect.documentId.id)
      effect.url
      effect.body
  for effect in result.updateTheRenderingEffects do
    reportCompletion {
      traversableId := effect.traversableId
      eventLoopId := effect.eventLoopId
      documentId := effect.documentId
    }
  for effect in result.dispatchEventEffects do
    contentProcessDispatchEvent
      nextState.contentProcess.raw
      (USize.ofNat effect.documentId.id)
      effect.event
  for effect in result.paintEffects do
    contentProcessUpdateTheRendering
      nextState.contentProcess.raw
      (USize.ofNat effect.documentId.id)
  for effect in result.documentFetchCompletionEffects do
    contentProcessCompleteDocumentFetch
      nextState.contentProcess.raw
      effect.handler.raw
      effect.resolvedUrl
      effect.body
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
