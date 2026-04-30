import Std.Data.TreeMap
import Std.Sync.Channel
import Mathlib.Control.Monad.Writer
import FormalWeb.Document
import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.Timer

namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/#task-source -/
inductive TaskSource
  | generic
  | timer
deriving Repr, DecidableEq

/-- Model-local summary of the work stored in https://html.spec.whatwg.org/multipage/#concept-task-steps -/
inductive TaskStep
  | createEmptyDocument
  | createLoadedDocument
  | destroyDocument
  | completeNav (navigationId : Nat)
  /-- Model-local UpdateTheRendering task step queued when rendering should be updated. -/
  | updateTheRendering
  /-- Model-local task step queued when the embedder runtime dispatches a serialized UI event. -/
  | dispatchEvent
  /-- Model-local task step queued when the user agent runs the `beforeunload` steps for a document. -/
  | runBeforeUnload
  /-- Model-local task step queued when a document-driven fetch result is handed back to Rust. -/
  | completeDocumentFetch
  /-- Model-local task step queued from the HTML timer task source. -/
  | runWindowTimer
  /-- Model-local task step queued when a document fetch timeout elapses. -/
  | documentFetchTimeout
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
  /-- Model-local allocator state for navigation ids scoped to this event loop. -/
  nextNavigationTaskId : Nat := 0
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
  | createEmptyDocument (traversableId : Nat) (documentId : DocumentId)
  | createLoadedDocument (traversableId : Nat) (documentId : DocumentId) (response : NavigationResponse)
  | destroyDocument (documentId : DocumentId)
  | startNavigation
      (sourceNavigableId : Nat)
      (destinationURL : String)
      (targetName : String)
      (userInvolvement : UserNavigationInvolvement)
      (noopener : Bool)
      (navigationId : Option Nat)
  | queueUpdateTheRendering (traversableId : Nat) (documentId : DocumentId)
  | queueDispatchEvent (documentId : DocumentId) (event : String)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | documentFetchRequested
      (handler : RustNetHandlerPointer)
      (request : NavigationRequest)
    | scheduleWindowTimer
      (documentId : DocumentId)
      (timerId : Nat)
      (timerKey : Nat)
      (timeoutMs : Nat)
      (nestingLevel : Nat)
    | clearTimeout (timerKey : Nat)
  | shutdown
  | runNextTask
  | queueDocumentFetchCompletion
      (handler : RustNetHandlerPointer)
      (response : FetchResponse)
    | queueNavigationContinuation
      (navigationId : Nat)
      (response : NavigationResponse)
    | queueWindowTimerTask
      (documentId : DocumentId)
      (timerId : Nat)
      (timerKey : Nat)
      (nestingLevel : Nat)
    | queueDocumentFetchTimeout (handler : RustNetHandlerPointer)
deriving Repr, DecidableEq

namespace EventLoop

private def scopedNavigationId
    (eventLoopId : Nat)
    (localNavigationId : Nat) : Nat :=
  let sum := eventLoopId + localNavigationId
  (sum * (sum + 1)) / 2 + localNavigationId

def allocateNavigationId
    (eventLoop : EventLoop) :
    Nat × EventLoop :=
  let navigationId := scopedNavigationId eventLoop.id eventLoop.nextNavigationTaskId
  (navigationId, {
    eventLoop with
      nextNavigationTaskId := eventLoop.nextNavigationTaskId + 1
  })

private structure ScheduledTaskRun where
  task : Task
deriving Repr, DecidableEq

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
    Option (ScheduledTaskRun × EventLoop) :=
  if eventLoop.awaitingTaskCompletion then
    none
  else
    match eventLoop.taskQueue with
    | task :: remainingTasks =>
        some ({ task }, {
          eventLoop with
            taskQueue := remainingTasks
            awaitingTaskCompletion := true
        })
    | [] =>
        none

end EventLoop

structure PendingCreateEmptyDocumentTask where
  traversableId : Nat
  documentId : DocumentId
deriving DecidableEq

structure PendingCreateLoadedDocumentTask where
  traversableId : Nat
  documentId : DocumentId
  response : NavigationResponse
deriving DecidableEq

structure PendingDestroyDocumentTask where
  documentId : DocumentId
deriving DecidableEq

/-- Model-local runtime payload for an UpdateTheRendering task. -/
structure PendingUpdateTheRenderingTask where
  traversableId : Nat
  documentId : DocumentId
deriving DecidableEq

structure PendingDispatchEvent where
  documentId : DocumentId
  event : String
deriving Repr, DecidableEq

structure PendingDispatchEventTask where
  events : List PendingDispatchEvent
deriving Repr, DecidableEq

structure PendingDocumentFetchRequest where
  handler : RustNetHandlerPointer
  request : NavigationRequest
deriving Repr, DecidableEq

structure PendingRunBeforeUnloadTask where
  documentId : DocumentId
  checkId : Nat
deriving DecidableEq

structure PendingDocumentFetchCompletionTask where
  handler : RustNetHandlerPointer
  response : FetchResponse
deriving DecidableEq

structure PendingCompleteNavigationTask where
  navigationId : Nat
  response : NavigationResponse
deriving DecidableEq

structure PendingRunWindowTimerTask where
  documentId : DocumentId
  timerId : Nat
  timerKey : Nat
  nestingLevel : Nat
deriving DecidableEq

structure PendingDocumentFetchTimeoutTask where
  handler : RustNetHandlerPointer
