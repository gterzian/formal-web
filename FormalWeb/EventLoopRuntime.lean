import Std.Data.TreeMap
import Std.Sync.Channel
import FormalWeb.Document
import FormalWeb.EventLoop
import FormalWeb.EventLoopMessage
import FormalWeb.FFI
import FormalWeb.Fetch

namespace FormalWeb

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
  documentPointers : Std.TreeMap RustDocumentHandle RustDocumentPointer := Std.TreeMap.empty
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
    let pointer := createEmptyHtmlDocument ()
    nextState := {
      nextState with
        documentPointers := nextState.documentPointers.insert effect.documentId pointer
    }
  for effect in result.createLoadedDocumentEffects do
    let pointer := createLoadedHtmlDocument effect.url effect.body
    nextState := {
      nextState with
        documentPointers := nextState.documentPointers.insert effect.documentId pointer
    }
  for effect in result.updateTheRenderingEffects do
    reportCompletion {
      traversableId := effect.traversableId
      eventLoopId := effect.eventLoopId
      documentId := effect.documentId
    }
  for effect in result.dispatchEventEffects do
    let some documentPointer := nextState.documentPointers.get? effect.documentId | pure ()
    if documentPointer = RustDocumentPointer.null then
      pure ()
    else
      let baseDocumentPointer := extractBaseDocument documentPointer.raw
      applyUiEvent baseDocumentPointer.raw effect.event
  for effect in result.paintEffects do
    let some documentPointer := nextState.documentPointers.get? effect.documentId | pure ()
    if documentPointer = RustDocumentPointer.null then
      pure ()
    else
      let baseDocumentPointer := extractBaseDocument documentPointer.raw
      queuePaint baseDocumentPointer.raw
  for effect in result.documentFetchCompletionEffects do
    completeDocumentFetch effect.handler.raw effect.resolvedUrl effect.body
  pure nextState

partial def runEventLoop
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (reportCompletion : EventLoopTaskCompletion → IO Unit)
    (state : EventLoopTaskState) :
    IO Unit := do
  let some message ← recvCloseableChannel? channel | pure ()
  let state ← runEventLoopMessage reportCompletion state message
  runEventLoop channel reportCompletion state

end FormalWeb