deriving DecidableEq

private def dispatchEventKindLabel
    (event : String) : String :=
  if event.startsWith "{\"type\":\"PointerMove\"" then
    "PointerMove"
  else if event.startsWith "{\"type\":\"PointerDown\"" then
    "PointerDown"
  else if event.startsWith "{\"type\":\"PointerUp\"" then
    "PointerUp"
  else if event.startsWith "{\"type\":\"Wheel\"" then
    "Wheel"
  else if event.startsWith "{\"type\":\"KeyDown\"" then
    "KeyDown"
  else if event.startsWith "{\"type\":\"KeyUp\"" then
    "KeyUp"
  else if event.startsWith "{\"type\":\"Ime\"" then
    "Ime"
  else if event.startsWith "{\"type\":\"AppleStandardKeybinding\"" then
    "AppleStandardKeybinding"
  else
    "Other"

private def dispatchEventEntryLabel
    (pendingEvent : PendingDispatchEvent) : String :=
  s!"{pendingEvent.documentId.id}:{dispatchEventKindLabel pendingEvent.event}"

private def dispatchEventBatchLabel
    (events : List PendingDispatchEvent) : String :=
  let entries := events.map dispatchEventEntryLabel
  if entries.isEmpty then
    "<missing>"
  else
    s!"[{String.intercalate ", " entries}]"

private def retainLatestDispatchEventsPerKind
    (events : List PendingDispatchEvent) : List PendingDispatchEvent :=
  let rec go
      (remainingEvents : List PendingDispatchEvent)
      (seenKinds : List String)
      (retainedEvents : List PendingDispatchEvent) :
      List PendingDispatchEvent :=
    match remainingEvents with
    | [] =>
        retainedEvents
    | pendingEvent :: rest =>
        let kind := dispatchEventKindLabel pendingEvent.event
        if seenKinds.elem kind then
          go rest seenKinds retainedEvents
        else
          go rest (kind :: seenKinds) (pendingEvent :: retainedEvents)
  go events.reverse [] []

private def coalesceDispatchEventTasks
    (pendingDispatchEventTasks : List PendingDispatchEventTask) :
    List PendingDispatchEvent :=
  retainLatestDispatchEventsPerKind <|
    pendingDispatchEventTasks.foldl
      (fun queuedEvents pendingTask => queuedEvents ++ pendingTask.events)
      []

private def dropDispatchTasks
    (tasks : List Task)
    : List Task :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .dispatchEvent then
        dropDispatchTasks remainingTasks
      else
        task :: dropDispatchTasks remainingTasks
  | [] =>
      []

private def coalesceDispatchTaskQueue
    (tasks : List Task)
    : List Task :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .dispatchEvent then
        task :: dropDispatchTasks remainingTasks
      else
        task :: coalesceDispatchTaskQueue remainingTasks
  | [] =>
      []

private def dropLeadingUpdateTasks
    (tasks : List Task) :
    List Task :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .updateTheRendering then
        dropLeadingUpdateTasks remainingTasks
      else
        tasks
  | [] =>
      []

private theorem dropLeadingUpdateTasks_length_le
    (tasks : List Task) :
    (dropLeadingUpdateTasks tasks).length ≤ tasks.length := by
  induction tasks with
  | nil =>
      simp [dropLeadingUpdateTasks]
  | cons task remainingTasks ih =>
      by_cases hupdate : task.step = .updateTheRendering
      · simp [dropLeadingUpdateTasks, hupdate]
        exact Nat.le_trans ih (Nat.le_succ _)
      · simp [dropLeadingUpdateTasks, hupdate]

private def coalesceContinuousUpdateTaskQueue
    (tasks : List Task) :
    List Task :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .updateTheRendering then
        task :: coalesceContinuousUpdateTaskQueue (dropLeadingUpdateTasks remainingTasks)
      else
        task :: coalesceContinuousUpdateTaskQueue remainingTasks
  | [] =>
      []
termination_by tasks.length
decreasing_by
  all_goals simp_wf
  exact Nat.lt_of_le_of_lt
    (dropLeadingUpdateTasks_length_le remainingTasks)
    (Nat.lt_succ_self remainingTasks.length)

def coalesceTaskQueue
    (tasks : List Task) :
    List Task :=
  coalesceContinuousUpdateTaskQueue <| coalesceDispatchTaskQueue tasks

private def takeLeadingPendingTasks
    (count : Nat)
    (pendingTasks : List α) :
    List α × List α :=
  (pendingTasks.take count, pendingTasks.drop count)

private def countDispatchTasks
    (tasks : List Task) :
    Nat :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .dispatchEvent then
        countDispatchTasks remainingTasks + 1
      else
        countDispatchTasks remainingTasks
  | [] =>
      0

private def coalesceQueuedPendingDispatchEventTasks
    (tasks : List Task)
    (pendingDispatchEventTasks : List PendingDispatchEventTask) :
    List PendingDispatchEventTask :=
  let dispatchTaskCount := countDispatchTasks tasks
  if dispatchTaskCount = 0 then
    pendingDispatchEventTasks
  else
    let (leadingPendingTasks, remainingPendingTasks) :=
      takeLeadingPendingTasks dispatchTaskCount pendingDispatchEventTasks
    let coalescedPendingTasks :=
      match coalesceDispatchEventTasks leadingPendingTasks with
      | [] =>
          []
      | events =>
          [{ events }]
    coalescedPendingTasks ++ remainingPendingTasks

private def consumeLeadingUpdateRun
    (tasks : List Task)
    (pendingUpdateTheRenderingTasks : List PendingUpdateTheRenderingTask)
    (lastPendingTask? : Option PendingUpdateTheRenderingTask) :
    Option PendingUpdateTheRenderingTask × List Task × List PendingUpdateTheRenderingTask :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .updateTheRendering then
        match pendingUpdateTheRenderingTasks with
        | pendingTask :: remainingPendingTasks =>
            consumeLeadingUpdateRun
              remainingTasks
              remainingPendingTasks
              (some pendingTask)
        | [] =>
            consumeLeadingUpdateRun remainingTasks [] lastPendingTask?
      else
        (lastPendingTask?, tasks, pendingUpdateTheRenderingTasks)
  | [] =>
      (lastPendingTask?, [], pendingUpdateTheRenderingTasks)

private partial def coalesceQueuedPendingUpdateTheRenderingTasks
    (tasks : List Task)
    (pendingUpdateTheRenderingTasks : List PendingUpdateTheRenderingTask) :
    List PendingUpdateTheRenderingTask :=
  match tasks with
  | task :: remainingTasks =>
      if task.step = .updateTheRendering then
        let (firstPendingTask?, remainingPendingTasks) :=
          match pendingUpdateTheRenderingTasks with
          | pendingTask :: nextPendingTasks =>
              (some pendingTask, nextPendingTasks)
          | [] =>
              (none, [])
        let (lastPendingTask?, remainingTasks, remainingPendingTasks) :=
          consumeLeadingUpdateRun
            remainingTasks
            remainingPendingTasks
            firstPendingTask?
        let nextPendingTasks :=
          coalesceQueuedPendingUpdateTheRenderingTasks
            remainingTasks
            remainingPendingTasks
        match lastPendingTask? with
        | some pendingTask =>
            pendingTask :: nextPendingTasks
        | none =>
            nextPendingTasks
      else
        coalesceQueuedPendingUpdateTheRenderingTasks
          remainingTasks
          pendingUpdateTheRenderingTasks
  | [] =>
      pendingUpdateTheRenderingTasks

private abbrev HasUpdateTheRenderingTask
    (tasks : List Task) :
    Prop :=
  ∃ task, task ∈ tasks ∧ task.step = .updateTheRendering

private theorem hasUpdateTheRenderingTask_dropDispatchTasks
    {tasks : List Task}
    (h : HasUpdateTheRenderingTask tasks) :
    HasUpdateTheRenderingTask (dropDispatchTasks tasks) := by
  induction tasks with
  | nil =>
      rcases h with ⟨task, hmem, _⟩
      cases hmem
  | cons task remainingTasks ih =>
      by_cases hdispatch : task.step = .dispatchEvent
      · simp [HasUpdateTheRenderingTask, dropDispatchTasks, hdispatch] at h ⊢
        exact ih h
      · by_cases hupdate : task.step = .updateTheRendering
        · simp [HasUpdateTheRenderingTask, dropDispatchTasks, hupdate]
        · simp [HasUpdateTheRenderingTask, dropDispatchTasks, hdispatch, hupdate] at h ⊢
          exact ih h

private theorem hasUpdateTheRenderingTask_coalesceDispatchTaskQueue
    {tasks : List Task}
    (h : HasUpdateTheRenderingTask tasks) :
    HasUpdateTheRenderingTask (coalesceDispatchTaskQueue tasks) := by
  induction tasks with
  | nil =>
      rcases h with ⟨task, hmem, _⟩
      cases hmem
  | cons task remainingTasks ih =>
      by_cases hdispatch : task.step = .dispatchEvent
      · simp [HasUpdateTheRenderingTask, coalesceDispatchTaskQueue, hdispatch] at h ⊢
        exact hasUpdateTheRenderingTask_dropDispatchTasks h
      · by_cases hupdate : task.step = .updateTheRendering
        · simp [HasUpdateTheRenderingTask, coalesceDispatchTaskQueue, hupdate]
        · simp [HasUpdateTheRenderingTask, coalesceDispatchTaskQueue, hdispatch, hupdate] at h ⊢
          exact ih h

private theorem hasUpdateTheRenderingTask_coalesceContinuousUpdateTaskQueue
    {tasks : List Task}
    (h : HasUpdateTheRenderingTask tasks) :
    HasUpdateTheRenderingTask (coalesceContinuousUpdateTaskQueue tasks) := by
  induction tasks with
  | nil =>
      rcases h with ⟨task, hmem, _⟩
      cases hmem
  | cons task remainingTasks ih =>
      by_cases hupdate : task.step = .updateTheRendering
      · refine ⟨task, ?_, hupdate⟩
        unfold coalesceContinuousUpdateTaskQueue
        simp [hupdate]
      · simp [HasUpdateTheRenderingTask, coalesceContinuousUpdateTaskQueue, hupdate] at h ⊢
        exact ih h

private theorem hasUpdateTheRenderingTask_coalesceTaskQueue
    {tasks : List Task}
    (h : HasUpdateTheRenderingTask tasks) :
    HasUpdateTheRenderingTask (coalesceTaskQueue tasks) :=
  hasUpdateTheRenderingTask_coalesceContinuousUpdateTaskQueue <|
    hasUpdateTheRenderingTask_coalesceDispatchTaskQueue h

theorem coalesceTaskQueue_preserves_head_renderOpportunity
    {headTask : Task}
    {tail : List Task}
    (headNotUpdate : headTask.step ≠ .updateTheRendering)
    (tailHasUpdate : ∃ task, task ∈ tail ∧ task.step = .updateTheRendering) :
    ∃ remainingTasks,
      coalesceTaskQueue (headTask :: tail) = headTask :: remainingTasks ∧
      ∃ task, task ∈ remainingTasks ∧ task.step = .updateTheRendering := by
  by_cases hdispatch : headTask.step = .dispatchEvent
  · refine ⟨coalesceContinuousUpdateTaskQueue (dropDispatchTasks tail), ?_, ?_⟩
    · rw [coalesceTaskQueue]
      rw [show coalesceDispatchTaskQueue (headTask :: tail) = headTask :: dropDispatchTasks tail by
            simp [coalesceDispatchTaskQueue, hdispatch]]
      simp [coalesceContinuousUpdateTaskQueue, headNotUpdate]
    · exact
        hasUpdateTheRenderingTask_coalesceContinuousUpdateTaskQueue <|
          hasUpdateTheRenderingTask_dropDispatchTasks tailHasUpdate
  · refine ⟨coalesceTaskQueue tail, ?_, ?_⟩
    · rw [coalesceTaskQueue]
      rw [show coalesceDispatchTaskQueue (headTask :: tail) = headTask :: coalesceDispatchTaskQueue tail by
            simp [coalesceDispatchTaskQueue, hdispatch]]
      simp [coalesceContinuousUpdateTaskQueue, coalesceTaskQueue, headNotUpdate]
    · exact hasUpdateTheRenderingTask_coalesceTaskQueue tailHasUpdate

private def dropPendingDocumentFetchRequest
    (pendingDocumentFetchRequests : List PendingDocumentFetchRequest)
    (handler : RustNetHandlerPointer) :
    List PendingDocumentFetchRequest :=
  match pendingDocumentFetchRequests with
  | [] =>
      []
  | pendingRequest :: remainingRequests =>
      if pendingRequest.handler.raw = handler.raw then
        remainingRequests
      else
        pendingRequest :: dropPendingDocumentFetchRequest remainingRequests handler

private def hasPendingDocumentFetchRequest
    (pendingDocumentFetchRequests : List PendingDocumentFetchRequest)
    (handler : RustNetHandlerPointer) :
    Bool :=
  pendingDocumentFetchRequests.any fun pendingRequest =>
    pendingRequest.handler.raw = handler.raw

private def takePendingCompleteNavigationTask
    (pendingCompleteNavigationTasks : List PendingCompleteNavigationTask)
    (navigationId : Nat) :
  Option (PendingCompleteNavigationTask × List PendingCompleteNavigationTask) :=
  match pendingCompleteNavigationTasks with
  | [] =>
      none
  | pendingTask :: remainingTasks =>
      if pendingTask.navigationId = navigationId then
        some (pendingTask, remainingTasks)
      else
        match takePendingCompleteNavigationTask remainingTasks navigationId with
        | some (matchedTask, nextRemainingTasks) =>
            some (matchedTask, pendingTask :: nextRemainingTasks)
        | none =>
            none

private def documentFetchTimeoutMilliseconds : Nat :=
  5000

private def dispatchEventBatchSeparator : String :=
  String.singleton (Char.ofNat 30)

private def dispatchEventFieldSeparator : String :=
  String.singleton (Char.ofNat 31)

private def encodeDispatchEventBatchEntry
    (pendingEvent : PendingDispatchEvent) : String :=
  toString pendingEvent.documentId.id ++ dispatchEventFieldSeparator ++ pendingEvent.event

def encodeDispatchEventBatch
    (events : List PendingDispatchEvent) : String :=
  String.intercalate dispatchEventBatchSeparator <|
    events.map encodeDispatchEventBatchEntry

structure EventLoopTaskState where
  eventLoop : EventLoop
  contentProcess : RustContentProcessPointer := { raw := 0 }
  liveDocumentIds : Std.TreeMap Nat Unit := Std.TreeMap.empty
  pendingCreateEmptyDocumentTasks : List PendingCreateEmptyDocumentTask := []
  pendingCreateLoadedDocumentTasks : List PendingCreateLoadedDocumentTask := []
  pendingDestroyDocumentTasks : List PendingDestroyDocumentTask := []
  pendingUpdateTheRenderingTasks : List PendingUpdateTheRenderingTask := []
  pendingDispatchEventTasks : List PendingDispatchEventTask := []
  pendingRunBeforeUnloadTasks : List PendingRunBeforeUnloadTask := []
  pendingDocumentFetchRequests : List PendingDocumentFetchRequest := []
  pendingDocumentFetchCompletionTasks : List PendingDocumentFetchCompletionTask := []
  pendingCompleteNavigationTasks : List PendingCompleteNavigationTask := []
  pendingRunWindowTimerTasks : List PendingRunWindowTimerTask := []
  pendingDocumentFetchTimeoutTasks : List PendingDocumentFetchTimeoutTask := []
  stopRequested : Bool := false

instance : Inhabited EventLoopTaskState where
  default := { eventLoop := { id := 0 } }

inductive EventLoopRuntimeEffect where
  | createEmptyDocument (traversableId : Nat) (documentId : DocumentId)
  | createLoadedDocument (traversableId : Nat) (documentId : DocumentId) (response : NavigationResponse)
  | destroyDocument (documentId : DocumentId)
  | startNavigationInUserAgent
      (eventLoopId : Nat)
    (navigationId : Nat)
      (sourceNavigableId : Nat)
      (destinationURL : String)
      (targetName : String)
      (userInvolvement : UserNavigationInvolvement)
      (noopener : Bool)
  | continueNavigationInUserAgent
    (eventLoopId : Nat)
    (navigationId : Nat)
    (response : NavigationResponse)
  | updateTheRendering (traversableId : Nat) (documentId : DocumentId)
  | dispatchEvent (events : List PendingDispatchEvent)
  | runBeforeUnload (documentId : DocumentId) (checkId : Nat)
  | startDocumentFetch
      (handler : RustNetHandlerPointer)
      (request : NavigationRequest)
    | scheduleTimeout (request : RunStepsAfterTimeoutRequest)
    | clearTimeout (timerKey : Nat)
  | documentFetchCompletion
      (handler : RustNetHandlerPointer)
      (response : FetchResponse)
    | runWindowTimer
      (documentId : DocumentId)
      (timerId : Nat)
      (timerKey : Nat)
      (nestingLevel : Nat)
    | failDocumentFetch (handler : RustNetHandlerPointer)
deriving Repr, DecidableEq

inductive EventLoopEffect where
  | performRuntimeEffect (runtimeEffect : EventLoopRuntimeEffect)
  | runNextTask (task : Task) (runtimeEffect? : Option EventLoopRuntimeEffect)
deriving Repr, DecidableEq

structure EventLoopRuntimeHooks where
  startDocumentFetch : RustNetHandlerPointer -> NavigationRequest -> IO Unit
  startNavigationInUserAgent :
    Nat -> Nat -> Nat -> String -> String -> UserNavigationInvolvement -> Bool -> IO Unit
  continueNavigationInUserAgent :
    Nat -> Nat -> NavigationResponse -> IO Unit
  scheduleTimeout : RunStepsAfterTimeoutRequest -> IO Unit
  clearTimeout : Nat -> IO Unit

def handleRuntimeEffect
    (hooks : EventLoopRuntimeHooks)
    (state : EventLoopTaskState)
    (runtimeEffect : EventLoopRuntimeEffect) :
    IO Unit := do
  match runtimeEffect with
  | .createEmptyDocument traversableId documentId =>
      contentProcessCreateEmptyDocument
        state.contentProcess.raw
        (USize.ofNat traversableId)
        (USize.ofNat documentId.id)
  | .createLoadedDocument traversableId documentId response =>
      contentProcessCreateLoadedDocument
        state.contentProcess.raw
        (USize.ofNat traversableId)
        (USize.ofNat documentId.id)
        response.url
        (USize.ofNat response.status)
        response.contentType
        response.body
  | .destroyDocument documentId =>
      contentProcessDestroyDocument state.contentProcess.raw (USize.ofNat documentId.id)
  | .startNavigationInUserAgent
      eventLoopId
      navigationId
      sourceNavigableId
      destinationURL
      targetName
      userInvolvement
      noopener =>
      hooks.startNavigationInUserAgent
        eventLoopId
        navigationId
        sourceNavigableId
        destinationURL
        targetName
        userInvolvement
        noopener
  | .continueNavigationInUserAgent eventLoopId navigationId response =>
      hooks.continueNavigationInUserAgent
        eventLoopId
        navigationId
        response
  | .updateTheRendering traversableId documentId =>
      contentProcessUpdateTheRendering
        state.contentProcess.raw
        (USize.ofNat traversableId)
        (USize.ofNat documentId.id)
  | .dispatchEvent events =>
      contentProcessDispatchEvent
        state.contentProcess.raw
        (encodeDispatchEventBatch events)
  | .runBeforeUnload documentId checkId =>
      contentProcessRunBeforeUnload
        state.contentProcess.raw
        (USize.ofNat documentId.id)
        (USize.ofNat checkId)
  | .startDocumentFetch handler request =>
      hooks.startDocumentFetch handler request
  | .scheduleTimeout request =>
      hooks.scheduleTimeout request
  | .clearTimeout timerKey =>
      hooks.clearTimeout timerKey
  | .documentFetchCompletion handler response =>
      contentProcessCompleteDocumentFetch
        state.contentProcess.raw
        handler.raw
        response.url
        (USize.ofNat response.status)
        response.contentType
        response.body
  | .runWindowTimer documentId timerId timerKey nestingLevel =>
      contentProcessRunWindowTimer
        state.contentProcess.raw
        (USize.ofNat documentId.id)
        (USize.ofNat timerId)
        (USize.ofNat timerKey)
        (USize.ofNat nestingLevel)
  | .failDocumentFetch handler =>
      contentProcessFailDocumentFetch
        state.contentProcess.raw
        handler.raw

def coalesceQueuedHighFrequencyWork
    (state : EventLoopTaskState) :
    EventLoopTaskState :=
  match state with
  | EventLoopTaskState.mk
      eventLoop
      contentProcess
      liveDocumentIds
      pendingCreateEmptyDocumentTasks
      pendingCreateLoadedDocumentTasks
      pendingDestroyDocumentTasks
      pendingUpdateTheRenderingTasks
      pendingDispatchEventTasks
      pendingRunBeforeUnloadTasks
      pendingDocumentFetchRequests
      pendingDocumentFetchCompletionTasks
      pendingCompleteNavigationTasks
      pendingRunWindowTimerTasks
      pendingDocumentFetchTimeoutTasks
      stopRequested =>
      let taskQueueAfterDispatch :=
        coalesceDispatchTaskQueue eventLoop.taskQueue
      let taskQueue :=
        coalesceContinuousUpdateTaskQueue taskQueueAfterDispatch
      let pendingUpdateTheRenderingTasks :=
        coalesceQueuedPendingUpdateTheRenderingTasks
          taskQueueAfterDispatch
          pendingUpdateTheRenderingTasks
      let pendingDispatchEventTasks :=
        coalesceQueuedPendingDispatchEventTasks
          eventLoop.taskQueue
          pendingDispatchEventTasks
      let eventLoop := { eventLoop with taskQueue := taskQueue }
      EventLoopTaskState.mk
        eventLoop
        contentProcess
        liveDocumentIds
        pendingCreateEmptyDocumentTasks
        pendingCreateLoadedDocumentTasks
        pendingDestroyDocumentTasks
        pendingUpdateTheRenderingTasks
        pendingDispatchEventTasks
        pendingRunBeforeUnloadTasks
        pendingDocumentFetchRequests
        pendingDocumentFetchCompletionTasks
        pendingCompleteNavigationTasks
        pendingRunWindowTimerTasks
        pendingDocumentFetchTimeoutTasks
        stopRequested

abbrev EventLoopM := WriterT (Array EventLoopEffect) (StateM EventLoopTaskState)

namespace EventLoopM

def emit (effect : EventLoopEffect) : EventLoopM Unit :=
  tell #[effect]

def performRuntimeEffect (runtimeEffect : EventLoopRuntimeEffect) : EventLoopM Unit :=
  emit (.performRuntimeEffect runtimeEffect)

def runNextTask (task : Task) (runtimeEffect? : Option EventLoopRuntimeEffect) : EventLoopM Unit :=
  emit (.runNextTask task runtimeEffect?)

end EventLoopM

def runNextQueuedTaskM : EventLoopM Unit := fun state =>
  match state.eventLoop.takeNextTask? with
  | none =>
      (((), #[]), state)
  | some (selectedTask, eventLoop) =>
      let task := selectedTask.task
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
                (some (.createEmptyDocument pendingTask.traversableId pendingTask.documentId),
                  {
                    blockedState with
                      liveDocumentIds := blockedState.liveDocumentIds.insert pendingTask.documentId.id ()
                      pendingCreateEmptyDocumentTasks := pendingTasks
                  })
        | .createLoadedDocument =>
            match state.pendingCreateLoadedDocumentTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.createLoadedDocument pendingTask.traversableId pendingTask.documentId pendingTask.response),
                  {
                    blockedState with
                      liveDocumentIds := blockedState.liveDocumentIds.insert pendingTask.documentId.id ()
                      pendingCreateLoadedDocumentTasks := pendingTasks
                  })
        | .destroyDocument =>
            match state.pendingDestroyDocumentTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.destroyDocument pendingTask.documentId),
                  {
                    blockedState with
                      liveDocumentIds := blockedState.liveDocumentIds.erase pendingTask.documentId.id
                      pendingDestroyDocumentTasks := pendingTasks
                  })
        | .completeNav navigationId =>
            match takePendingCompleteNavigationTask state.pendingCompleteNavigationTasks navigationId with
            | some (pendingTask, pendingTasks) =>
                (some (.continueNavigationInUserAgent
                  blockedState.eventLoop.id
                  navigationId
                  pendingTask.response),
                  {
                    blockedState with
                      pendingCompleteNavigationTasks := pendingTasks
                  })
            | none =>
                (none, readyState)
        | .updateTheRendering =>
            match state.pendingUpdateTheRenderingTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
            (some (.updateTheRendering pendingTask.traversableId pendingTask.documentId),
                  { blockedState with pendingUpdateTheRenderingTasks := pendingTasks })
        | .dispatchEvent =>
            match state.pendingDispatchEventTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                match retainLatestDispatchEventsPerKind pendingTask.events with
                | [] =>
                    (none, readyState)
                | pendingEvents =>
                    (some (.dispatchEvent pendingEvents),
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
                if hasPendingDocumentFetchRequest blockedState.pendingDocumentFetchRequests pendingTask.handler then
                  (some (.documentFetchCompletion pendingTask.handler pendingTask.response),
                    {
                      blockedState with
                        pendingDocumentFetchRequests :=
                          dropPendingDocumentFetchRequest
                            blockedState.pendingDocumentFetchRequests
                            pendingTask.handler
                        pendingDocumentFetchCompletionTasks := pendingTasks
                    })
                else
                  (none,
                    {
                      blockedState with
                        pendingDocumentFetchCompletionTasks := pendingTasks
                    })
        | .runWindowTimer =>
            match state.pendingRunWindowTimerTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                (some (.runWindowTimer pendingTask.documentId pendingTask.timerId pendingTask.timerKey pendingTask.nestingLevel),
                  {
                    blockedState with
                      pendingRunWindowTimerTasks := pendingTasks
                  })
        | .documentFetchTimeout =>
            match state.pendingDocumentFetchTimeoutTasks with
            | [] =>
                (none, readyState)
            | pendingTask :: pendingTasks =>
                if hasPendingDocumentFetchRequest blockedState.pendingDocumentFetchRequests pendingTask.handler then
                  (some (.failDocumentFetch pendingTask.handler),
                    {
                      blockedState with
                        pendingDocumentFetchRequests :=
                          dropPendingDocumentFetchRequest
                            blockedState.pendingDocumentFetchRequests
                            pendingTask.handler
                        pendingDocumentFetchTimeoutTasks := pendingTasks
                    })
                else
                  (none,
                    {
                      blockedState with
                        pendingDocumentFetchTimeoutTasks := pendingTasks
                    })
        (((), #[EventLoopEffect.runNextTask task runtimeEffect?]), nextState)

def handleEventLoopTaskMessage
    (message : EventLoopTaskMessage) :
    EventLoopM Unit := fun state =>
  match message with
  | .createEmptyDocument traversableId documentId =>
      let task : Task := {
        step := .createEmptyDocument
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          liveDocumentIds := state.liveDocumentIds
          pendingCreateEmptyDocumentTasks :=
            state.pendingCreateEmptyDocumentTasks.concat { traversableId, documentId }
      }
      (((), #[]), state)
  | .createLoadedDocument traversableId documentId response =>
      let task : Task := {
        step := .createLoadedDocument
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCreateLoadedDocumentTasks :=
            state.pendingCreateLoadedDocumentTasks.concat { traversableId, documentId, response }
      }
      (((), #[]), state)
  | .destroyDocument documentId =>
      let task : Task := {
        step := .destroyDocument
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDestroyDocumentTasks :=
            state.pendingDestroyDocumentTasks.concat { documentId }
      }
      (((), #[]), state)
  | .startNavigation sourceNavigableId destinationURL targetName userInvolvement noopener navigationId =>
      let (navigationId, eventLoop) :=
        match navigationId with
        | some navigationId =>
            (navigationId, state.eventLoop)
        | none =>
            state.eventLoop.allocateNavigationId
      let state := {
        state with
          eventLoop := eventLoop
      }
      (((), #[EventLoopEffect.performRuntimeEffect (.startNavigationInUserAgent
        state.eventLoop.id
        navigationId
        sourceNavigableId
        destinationURL
        targetName
        userInvolvement
        noopener)]), state)
  | .queueUpdateTheRendering traversableId documentId =>
      let task : Task := { step := .updateTheRendering }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingUpdateTheRenderingTasks :=
            state.pendingUpdateTheRenderingTasks.concat { traversableId, documentId }
      }
      (((), #[]), state)
  | .queueDispatchEvent documentId event =>
      let task : Task := {
        step := .dispatchEvent
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDispatchEventTasks :=
            state.pendingDispatchEventTasks.concat {
              events := [{ documentId, event }]
            }
      }
      (((), #[]), state)
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
      (((), #[]), state)
  | .documentFetchRequested handler request =>
      let state := {
        state with
          pendingDocumentFetchRequests :=
            state.pendingDocumentFetchRequests.concat { handler, request }
      }
      let timeoutRequest : RunStepsAfterTimeoutRequest := {
        timerKey := handler.raw.toNat
        globalId := state.eventLoop.id
        orderingIdentifier := "document-fetch"
        milliseconds := documentFetchTimeoutMilliseconds
        eventLoopId := state.eventLoop.id
        completion := .documentFetchTimeout handler.raw.toNat
      }
      (((), #[
        EventLoopEffect.performRuntimeEffect (.startDocumentFetch handler request),
        EventLoopEffect.performRuntimeEffect (.scheduleTimeout timeoutRequest)
      ]), state)
  | .scheduleWindowTimer documentId timerId timerKey timeoutMs nestingLevel =>
      let request : RunStepsAfterTimeoutRequest := {
        timerKey
        globalId := documentId.id
        orderingIdentifier := "setTimeout/setInterval"
        milliseconds := timeoutMs
        eventLoopId := state.eventLoop.id
        completion := .windowTimerTask documentId.id timerId timerKey nestingLevel
      }
      (((), #[EventLoopEffect.performRuntimeEffect (.scheduleTimeout request)]), state)
  | .clearTimeout timerKey =>
      (((), #[EventLoopEffect.performRuntimeEffect (.clearTimeout timerKey)]), state)
  | .shutdown =>
      let eventLoop := {
        state.eventLoop.wakeForNextTask with
          taskQueue := []
      }
      let state := {
        state with
          eventLoop
          liveDocumentIds := Std.TreeMap.empty
          pendingCreateEmptyDocumentTasks := []
          pendingCreateLoadedDocumentTasks := []
          pendingDestroyDocumentTasks := []
          pendingUpdateTheRenderingTasks := []
          pendingDispatchEventTasks := []
          pendingRunBeforeUnloadTasks := []
          pendingDocumentFetchRequests := []
          pendingDocumentFetchCompletionTasks := []
          pendingCompleteNavigationTasks := []
          pendingRunWindowTimerTasks := []
          pendingDocumentFetchTimeoutTasks := []
          stopRequested := true
      }
      (((), #[]), state)
  | .runNextTask =>
      let state := {
        state with
          eventLoop := state.eventLoop.wakeForNextTask
      }
      (((), #[]), state)
  | .queueDocumentFetchCompletion handler response =>
      let task : Task := { step := .completeDocumentFetch }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDocumentFetchCompletionTasks :=
            state.pendingDocumentFetchCompletionTasks.concat { handler, response }
      }
      (((), #[EventLoopEffect.performRuntimeEffect (.clearTimeout handler.raw.toNat)]), state)
  | .queueNavigationContinuation navigationId response =>
      let task : Task := { step := .completeNav navigationId }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingCompleteNavigationTasks :=
            state.pendingCompleteNavigationTasks.concat { navigationId, response }
      }
      (((), #[]), state)
  | .queueWindowTimerTask documentId timerId timerKey nestingLevel =>
      let task : Task := {
        step := .runWindowTimer
        source := .timer
        documentId := some documentId.id
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingRunWindowTimerTasks :=
            state.pendingRunWindowTimerTasks.concat { documentId, timerId, timerKey, nestingLevel }
      }
      (((), #[]), state)
  | .queueDocumentFetchTimeout handler =>
      let task : Task := {
        step := .documentFetchTimeout
        source := .timer
      }
      let state := {
        state with
          eventLoop := state.eventLoop.enqueueTask task
          pendingDocumentFetchTimeoutTasks :=
            state.pendingDocumentFetchTimeoutTasks.concat { handler }
      }
      (((), #[]), state)

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
  let queuedState := coalesceQueuedHighFrequencyWork queuedState
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
    (performRuntimeEffect : EventLoopTaskState -> EventLoopRuntimeEffect -> IO Unit)
    (state : EventLoopTaskState)
    (messages : List EventLoopTaskMessage) :
    IO EventLoopTaskState := do
  let (effects, nextState) := runEventLoopMessagesMonadic state messages
  for effect in effects do
    match effect with
    | .performRuntimeEffect runtimeEffect =>
        performRuntimeEffect nextState runtimeEffect
    | .runNextTask _ runtimeEffect? =>
        match runtimeEffect? with
        | none =>
            pure ()
        | some runtimeEffect =>
            performRuntimeEffect nextState runtimeEffect
  pure nextState

def runEventLoopMessage
    (performRuntimeEffect : EventLoopTaskState -> EventLoopRuntimeEffect -> IO Unit)
    (state : EventLoopTaskState)
    (message : EventLoopTaskMessage) :
    IO EventLoopTaskState := do
  runEventLoopMessages performRuntimeEffect state [message]

partial def runEventLoopLoop
    (performRuntimeEffect : EventLoopTaskState -> EventLoopRuntimeEffect -> IO Unit)
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (state : EventLoopTaskState) :
    IO Unit := do
  let some messages ← recvDrainedMessages? channel | pure ()
  let state ← runEventLoopMessages performRuntimeEffect state messages
  if state.stopRequested || (state.liveDocumentIds.isEmpty && state.eventLoop.taskQueue.isEmpty && !state.eventLoop.awaitingTaskCompletion) then
    channel.close
  else
    runEventLoopLoop performRuntimeEffect channel state

def runEventLoop
    (performRuntimeEffect : EventLoopTaskState -> EventLoopRuntimeEffect -> IO Unit)
    (channel : Std.CloseableChannel EventLoopTaskMessage)
    (state : EventLoopTaskState) :
    IO Unit := do
  let contentProcess ← contentProcessStart (USize.ofNat state.eventLoop.id)
  let state := { state with contentProcess }
  try
    runEventLoopLoop performRuntimeEffect channel state
  finally
    if contentProcess.raw ≠ 0 then
      contentProcessStop contentProcess.raw

structure EventLoopWorker where
  channel : Std.CloseableChannel EventLoopTaskMessage
  task : _root_.Task (Except IO.Error Unit)

private def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

def startEventLoopWorker
    (fetchChannel : Std.CloseableChannel FetchRuntimeMessage)
    (timerChannel : Std.CloseableChannel TimerRuntimeMessage)
    (onStartNavigationInUserAgent :
      Nat -> Nat -> Nat -> String -> String -> UserNavigationInvolvement -> Bool -> IO Unit)
    (onContinueNavigationInUserAgent :
      Nat -> Nat -> NavigationResponse -> IO Unit)
    (eventLoop : EventLoop)
    (onStopped : IO Unit := pure ()) :
    IO EventLoopWorker := do
  let channel ← Std.CloseableChannel.new
  let hooks : EventLoopRuntimeHooks := {
    startDocumentFetch := fun handler request => do
      let onComplete := fun response => do
        trySendAndForget
          channel
          (.queueDocumentFetchCompletion handler response)
      trySendAndForget fetchChannel (.startDocumentFetch handler request onComplete)
    scheduleTimeout := fun request => do
      let nowMs ← IO.monoMsNow
      let onComplete := fun completion => do
        match completion with
        | .windowTimerTask documentId timerId timerKey nestingLevel =>
            trySendAndForget
              channel
              (.queueWindowTimerTask { id := documentId } timerId timerKey nestingLevel)
        | .documentFetchTimeout handlerId =>
            trySendAndForget
              channel
              (.queueDocumentFetchTimeout { raw := USize.ofNat handlerId })
      trySendAndForget timerChannel (.scheduleTimeout nowMs request onComplete)
    clearTimeout := fun timerKey => do
      let nowMs ← IO.monoMsNow
      trySendAndForget timerChannel (.task (.clearTimeout nowMs timerKey))
    startNavigationInUserAgent :=
      onStartNavigationInUserAgent
    continueNavigationInUserAgent :=
      onContinueNavigationInUserAgent
  }
  let task ← IO.asTask <| do
    try
      runEventLoop (handleRuntimeEffect hooks) channel { eventLoop := eventLoop }
    finally
      onStopped
  pure {
    channel
    task
  }

end FormalWeb
