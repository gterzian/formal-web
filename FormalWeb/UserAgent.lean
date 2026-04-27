import Std.Data.TreeMap
import Std.Sync.Channel
import Mathlib.Control.Monad.Writer
import FormalWeb.EventLoop
import FormalWeb.Fetch
import FormalWeb.SessionHistoryNavigation
import FormalWeb.Traversable

namespace FormalWeb

/--
The user agent is the top-level global state for the browser model.
-/
structure UserAgent where
  /-- Model-local allocator state for https://dom.spec.whatwg.org/#concept-document -/
  nextDocumentId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#agent-cluster -/
  nextAgentClusterId : Nat := 0
  /-- Model-local allocator state for https://tc39.es/ecma262/#sec-agents -/
  nextAgentId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#event-loop -/
  nextEventLoopId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
  nextNavigationId : Nat := 0
  /-- Model-local allocator state for pending https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled continuations. -/
  nextBeforeUnloadCheckId : Nat := 0
  /-- Model-local allocator state for https://fetch.spec.whatwg.org/#fetch-controller -/
  nextFetchId : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  browsingContextGroupSet : BrowsingContextGroupSet := {}
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  topLevelTraversableSet : TopLevelTraversableSet := {}
  /-- Model-local map from https://html.spec.whatwg.org/multipage/#event-loop identifiers to event-loop objects. -/
  eventLoops : Std.TreeMap Nat EventLoop := Std.TreeMap.empty
  /-- Model-local queue of fetch-backed navigations suspended in https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
  pendingNavigationFetches : Std.TreeMap Nat PendingNavigationFetch := Std.TreeMap.empty
  /-- Model-local reverse index from https://fetch.spec.whatwg.org/#fetch-controller identifiers to pending navigation ids. -/
  pendingNavigationFetchIdsByFetchId : Std.TreeMap Nat Nat := Std.TreeMap.empty
  /-- Model-local queue of navigations paused while a content runtime runs `beforeunload`. -/
  pendingBeforeUnloadNavigations : Std.TreeMap Nat PendingBeforeUnloadNavigation := Std.TreeMap.empty
  /-- Model-local queue of loaded documents waiting for https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation. -/
  pendingNavigationFinalizations : Std.TreeMap Nat PendingNavigationFinalization := Std.TreeMap.empty
  /-- Model-local reverse index from https://html.spec.whatwg.org/multipage/#navigation-params-id to pending finalization document ids. -/
  pendingNavigationFinalizationIdsByNavigationId : Std.TreeMap Nat Nat := Std.TreeMap.empty
deriving Repr

instance : Inhabited UserAgent where
  default := {}

/-- Effects that the user-agent model can emit during a state transition. -/
inductive UserAgentEffect where
  | startEventLoop (eventLoopId : Nat)
  | startFetch (request : PendingFetchRequest)
  | eventLoopMessage (eventLoopId : Nat) (message : EventLoopTaskMessage)
  | notifyTopLevelTraversableCreated (webviewId : Nat)
  | notifyNavigationRequested (destinationURL : String)
  | notifyBeforeUnloadCompleted (documentId : Nat) (checkId : Nat) (canceled : Bool)
  | notifyFinalizeNavigation (webviewId : Nat) (url : String)
  | logError (message : String)
deriving Repr, DecidableEq

/--
The monad used by user-agent task handlers.
State = `UserAgent`, Writer = accumulated effects.
-/
abbrev M := WriterT (Array UserAgentEffect) (StateM UserAgent)

/-- Global registry mapping event-loop IDs to their task channels.
    Rust FFI callers read from this registry via `sendEventLoopMessage` to route
    event-loop messages without going through the user-agent channel. -/
initialize eventLoopChannelRegistry :
    IO.Ref (Std.TreeMap Nat (Std.CloseableChannel EventLoopTaskMessage)) ←
  IO.mkRef Std.TreeMap.empty

/-- Route an `EventLoopTaskMessage` to the channel for `eventLoopId`.
    Called directly by Rust FFI handlers (§4) rather than routing through the UA channel. -/
@[export sendEventLoopMessage]
def sendEventLoopMessage (eventLoopId : Nat) (message : EventLoopTaskMessage) : IO Unit := do
  let registry ← eventLoopChannelRegistry.get
  let some channel := registry.get? eventLoopId | return ()
  let _ ← channel.trySend message
  pure ()


namespace UserAgent

private abbrev M := StateM UserAgent

def setEventLoop
    (userAgent : UserAgent)
    (eventLoop : EventLoop) :
    UserAgent :=
  {
    userAgent with
      eventLoops := userAgent.eventLoops.insert eventLoop.id eventLoop
  }

def eventLoop?
    (userAgent : UserAgent)
    (eventLoopId : Nat) :
    Option EventLoop :=
  userAgent.eventLoops.get? eventLoopId

def allocateDocumentId (userAgent : UserAgent) : UserAgent × DocumentId :=
  let documentId : DocumentId := { id := userAgent.nextDocumentId }
  let userAgent := {
    userAgent with
      nextDocumentId := userAgent.nextDocumentId + 1
  }
  (userAgent, documentId)

def allocateAgentClusterId (userAgent : UserAgent) : UserAgent × Nat :=
  let agentClusterId := userAgent.nextAgentClusterId
  let userAgent := { userAgent with nextAgentClusterId := userAgent.nextAgentClusterId + 1 }
  (userAgent, agentClusterId)

def allocateAgentId (userAgent : UserAgent) : UserAgent × Nat :=
  let agentId := userAgent.nextAgentId
  let userAgent := {
    userAgent with
      nextAgentId := userAgent.nextAgentId + 1
  }
  (userAgent, agentId)

def allocateEventLoopId (userAgent : UserAgent) : UserAgent × Nat :=
  let eventLoopId := userAgent.nextEventLoopId
  let userAgent := { userAgent with nextEventLoopId := userAgent.nextEventLoopId + 1 }
  (userAgent, eventLoopId)

def allocateNavigationId (userAgent : UserAgent) : UserAgent × Nat :=
  let navigationId := userAgent.nextNavigationId
  let userAgent := { userAgent with nextNavigationId := userAgent.nextNavigationId + 1 }
  (userAgent, navigationId)

def allocateBeforeUnloadCheckId (userAgent : UserAgent) : UserAgent × Nat :=
  let checkId := userAgent.nextBeforeUnloadCheckId
  let userAgent := {
    userAgent with
      nextBeforeUnloadCheckId := userAgent.nextBeforeUnloadCheckId + 1
  }
  (userAgent, checkId)

def allocateFetchId (userAgent : UserAgent) : UserAgent × Nat :=
  let fetchId := userAgent.nextFetchId * 2
  let userAgent := { userAgent with nextFetchId := userAgent.nextFetchId + 1 }
  (userAgent, fetchId)

def appendPendingBeforeUnloadNavigation
    (userAgent : UserAgent)
    (pendingBeforeUnloadNavigation : PendingBeforeUnloadNavigation) :
    UserAgent :=
  {
    userAgent with
      pendingBeforeUnloadNavigations :=
        userAgent.pendingBeforeUnloadNavigations.insert
          pendingBeforeUnloadNavigation.checkId
          pendingBeforeUnloadNavigation
  }

def takePendingBeforeUnloadNavigation
    (userAgent : UserAgent)
    (checkId : Nat) :
    UserAgent × Option PendingBeforeUnloadNavigation :=
  let result := userAgent.pendingBeforeUnloadNavigations.get? checkId
  let userAgent := {
    userAgent with
      pendingBeforeUnloadNavigations :=
        userAgent.pendingBeforeUnloadNavigations.erase checkId
  }
  (userAgent, result)

def pendingBeforeUnloadNavigation?
    (userAgent : UserAgent)
    (checkId : Nat) :
    Option PendingBeforeUnloadNavigation :=
  userAgent.pendingBeforeUnloadNavigations.get? checkId

def setPendingBeforeUnloadNavigation
    (userAgent : UserAgent)
    (pendingBeforeUnloadNavigation : PendingBeforeUnloadNavigation) :
    UserAgent :=
  {
    userAgent with
      pendingBeforeUnloadNavigations :=
        userAgent.pendingBeforeUnloadNavigations.insert
          pendingBeforeUnloadNavigation.checkId
          pendingBeforeUnloadNavigation
  }

def appendPendingNavigationFetch
    (userAgent : UserAgent)
    (pendingNavigationFetch : PendingNavigationFetch) :
    UserAgent :=
  {
    userAgent with
      pendingNavigationFetches :=
        userAgent.pendingNavigationFetches.insert
          pendingNavigationFetch.navigationId
          pendingNavigationFetch
      pendingNavigationFetchIdsByFetchId :=
        userAgent.pendingNavigationFetchIdsByFetchId.insert
          pendingNavigationFetch.fetchId
          pendingNavigationFetch.navigationId
  }

def takePendingNavigationFetch
    (userAgent : UserAgent)
    (navigationId : Nat) :
    UserAgent × Option PendingNavigationFetch :=
  let result := userAgent.pendingNavigationFetches.get? navigationId
  let userAgent := {
    userAgent with
      pendingNavigationFetches := userAgent.pendingNavigationFetches.erase navigationId
      pendingNavigationFetchIdsByFetchId :=
        match result with
        | some pendingNavigationFetch =>
            userAgent.pendingNavigationFetchIdsByFetchId.erase pendingNavigationFetch.fetchId
        | none =>
            userAgent.pendingNavigationFetchIdsByFetchId
  }
  (userAgent, result)

def pendingNavigationFetch?
    (userAgent : UserAgent)
    (navigationId : Nat) :
    Option PendingNavigationFetch :=
  userAgent.pendingNavigationFetches.get? navigationId

def pendingNavigationFetchByFetchId?
    (userAgent : UserAgent)
    (fetchId : Nat) :
    Option PendingNavigationFetch := do
  let navigationId <- userAgent.pendingNavigationFetchIdsByFetchId.get? fetchId
  userAgent.pendingNavigationFetch? navigationId

def appendPendingNavigationFinalization
    (userAgent : UserAgent)
    (pendingNavigationFinalization : PendingNavigationFinalization) :
    UserAgent :=
  {
    userAgent with
      pendingNavigationFinalizations :=
        userAgent.pendingNavigationFinalizations.insert
          pendingNavigationFinalization.documentId
          pendingNavigationFinalization
      pendingNavigationFinalizationIdsByNavigationId :=
        userAgent.pendingNavigationFinalizationIdsByNavigationId.insert
          pendingNavigationFinalization.navigationId
          pendingNavigationFinalization.documentId
  }

def pendingNavigationFinalization?
    (userAgent : UserAgent)
    (documentId : Nat) :
    Option PendingNavigationFinalization :=
  userAgent.pendingNavigationFinalizations.get? documentId

def pendingNavigationFinalizationByNavigationId?
    (userAgent : UserAgent)
    (navigationId : Nat) :
    Option PendingNavigationFinalization := do
  let documentId <- userAgent.pendingNavigationFinalizationIdsByNavigationId.get? navigationId
  userAgent.pendingNavigationFinalization? documentId

def takePendingNavigationFinalization
    (userAgent : UserAgent)
    (documentId : Nat) :
    UserAgent × Option PendingNavigationFinalization :=
  let result := userAgent.pendingNavigationFinalization? documentId
  let userAgent := {
    userAgent with
      pendingNavigationFinalizations := userAgent.pendingNavigationFinalizations.erase documentId
      pendingNavigationFinalizationIdsByNavigationId :=
        match result with
        | some pendingNavigationFinalization =>
            userAgent.pendingNavigationFinalizationIdsByNavigationId.erase
              pendingNavigationFinalization.navigationId
        | none =>
            userAgent.pendingNavigationFinalizationIdsByNavigationId
  }
  (userAgent, result)

/-- Model-local bridge from the HTML navigation wait state to the runtime fetch worker. -/
def pendingNavigationFetchRequest?
    (userAgent : UserAgent)
    (navigationId : Nat) :
    Option PendingFetchRequest := do
  let pendingNavigationFetch <- userAgent.pendingNavigationFetch? navigationId
  pure {
    fetchId := pendingNavigationFetch.fetchId
    navigationId := pendingNavigationFetch.navigationId
    request := pendingNavigationFetch.request
  }

private def allocateDocumentIdM : M DocumentId := do
  let userAgent ← get
  let (userAgent, documentId) := userAgent.allocateDocumentId
  set userAgent
  pure documentId

private def allocateFetchIdM : M Nat := do
  let userAgent ← get
  let (userAgent, fetchId) := userAgent.allocateFetchId
  set userAgent
  pure fetchId

private def allocateAgentIdM : M Nat := do
  let userAgent ← get
  let (userAgent, agentId) := userAgent.allocateAgentId
  set userAgent
  pure agentId

private def allocateAgentClusterIdM : M Nat := do
  let userAgent ← get
  let (userAgent, agentClusterId) := userAgent.allocateAgentClusterId
  set userAgent
  pure agentClusterId

private def allocateEventLoopIdM : M Nat := do
  let userAgent ← get
  let (userAgent, eventLoopId) := userAgent.allocateEventLoopId
  set userAgent
  pure eventLoopId

private def setEventLoopM (eventLoop : EventLoop) : M Unit :=
  modify (·.setEventLoop eventLoop)

end UserAgent

def replaceTraversable
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable) :
    UserAgent :=
  {
    userAgent with
      topLevelTraversableSet := userAgent.topLevelTraversableSet.replace traversable
  }

def traversable?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option TopLevelTraversable :=
  userAgent.topLevelTraversableSet.find? traversableId

def activeTraversable?
    (userAgent : UserAgent) :
    Option TopLevelTraversable :=
  let rec loop (index remaining : Nat) : Option TopLevelTraversable :=
    match remaining with
    | 0 => none
    | remaining + 1 =>
        match traversable? userAgent index with
        | some traversable =>
            if traversable.isActive then
              some traversable
            else
              loop (index + 1) remaining
        | none =>
            loop (index + 1) remaining
  loop 0 userAgent.topLevelTraversableSet.members.size

def activeTraversableId?
    (userAgent : UserAgent) :
    Option Nat :=
  (activeTraversable? userAgent).map (·.id)

def traversableContainingDocument?
    (userAgent : UserAgent)
    (documentId : Nat) :
    Option TopLevelTraversable :=
  let rec loop (index remaining : Nat) : Option TopLevelTraversable :=
    match remaining with
    | 0 => none
    | remaining + 1 =>
        match traversable? userAgent index with
        | some traversable =>
            match traversable.toTraversableNavigable.activeDocument with
            | some document =>
                if document.documentId.id = documentId then
                  some traversable
                else
                  loop (index + 1) remaining
            | none =>
                loop (index + 1) remaining
        | none =>
            loop (index + 1) remaining
  loop 0 userAgent.topLevelTraversableSet.members.size

def traversableWithTargetName?
    (userAgent : UserAgent)
    (targetName : String) :
    Option TopLevelTraversable :=
  let rec loop (index remaining : Nat) : Option TopLevelTraversable :=
    match remaining with
    | 0 => none
    | remaining + 1 =>
        match traversable? userAgent index with
        | some traversable =>
            if traversable.targetName = targetName then
              some traversable
            else
              loop (index + 1) remaining
        | none =>
            loop (index + 1) remaining
  loop 0 userAgent.topLevelTraversableSet.members.size

def browsingContextGroupOfBrowsingContext?
    (userAgent : UserAgent)
    (browsingContextId : Nat) :
    Option BrowsingContextGroup :=
  let rec loop (index remaining : Nat) : Option BrowsingContextGroup :=
    match remaining with
    | 0 => none
    | remaining + 1 =>
        match userAgent.browsingContextGroupSet.members.get? index with
        | some group =>
            if group.browsingContextSet.get? browsingContextId |>.isSome then
              some group
            else
              loop (index + 1) remaining
        | none =>
            loop (index + 1) remaining
  loop 0 userAgent.browsingContextGroupSet.members.size

def setActiveTraversable
    (userAgent : UserAgent)
    (activeTraversableId : Nat) :
    UserAgent :=
  let userAgent :=
    match activeTraversable? userAgent with
    | some activeTraversable =>
        if activeTraversable.id = activeTraversableId then
          userAgent
        else
          replaceTraversable userAgent { activeTraversable with isActive := false }
    | none =>
        userAgent
  match traversable? userAgent activeTraversableId with
  | some traversable =>
      if traversable.isActive then
        userAgent
      else
        replaceTraversable userAgent { traversable with isActive := true }
  | none =>
      userAgent

def activeDocumentId?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option DocumentId := do
  let some traversable := traversable? userAgent traversableId | none
  let some document := traversable.toTraversableNavigable.activeDocument | none
  pure document.documentId

private def updateOptionDocument
    (optionDocument : Option Document)
    (documentId : Nat)
    (f : Document -> Document) :
    Option Document :=
  optionDocument.map fun document =>
    if document.documentId.id = documentId then
      f document
    else
      document

private def updateHistoryEntryDocument
    (historyEntry : SessionHistoryEntry)
    (documentId : Nat)
    (f : Document -> Document) :
    SessionHistoryEntry :=
  {
    historyEntry with
      documentState := {
        historyEntry.documentState with
          document := updateOptionDocument historyEntry.documentState.document documentId f
      }
  }

private def updateTraversableDocument
    (traversable : TopLevelTraversable)
    (documentId : Nat)
    (f : Document -> Document) :
    TopLevelTraversable :=
  {
    traversable with
      toTraversableNavigable := {
        traversable.toTraversableNavigable with
          toNavigable := {
            traversable.toTraversableNavigable.toNavigable with
              currentSessionHistoryEntry :=
                traversable.toTraversableNavigable.toNavigable.currentSessionHistoryEntry.map
                  (fun historyEntry => updateHistoryEntryDocument historyEntry documentId f)
              activeSessionHistoryEntry :=
                traversable.toTraversableNavigable.toNavigable.activeSessionHistoryEntry.map
                  (fun historyEntry => updateHistoryEntryDocument historyEntry documentId f)
          }
          activeDocument := updateOptionDocument traversable.toTraversableNavigable.activeDocument documentId f
          sessionHistoryEntries :=
            traversable.toTraversableNavigable.sessionHistoryEntries.map
              (fun historyEntry => updateHistoryEntryDocument historyEntry documentId f)
      }
      parentNavigableIdNone := by
        simpa using traversable.parentNavigableIdNone
  }

def cancelPendingNavigationFinalization
    (userAgent : UserAgent)
    (navigationId : Nat) :
    UserAgent :=
  match UserAgent.pendingNavigationFinalizationByNavigationId? userAgent navigationId with
  | some pendingNavigationFinalization =>
      (userAgent.takePendingNavigationFinalization pendingNavigationFinalization.documentId).1
  | none =>
      userAgent

/-- https://html.spec.whatwg.org/multipage/#make-document-unsalvageable -/
def makeDocumentUnsalvageable
    (userAgent : UserAgent)
    (documentId : Nat) :
    UserAgent :=
  match traversableContainingDocument? userAgent documentId with
  | some traversable =>
      replaceTraversable
        userAgent
        (updateTraversableDocument traversable documentId fun document =>
          { document with salvageable := false })
  | none =>
      userAgent

/-- https://html.spec.whatwg.org/multipage/#abort-a-document-and-its-descendants -/
def abortDocumentAndDescendants
    (userAgent : UserAgent)
    (documentId : Nat) :
    UserAgent :=
  -- Step 1: Let descendantNavigables be document's descendant navigables.
  -- TODO: Model descendant navigables once child navigables exist in the user-agent state.

  -- Step 3: Abort document.
  -- Note: The current model collapses the queued abort task to its durable effect of making the active document unsalvageable.
  makeDocumentUnsalvageable userAgent documentId

def notePendingUpdateTheRendering
    (userAgent : UserAgent)
    (traversableId : Nat) :
    UserAgent :=
  match traversable? userAgent traversableId with
  | none =>
      userAgent
  | some traversable =>
      if traversable.toTraversableNavigable.hasDeferredUpdateTheRendering then
        userAgent
      else
        let traversable := {
          traversable with
            toTraversableNavigable := {
              traversable.toTraversableNavigable with
                hasDeferredUpdateTheRendering := true
            }
        }
        replaceTraversable userAgent traversable

def queueUpdateTheRendering
    (userAgent : UserAgent)
    (traversableId : Nat) :
    UserAgent × Option (Nat × EventLoopTaskMessage) := Id.run do
  let some traversable := traversable? userAgent traversableId
    | (userAgent, none)
  let some document := traversable.toTraversableNavigable.activeDocument
    | (userAgent, none)
  let some documentId := activeDocumentId? userAgent traversableId
    | (userAgent, none)
  let some eventLoop := userAgent.eventLoop? document.eventLoopId
    | (userAgent, none)
  let userAgent :=
    if traversable.toTraversableNavigable.hasDeferredUpdateTheRendering then
      let traversable := {
        traversable with
          toTraversableNavigable := {
            traversable.toTraversableNavigable with
              hasDeferredUpdateTheRendering := false
          }
      }
      replaceTraversable userAgent traversable
    else
      userAgent
  let eventLoop := eventLoop.enqueueTask { step := .updateTheRendering }
  (
    userAgent.setEventLoop eventLoop,
    some (document.eventLoopId, EventLoopTaskMessage.queueUpdateTheRendering traversableId documentId)
  )

def resumePendingUpdateTheRenderingAfterNavigation
    (userAgent : UserAgent)
    (traversableId : Nat) :
    UserAgent × Option (Nat × EventLoopTaskMessage) :=
  match traversable? userAgent traversableId with
  | some traversable =>
      if traversable.toTraversableNavigable.hasDeferredUpdateTheRendering then
        queueUpdateTheRendering userAgent traversableId
      else
        (userAgent, none)
  | none =>
      (userAgent, none)

def queueDispatchedEvent
    (userAgent : UserAgent)
  (traversableId : Nat) :
    Option (UserAgent × Nat × DocumentId) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  let documentId <- activeDocumentId? userAgent traversableId
  let eventLoop <- userAgent.eventLoop? document.eventLoopId
  let eventLoop := eventLoop.enqueueTask { step := .dispatchEvent }
  pure (
    userAgent.setEventLoop eventLoop,
    document.eventLoopId,
    documentId
  )

def queueRunBeforeUnload
    (userAgent : UserAgent)
  (traversableId : Nat) :
    Option (UserAgent × Nat × DocumentId) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  let documentId <- activeDocumentId? userAgent traversableId
  let eventLoop <- userAgent.eventLoop? document.eventLoopId
  let eventLoop := eventLoop.enqueueTask { step := .runBeforeUnload }
  pure (
    userAgent.setEventLoop eventLoop,
    document.eventLoopId,
    documentId
  )

/-- https://html.spec.whatwg.org/multipage/#obtain-browsing-context-navigation -/
def obtainBrowsingContextToUseForNavigationResponse
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams) :
    UserAgent × Nat :=
  Id.run do
    -- Step 1: Let browsingContext be navigationParams's navigable's active browsing context.
    let browsingContextId := traversable.toTraversableNavigable.activeBrowsingContextId.getD 0

    -- Step 2: If browsingContext is not a top-level browsing context, then return browsingContext.
    -- Note: The current model stores only top-level browsing contexts, so this always continues with a top-level context.

    -- Step 5: Let sourceOrigin be browsingContext's active document's origin.
    let sourceOrigin? := traversable.toTraversableNavigable.activeDocument.map (·.origin)

    -- Step 6: Let destinationOrigin be navigationParams's origin.
    let destinationOrigin := navigationParams.origin

    -- Step 8: If swapGroup is false, then return browsingContext.
    -- Note: The current model approximates the COOP and same-site checks with direct same-origin reuse.
    if sourceOrigin? = some destinationOrigin then
      (userAgent, browsingContextId)
    else
      -- Step 9: Let newBrowsingContext be the first return value of creating a new top-level browsing context and document.
      -- Note: The current model allocates a fresh browsing-context identifier in the existing group and lets the navigation response allocate the replacement `Document`.
      let some group := browsingContextGroupOfBrowsingContext? userAgent browsingContextId
        | (userAgent, browsingContextId)
      let (group, browsingContext) := group.append { id := group.nextBrowsingContextId }
      let userAgent := {
        userAgent with
          browsingContextGroupSet := userAgent.browsingContextGroupSet.replace group
      }
      (userAgent, browsingContext.id)

/-- https://html.spec.whatwg.org/multipage/#determining-the-creation-sandboxing-flags -/
def determineCreationSandboxingFlags
    (_browsingContext : BrowsingContext)
    (_embedder : Option Unit) :
    SandboxingFlagSet :=
  -- TODO: Model the creation sandboxing flags algorithm.
  []

/-- https://html.spec.whatwg.org/multipage/#snapshotting-source-snapshot-params -/
def snapshotSourceSnapshotParams (sourceDocument : Document) : SourceSnapshotParams :=
  {
    sourcePolicyContainer := sourceDocument.policyContainer
  }

/-- https://html.spec.whatwg.org/multipage/#snapshotting-target-snapshot-params -/
def snapshotTargetSnapshotParams (_traversable : TopLevelTraversable) : TargetSnapshotParams :=
  {}

/-- https://html.spec.whatwg.org/multipage/#queue-a-task -/
def queueTask
    (userAgent : UserAgent)
    (source : TaskSource)
    (eventLoopId : Nat)
    (documentId : Option Nat)
    (step : TaskStep) :
    Option UserAgent := do
  -- Step 3: Let task be a new task.
  let task : Task := {
    step
    source
    documentId
  }
  -- Steps 4-9: Populate the task and append it to the event loop's associated queue.
  let eventLoop <- userAgent.eventLoop? eventLoopId
  let eventLoop := eventLoop.enqueueTask task
  pure (userAgent.setEventLoop eventLoop)

/-- https://html.spec.whatwg.org/multipage/#queue-a-global-task -/
def queueGlobalTask
    (userAgent : UserAgent)
    (source : TaskSource)
    (_globalId : GlobalObjectId)
    (eventLoopId : Nat)
    (step : TaskStep) :
    Option UserAgent :=
  -- Step 1: Let event loop be global's relevant agent's event loop.
  -- TODO: Replace the explicit eventLoopId parameter with a lookup from an opaque global-object model.
  -- Step 2: Let document be global's associated Document, if the global is a Window object; otherwise null.
  let documentId : Option Nat := none
  -- Step 3: Queue a task given source, event loop, document, and steps.
  queueTask userAgent source eventLoopId documentId step

/-- https://html.spec.whatwg.org/multipage/#populate-with-html/head/body -/
def populateWithHtmlHeadBody
    (userAgent : UserAgent)
    (document : Document) :
    UserAgent :=
  -- Step 1: Let html be the result of creating an element given document, "html", and the HTML namespace.
  -- Step 2: Let head be the result of creating an element given document, "head", and the HTML namespace.
  -- Step 3: Let body be the result of creating an element given document, "body", and the HTML namespace.
  -- Step 4: Append html to document.
  -- Step 5: Append head to html.
  -- Step 6: Append body to html.
  -- Notes: The executable runtime now performs this embedder-side document allocation inside the owning event loop.
  let _ := document
  userAgent


/-- https://html.spec.whatwg.org/multipage/#create-an-agent -/
def createAgent
    (userAgent : UserAgent)
    (canBlock : Bool) :
    UserAgent × Agent :=
  let (agent, userAgent) := (createAgentImpl canBlock).run userAgent
  (userAgent, agent)
where
  createAgentImpl (canBlock : Bool) : UserAgent.M Agent := do
    -- Step 1: Let signifier be a new unique internal value.
    let agentId ← UserAgent.allocateAgentIdM
    -- Step 2: Let candidateExecution be a new candidate execution.
    -- TODO: Model candidate execution if scheduling between agents becomes explicit.
    -- Step 3: Let agent be a new agent whose [[CanBlock]] is canBlock, [[Signifier]] is signifier, [[CandidateExecution]] is candidateExecution, and [[IsLockFree1]], [[IsLockFree2]], and [[LittleEndian]] are set at the implementation's discretion.
    -- Step 4: Set agent's event loop to a new event loop.
    let eventLoopId ← UserAgent.allocateEventLoopIdM
    let eventLoop : EventLoop := { id := eventLoopId }
    UserAgent.setEventLoopM eventLoop
    -- Step 5: Return agent.
    pure {
      id := agentId
      canBlock
      eventLoop
    }

/-- https://html.spec.whatwg.org/multipage/#initialise-the-document-object -/
def createAndInitializeDocumentObject
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams) :
    UserAgent × Document :=
  let (document, userAgent) :=
    (createAndInitializeDocumentObjectImpl traversable navigationParams).run userAgent
  (userAgent, document)
where
  createAndInitializeDocumentObjectImpl
      (traversable : TopLevelTraversable)
      (navigationParams : NavigationParams) :
      UserAgent.M Document := do
    -- Step 1: Let browsingContext be the result of obtaining a browsing context to use for a navigation response.
    let userAgent ← get
    let (userAgent, browsingContextId) :=
      obtainBrowsingContextToUseForNavigationResponse userAgent traversable navigationParams
    set userAgent

    -- Step 2: Let permissionsPolicy be the result of creating a permissions policy from a response.
    let permissionsPolicy := createPermissionsPolicy none navigationParams.origin
    -- TODO: Model creating a permissions policy from a response.

    -- Step 3: Let creationURL be navigationParams's response's URL.
    let creationURL := navigationParams.response.url

    -- Steps 4-10: Window/realm/environment setup.
    -- TODO: Model window reuse, new realms, and window environment settings for response-driven document creation.

    -- Step 11: Let loadTimingInfo be a new document load timing info.
    let loadTimingInfo : DocumentLoadTimingInfo := {}
    -- TODO: Thread actual response timing info into load timing.

    -- Notes: Response-driven document creation stays on the traversable's existing event loop.
    let eventLoopId :=
      (traversable.toTraversableNavigable.activeDocument.map (·.eventLoopId)).getD 0

    -- Step 12: Let document be a new Document, with:
    let documentId ← UserAgent.allocateDocumentIdM
    let document : Document := {
      documentId
      origin := navigationParams.origin
      browsingContextId
      eventLoopId
      policyContainer := navigationParams.policyContainer
      permissionsPolicy
      activeSandboxingFlagSet := navigationParams.finalSandboxingFlagSet
      openerPolicy := navigationParams.coop
      loadTimingInfo
      isInitialAboutBlank := false
      aboutBaseURL := navigationParams.aboutBaseURL
      referrer := navigationParams.request.map (·.referrer)
      url := creationURL
    }

    -- Steps 13-14: Set ancestor-origin lists.
    let document := {
      document with
        internalAncestorOriginObjectsList :=
          internalAncestorOriginObjectsListCreationSteps document navigationParams.iframeElementReferrerPolicy
    }
    let document := {
      document with
        ancestorOriginsList := some (ancestorOriginsListCreationSteps document)
    }

    -- Steps 15+: Response-driven initialization hooks.
    -- TODO: Model CSP initialization, navigation timing entries, early hints, and link-header processing.

    pure document

/-- https://html.spec.whatwg.org/multipage/#navigate-html -/
def loadHtmlDocument
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams) :
    UserAgent × Document :=
  -- Step 1: Let document be the result of creating and initializing a Document object.
  let (userAgent, document) := createAndInitializeDocumentObject userAgent traversable navigationParams

  -- Step 2: If document's URL is about:blank, then populate with html/head/body given document.
  if document.url = "about:blank" then
    let userAgent := populateWithHtmlHeadBody userAgent document
    (userAgent, document)
  else
    -- Step 3: Otherwise, create an HTML parser and associate it with the document.
    -- Notes: The current model forwards the fetched response body into a Rust-side HTML document.
    -- TODO: Apply the fetched response body to the document (incremental parser; streaming fetch delivery is future work).
    -- Notes: Incremental parser input and streaming fetch delivery remain future work.
    (userAgent, document)

/-- https://html.spec.whatwg.org/multipage/#loading-a-document -/
private def contentTypeEssence (contentType : String) : String :=
  match contentType.splitOn ";" with
  | [] => contentType.trimAscii.toString.toLower
  | mediaType :: _ => mediaType.trimAscii.toString.toLower


/-- https://html.spec.whatwg.org/multipage/#loading-a-document -/
def loadDocument
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams)
    (_sourceSnapshotParams : SourceSnapshotParams)
    (_initiatorOrigin : Option Origin) :
    UserAgent × Option Document :=
  let responseContentType := contentTypeEssence navigationParams.response.contentType
  if responseContentType = "text/html" then
    let (userAgent, document) := loadHtmlDocument userAgent traversable navigationParams
    (userAgent, some document)
  else
    -- TODO: Model non-HTML branches of loading a document.
    (userAgent, none)

/-- Helper for the response-arrived half of create-navigation-params-by-fetching. -/
def createNavigationParamsFromResponse
    (pendingNavigationFetch : PendingNavigationFetch)
    (response : NavigationResponse) :
    NavigationParams :=
  let finalSandboxingFlagSet := pendingNavigationFetch.targetSnapshotParams.sandboxingFlags
  let origin :=
    determineOrigin response.url finalSandboxingFlagSet pendingNavigationFetch.historyEntry.documentState.initiatorOrigin
  {
    id := pendingNavigationFetch.navigationId
    traversableId := pendingNavigationFetch.traversableId
    request := some pendingNavigationFetch.request
    response
    fetchControllerId := some pendingNavigationFetch.fetchId
    origin
    policyContainer := pendingNavigationFetch.sourceSnapshotParams.sourcePolicyContainer
    finalSandboxingFlagSet
    iframeElementReferrerPolicy := pendingNavigationFetch.targetSnapshotParams.iframeElementReferrerPolicy
    aboutBaseURL := pendingNavigationFetch.historyEntry.documentState.aboutBaseURL
    userInvolvement := pendingNavigationFetch.userInvolvement
    navigationTimingType := pendingNavigationFetch.navTimingType
  }

/-- https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
def createNavigationParamsByFetching
    (userAgent : UserAgent)
    (entry : SessionHistoryEntry)
    (traversable : TopLevelTraversable)
    (sourceSnapshotParams : SourceSnapshotParams)
    (targetSnapshotParams : TargetSnapshotParams)
    (cspNavigationType : String)
    (userInvolvement : UserNavigationInvolvement)
    (navigationId : Nat)
    (navTimingType : NavigationTimingType) :
    UserAgent :=
  -- Step 1: Let documentResource be entry's document state's resource.
  let documentResource := entry.documentState.resource

  -- Step 2: Let request be a new request, with:
  let request : NavigationRequest :=
    match documentResource with
    | some (.post postResource) => {
        url := entry.url
        method := "POST"
        referrer := entry.documentState.requestReferrer
        referrerPolicy := entry.documentState.requestReferrerPolicy
        policyContainer := sourceSnapshotParams.sourcePolicyContainer
        body := postResource.requestBody
      }
    | _ => {
        url := entry.url
        referrer := entry.documentState.requestReferrer
        referrerPolicy := entry.documentState.requestReferrerPolicy
        policyContainer := sourceSnapshotParams.sourcePolicyContainer
      }
  -- TODO: Model the full request initialization surface, including client, destination, and POST resources.

  -- Steps 3-18: Reserved-environment, CSP, redirect-loop, and fetch-controller orchestration.
  -- For now we stop at the asynchronous boundary where fetch has been initiated and the algorithm waits for response to become non-null.
  let (userAgent, fetchId) := userAgent.allocateFetchId
  let pendingNavigationFetch : PendingNavigationFetch := {
    fetchId
    navigationId
    traversableId := traversable.id
    historyEntry := entry
    sourceSnapshotParams
    targetSnapshotParams
    navTimingType
    userInvolvement
    cspNavigationType
    request
  }
  userAgent.appendPendingNavigationFetch pendingNavigationFetch

/-- https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document -/
def continueAttemptToPopulateHistoryEntryDocument
    (userAgent : UserAgent)
    (entry : SessionHistoryEntry)
    (traversable : TopLevelTraversable)
    (sourceSnapshotParams : SourceSnapshotParams)
    (navigationParams : NavigationParams) :
    UserAgent :=
  -- Step 5.8: Let entry's document state's document be the result of loading a document.
  let (userAgent, document) :=
    loadDocument
      userAgent
      traversable
      navigationParams
      sourceSnapshotParams
      entry.documentState.initiatorOrigin

  -- Step 6: If entry's document state's document is not null, then:
  let historyEntry : SessionHistoryEntry := {
    entry with
      documentState := {
        entry.documentState with
          document
          everPopulated := document.isSome
          origin := match document with
            | some document => some document.origin
            | none => entry.documentState.origin
          historyPolicyContainer := some navigationParams.policyContainer
          requestReferrer :=
            (navigationParams.request.map (·.referrer)).getD entry.documentState.requestReferrer
      }
  }
  match document with
  | some document =>
      let pendingNavigationFinalization : PendingNavigationFinalization := {
        documentId := document.documentId.id
        navigationId := navigationParams.id
        traversableId := traversable.id
        historyEntry
        historyHandling := .push
        userInvolvement := navigationParams.userInvolvement
      }

      -- Step 7: Run completionSteps.
      -- Note: The current model stores the completion-steps continuation explicitly and waits for the content runtime's `FinalizeNavigation` signal before mutating session history.
      userAgent.appendPendingNavigationFinalization pendingNavigationFinalization
  | none =>
      let updatedNavigable :=
        setOngoingNavigation traversable.toTraversableNavigable.toNavigable none
      let traversable := {
        traversable with
          toTraversableNavigable := {
            traversable.toTraversableNavigable with
              toNavigable := updatedNavigable
          }
          parentNavigableIdNone := by
            simpa [updatedNavigable, setOngoingNavigation_preserves_parentNavigableId] using
              traversable.parentNavigableIdNone
      }
      replaceTraversable userAgent traversable

/--
Continuation of the wait in https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching.

If `response` is `some`, this models the branch where the wait ends because the fetch response became non-null.
If `response` is `none`, this models the branch where the wait ends because the navigable's ongoing navigation changed to no longer equal `navigationId`.
-/
def finishCreatingNavigationParamsByFetching
    (userAgent : UserAgent)
    (navigationId : Nat)
    (response : Option NavigationResponse) :
    UserAgent :=
  Id.run do
    let some pendingNavigationFetch := userAgent.pendingNavigationFetch? navigationId
      | userAgent
    let ongoingNavigationMatches :=
      if let some traversable := traversable? userAgent pendingNavigationFetch.traversableId then
        traversable.toTraversableNavigable.toNavigable.ongoingNavigation = some (.navigationId navigationId)
      else
        false
    if response.isNone && ongoingNavigationMatches then
      -- The wait condition has not been satisfied yet, so the pending fetch remains in place.
      userAgent
    else
      let (userAgent, pendingNavigationFetch?) := userAgent.takePendingNavigationFetch navigationId
      let some pendingNavigationFetch := pendingNavigationFetch?
        | userAgent
      let some traversable := traversable? userAgent pendingNavigationFetch.traversableId
        | userAgent
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation != some (.navigationId navigationId) then
        -- The latter wait condition occurred, so the fetch-backed creation returns without producing navigation params.
        userAgent
      else
        let some response := response
          | userAgent
        let navigationParams := createNavigationParamsFromResponse pendingNavigationFetch response
        continueAttemptToPopulateHistoryEntryDocument
          userAgent
          pendingNavigationFetch.historyEntry
          traversable
          pendingNavigationFetch.sourceSnapshotParams
          navigationParams

/-- https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document -/
def attemptToPopulateHistoryEntryDocument
    (userAgent : UserAgent)
    (entry : SessionHistoryEntry)
    (traversable : TopLevelTraversable)
    (navTimingType : NavigationTimingType)
    (sourceSnapshotParams : SourceSnapshotParams)
    (targetSnapshotParams : TargetSnapshotParams)
    (userInvolvement : UserNavigationInvolvement)
    (navigationId : Nat)
    (navigationParams : Option NavigationParams := none)
    (cspNavigationType : String := "other")
    (allowPOST : Bool := false) :
    UserAgent :=
  Id.run do
    if let some navigationParams := navigationParams then
      continueAttemptToPopulateHistoryEntryDocument
        userAgent
        entry
        traversable
        sourceSnapshotParams
        navigationParams
    else if let some (.srcdoc _) := entry.documentState.resource then
      -- TODO: Model create-navigation-params-from-a-srcdoc-resource.
      userAgent
    else
      let mayFetch :=
        match entry.documentState.resource with
        | none => true
        | some documentResource => allowPOST && hasUsablePostResource documentResource
      if isFetchScheme entry.url && mayFetch then
        createNavigationParamsByFetching
          userAgent
          entry
          traversable
          sourceSnapshotParams
          targetSnapshotParams
          cspNavigationType
          userInvolvement
          navigationId
          navTimingType
      else
        -- TODO: Model non-fetch-scheme branches.
        userAgent

def processNavigationFetchResponse
    (userAgent : UserAgent)
    (navigationId : Nat)
    (response : NavigationResponse) :
    UserAgent :=
  finishCreatingNavigationParamsByFetching userAgent navigationId (some response)

/--
Continuation of the alternative branch of the wait in https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching,
when the navigable's ongoing navigation changed before any response became available.
-/
def processNavigationFetchCancellation
    (userAgent : UserAgent)
    (navigationId : Nat) :
    UserAgent :=
  cancelPendingNavigationFinalization
    (finishCreatingNavigationParamsByFetching userAgent navigationId none)
    navigationId

/-- Model a concrete ongoing-navigation change that can wake a fetch-backed wait. -/
def abortNavigation
    (userAgent : UserAgent)
    (traversableId : Nat) :
    UserAgent :=
  Id.run do
    let some traversable := traversable? userAgent traversableId
      | userAgent
    let previousOngoingNavigation :=
      traversable.toTraversableNavigable.toNavigable.ongoingNavigation
    let updatedNavigable :=
      setOngoingNavigation traversable.toTraversableNavigable.toNavigable none
    let updatedTraversable := {
      traversable with
        toTraversableNavigable := {
          traversable.toTraversableNavigable with
            toNavigable := updatedNavigable
        }
        parentNavigableIdNone := by
          simpa [updatedNavigable, setOngoingNavigation_preserves_parentNavigableId] using
            traversable.parentNavigableIdNone
    }
    let userAgent := replaceTraversable userAgent updatedTraversable
    if let some (.navigationId navigationId) := previousOngoingNavigation then
      processNavigationFetchCancellation userAgent navigationId
    else
      userAgent

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigate
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none)
    (userInvolvement : UserNavigationInvolvement := .none) :
    UserAgent × Option PendingFetchRequest :=
  Id.run do
    let some sourceDocument := traversable.toTraversableNavigable.activeDocument
      | (userAgent, none)
    -- TODO: Model the browser-UI/sourceDocument-null branch of beginning navigation.
    let previousOngoingNavigation :=
      traversable.toTraversableNavigable.toNavigable.ongoingNavigation
    let (userAgent, navigationId) := userAgent.allocateNavigationId
    let updatedNavigable :=
      setOngoingNavigation
        traversable.toTraversableNavigable.toNavigable
        (some (.navigationId navigationId))
    let traversable := {
      traversable with
        toTraversableNavigable := {
          traversable.toTraversableNavigable with
            toNavigable := updatedNavigable
        }
        parentNavigableIdNone := by
          simpa [updatedNavigable, setOngoingNavigation_preserves_parentNavigableId] using
            traversable.parentNavigableIdNone
    }
    let userAgent := replaceTraversable userAgent traversable
    let userAgent :=
      if let some (.navigationId previousNavigationId) := previousOngoingNavigation then
        if previousNavigationId = navigationId then
          userAgent
        else
          processNavigationFetchCancellation userAgent previousNavigationId
      else
        userAgent
    let sourceSnapshotParams := snapshotSourceSnapshotParams sourceDocument
    let targetSnapshotParams := snapshotTargetSnapshotParams traversable
    let documentState : DocumentState := {
      initiatorOrigin := some sourceDocument.origin
      aboutBaseURL := sourceDocument.aboutBaseURL
      resource := documentResource
      navigableTargetName := traversable.targetName
    }
    let historyEntry : SessionHistoryEntry := {
      url := destinationURL
      documentState
    }
    let userAgent :=
      attemptToPopulateHistoryEntryDocument
      userAgent
      historyEntry
      traversable
      .navigate
      sourceSnapshotParams
      targetSnapshotParams
      userInvolvement
      navigationId
      none
      "other"
      true
    (userAgent, UserAgent.pendingNavigationFetchRequest? userAgent navigationId)

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigateWithPendingFetchRequest
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none) :
    UserAgent × Option PendingFetchRequest :=
  navigate userAgent traversable destinationURL documentResource

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigateIgnoringPendingFetchRequest
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none) :
    UserAgent :=
  (navigate userAgent traversable destinationURL documentResource).1

/-- https://html.spec.whatwg.org/multipage/#obtain-similar-origin-window-agent -/
def obtainSimilarOriginWindowAgent
    (userAgent : UserAgent)
    (origin : Origin)
    (group : BrowsingContextGroup)
    (requestsOAC : Bool) :
    UserAgent × BrowsingContextGroup × Agent :=
  -- Step 1: Let site be the result of obtaining a site with origin.
  let site := obtainSite origin

  -- Step 2: Let key be site.
  let defaultKey := AgentClusterKey.site site
  let historicalKey := group.historicalAgentClusterKey origin

  -- Step 3: If group's cross-origin isolation mode is not "none", then set key to origin.
  -- Step 4: Otherwise, if group's historical agent cluster key map[origin] exists, then set key to group's historical agent cluster key map[origin].
  let key :=
    if group.crossOriginIsolationMode != .none then
      AgentClusterKey.origin origin
    else
      historicalKey.getD defaultKey

  -- Step 5: Otherwise:
  let (group, key) :=
    if group.crossOriginIsolationMode != .none || historicalKey.isSome then
      (group, key)
    else
      -- Step 5.1: If requestsOAC is true, then set key to origin.
      let key := if requestsOAC then AgentClusterKey.origin origin else key
      -- Step 5.2: Set group's historical agent cluster key map[origin] to key.
      let group := group.setHistoricalAgentClusterKey origin key
      (group, key)

  -- Step 6: If group's agent cluster map[key] does not exist, then:
  let (userAgent, group, agentCluster) := match group.agentCluster key with
    | some agentCluster => (userAgent, group, agentCluster)
    | none =>
        -- Step 6.1: Let agentCluster be a new agent cluster.
      let (userAgent, agentClusterId) := userAgent.allocateAgentClusterId
        -- Step 6.2: Set agentCluster's cross-origin isolation mode to group's cross-origin isolation mode.
        -- Step 6.3: If key is an origin, then set agentCluster's is origin-keyed to true.
        -- Step 6.4: Add the result of creating an agent, given false, to agentCluster.
        let (userAgent, agent) := createAgent userAgent false
        let agentCluster : AgentCluster := {
          id := agentClusterId
          similarOriginWindowAgent := agent
          crossOriginIsolationMode := group.crossOriginIsolationMode
          isOriginKeyed :=
            match key with
            | .origin _ => true
            | .site _ => false
        }
        -- Step 6.5: Set group's agent cluster map[key] to agentCluster.
        let group := group.setAgentCluster key agentCluster
        (userAgent, group, agentCluster)

  -- Step 7: Return the single similar-origin window agent contained in group's agent cluster map[key].
  (userAgent, group, agentCluster.similarOriginWindowAgent)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-browsing-context -/
def createNewBrowsingContextAndDocument
    (userAgent : UserAgent)
    (creator : Option Document)
    (embedder : Option Unit)
    (group : BrowsingContextGroup) :
    UserAgent × BrowsingContextGroup × BrowsingContext × Document :=
  let ((group, browsingContext, document), userAgent) :=
    (createNewBrowsingContextAndDocumentImpl creator embedder group).run userAgent
  (userAgent, group, browsingContext, document)
where
  obtainSimilarOriginWindowAgentM
      (origin : Origin)
      (group : BrowsingContextGroup)
      (requestsOAC : Bool) :
      UserAgent.M (BrowsingContextGroup × Agent) := do
    let userAgent ← get
    let (userAgent, group, agent) := obtainSimilarOriginWindowAgent userAgent origin group requestsOAC
    set userAgent
    pure (group, agent)

  createNewBrowsingContextAndDocumentImpl
      (creator : Option Document)
      (embedder : Option Unit)
      (group : BrowsingContextGroup) :
      UserAgent.M (BrowsingContextGroup × BrowsingContext × Document) := do
    -- Step 1: Let browsingContext be a new browsing context.
    let browsingContext : BrowsingContext := { id := group.nextBrowsingContextId }

    -- Step 2: Let unsafeContextCreationTime be the unsafe shared current time.
    let unsafeContextCreationTime : Nat := 0

    -- Step 3: Let creatorOrigin be null.
    let creatorOrigin : Option Origin := none

    -- Step 4: Let creatorBaseURL be null.
    let creatorBaseURL : Option String := none

    -- Step 5: If creator is non-null, then:
    let (creatorOrigin, creatorBaseURL) := match creator with
      | some creator =>
          -- Step 5.1: Set creatorOrigin to creator's origin.
          -- Step 5.2: Set creatorBaseURL to creator's document base URL.
          -- Step 5.3: Set browsingContext's virtual browsing context group ID to creator's browsing context's top-level browsing context's virtual browsing context group ID.
          -- TODO: Model virtual browsing context group IDs.
          (some creator.origin, creator.aboutBaseURL)
      | none =>
          (creatorOrigin, creatorBaseURL)

    -- Step 6: Let sandboxFlags be the result of determining the creation sandboxing flags given browsingContext and embedder.
    let sandboxFlags := determineCreationSandboxingFlags browsingContext embedder

    -- Step 7: Let origin be the result of determining the origin given about:blank, sandboxFlags, and creatorOrigin.
    let origin := determineOrigin "about:blank" sandboxFlags creatorOrigin

    -- Step 8: Let permissionsPolicy be the result of creating a permissions policy given embedder and origin.
    let permissionsPolicy := createPermissionsPolicy embedder origin

    -- Step 9: Let agent be the result of obtaining a similar-origin window agent given origin, group, and false.
    let (group, agent) ← obtainSimilarOriginWindowAgentM origin group false

    -- Step 10: Let realm execution context be the result of creating a new realm given agent and the following customizations:
    -- TODO: Model creating a new realm.

    -- Step 11: Let topLevelCreationURL be about:blank if embedder is null; otherwise embedder's relevant settings object's top-level creation URL.
    let _topLevelCreationURL : String := "about:blank"
    -- TODO: Model the non-null embedder case for top-level creation URL.

    -- Step 12: Let topLevelOrigin be origin if embedder is null; otherwise embedder's relevant settings object's top-level origin.
    let _topLevelOrigin : Origin := origin
    -- TODO: Model the non-null embedder case for top-level origin.

    -- Step 13: Set up a window environment settings object with about:blank, realm execution context, null, topLevelCreationURL, and topLevelOrigin.
    -- TODO: Model setting up a window environment settings object.

    -- Step 14: Let loadTimingInfo be a new document load timing info with its navigation start time set to the result of calling coarsen time with unsafeContextCreationTime and the new environment settings object's cross-origin isolated capability.
    let loadTimingInfo : DocumentLoadTimingInfo := { navigationStartTime := unsafeContextCreationTime }
    -- TODO: Model coarsen time once the environment settings object exists.

    -- Step 15: Let document be a new Document, with:
    let documentId ← UserAgent.allocateDocumentIdM
    let document : Document := {
      documentId
      origin
      browsingContextId := browsingContext.id
      eventLoopId := agent.eventLoop.id
      permissionsPolicy
      activeSandboxingFlagSet := sandboxFlags
      loadTimingInfo
      aboutBaseURL := creatorBaseURL
    }

    -- Step 16: Let iframeReferrerPolicy be the result of determining the iframe element referrer policy given embedder.
    let iframeReferrerPolicy : Option String := none
    -- TODO: Model determining the iframe element referrer policy.

    -- Step 17: Set document's internal ancestor origin objects list to the result of running the internal ancestor origin objects list creation steps given document and iframeReferrerPolicy.
    let document := {
      document with
        internalAncestorOriginObjectsList :=
          internalAncestorOriginObjectsListCreationSteps document iframeReferrerPolicy
    }

    -- Step 18: Set document's ancestor origins list to the result of running the ancestor origins list creation steps given document.
    let document := {
      document with
        ancestorOriginsList := some (ancestorOriginsListCreationSteps document)
    }

    -- Step 19: If creator is non-null, then:
    let document :=
      if let some creator := creator then
        -- Step 19.1: Set document's referrer to the serialization of creator's URL.
        -- Step 19.2: Set document's policy container to a clone of creator's policy container.
        -- Step 19.3: If creator's origin is same origin with creator's relevant settings object's top-level origin, then set document's opener policy to creator's browsing context's top-level browsing context's active document's opener policy.
        -- TODO: Model creator URL, policy-container cloning, and the same-origin opener-policy copy.
        {
          document with
            referrer := creator.referrer
            policyContainer := creator.policyContainer
            openerPolicy := creator.openerPolicy
        }
      else
        document

    -- Step 20: Assert: document's URL and document's relevant settings object's creation URL are about:blank.
    -- TODO: Model document URL and creation URL.

    -- Step 21: Mark document as ready for post-load tasks.
    -- TODO: Model readiness for post-load tasks.

    -- Step 22: Populate with html/head/body given document.
    modify (fun userAgent => populateWithHtmlHeadBody userAgent document)

    -- Step 23: Make active document.
    -- TODO: Model make active.

    -- Step 24: Completely finish loading document.
    -- TODO: Model completely finish loading.

    -- Step 25: Return browsingContext and document.
    pure (group, browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-browsing-context-group -/
def createNewBrowsingContextGroupAndDocument
    (userAgent : UserAgent) :
    UserAgent × BrowsingContextGroup × BrowsingContext × Document :=
  let ((group, browsingContext, document), userAgent) :=
    createNewBrowsingContextGroupAndDocumentImpl.run userAgent
  (userAgent, group, browsingContext, document)
where
  createNewBrowsingContextAndDocumentM
      (group : BrowsingContextGroup) :
      UserAgent.M (BrowsingContextGroup × BrowsingContext × Document) := do
    let userAgent ← get
    let (userAgent, group, browsingContext, document) :=
      createNewBrowsingContextAndDocument userAgent none none group
    set userAgent
    pure (group, browsingContext, document)

  createNewBrowsingContextGroupAndDocumentImpl :
      UserAgent.M (BrowsingContextGroup × BrowsingContext × Document) := do
    -- Step 1: Let group be a new browsing context group.
    let userAgent ← get
    let (browsingContextGroupSet, group) := userAgent.browsingContextGroupSet.appendFresh

    -- Step 2: Append group to the user agent's browsing context group set.
    set { userAgent with browsingContextGroupSet }

    -- Step 3: Let browsingContext and document be the result of creating a new browsing context and document with null, null, and group.
    let (group, browsingContext, document) ← createNewBrowsingContextAndDocumentM group

    -- Step 4: Append browsingContext to group.
    let (group, browsingContext) := group.append browsingContext
    modify fun userAgent =>
      { userAgent with browsingContextGroupSet := userAgent.browsingContextGroupSet.replace group }

    -- Step 5: Return group and document.
    pure (group, browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-top-level-browsing-context -/
def createNewTopLevelBrowsingContextAndDocument
    (userAgent : UserAgent) :
    UserAgent × BrowsingContext × Document :=
  -- Step 1: Let group and document be the result of creating a new browsing context group and document.
  let (userAgent, _group, browsingContext, document) :=
    createNewBrowsingContextGroupAndDocument userAgent

  -- Step 2: Return group's browsing context set[0] and document.
  (userAgent, browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-top-level-traversable -/
def createNewTopLevelTraversable
    (userAgent : UserAgent)
    (opener : Option Unit)
    (targetName : String)
    (_openerNavigableForWebDriver : Option Unit := none) :
    UserAgent × TopLevelTraversable :=
  let (traversable, userAgent) :=
    (createNewTopLevelTraversableImpl opener targetName).run userAgent
  (userAgent, traversable)
where
  createNewTopLevelBrowsingContextAndDocumentM : UserAgent.M (BrowsingContext × Document) := do
    let userAgent ← get
    let (userAgent, browsingContext, document) :=
      createNewTopLevelBrowsingContextAndDocument userAgent
    set userAgent
    pure (browsingContext, document)

  createNewTopLevelTraversableImpl
      (opener : Option Unit)
      (targetName : String) :
      UserAgent.M TopLevelTraversable := do
    -- Step 1: Let document be null.
    let document : Option Document := none

    -- Step 2: If opener is null, then set document to the second return value of creating a new top-level browsing context and document.
    let (browsingContextId, document) ←
      match opener with
      | none =>
          let (browsingContext, document) ← createNewTopLevelBrowsingContextAndDocumentM
          pure (some browsingContext.id, some document)
      | some _ =>
          pure (none, document)

    -- Step 3: Otherwise, set document to the second return value of creating a new auxiliary browsing context and document given opener.
    -- TODO: Model creating a new auxiliary browsing context and document given opener.

    -- Step 4: Let documentState be a new document state, with
    let documentState : DocumentState := match document with
      | some document => {
          document := some document
          initiatorOrigin := match opener with
            | none => none
            | some _ => some document.origin
          origin := some document.origin
          navigableTargetName := targetName
          aboutBaseURL := document.aboutBaseURL
        }
      | none => {
          initiatorOrigin := none
          navigableTargetName := targetName
        }

    -- Step 5: Let traversable be a new traversable navigable.
    let userAgent ← get
    let (topLevelTraversableSet, traversable) := userAgent.topLevelTraversableSet.appendFresh
    set { userAgent with topLevelTraversableSet }
    let traversable := match document with
      | some document =>
          {
            traversable with
              toTraversableNavigable :=
                {
                  traversable.toTraversableNavigable with
                    toNavigable :=
                      initializeNavigable traversable.toTraversableNavigable.toNavigable document {
                        documentState with everPopulated := true
                      }
                    activeDocument := some document
                }
              parentNavigableIdNone := by rfl
          }
      | none => traversable

    -- Step 6: Initialize the navigable traversable given documentState.
    -- Modeled above when a concrete initial document exists.

    -- Step 7: Let initialHistoryEntry be traversable's active session history entry.
    let initialHistoryEntry : Option SessionHistoryEntry :=
      traversable.toTraversableNavigable.toNavigable.activeSessionHistoryEntry.map fun entry =>
        { entry with step := 0 }

    -- Steps 8-9: Set the initial history entry's step to 0 and append it.
    let traversable := {
      traversable with
        toTraversableNavigable := {
          traversable.toTraversableNavigable with
            activeBrowsingContextId := browsingContextId
            toNavigable := {
              traversable.toTraversableNavigable.toNavigable with
                currentSessionHistoryEntry := initialHistoryEntry
                activeSessionHistoryEntry := initialHistoryEntry
            }
            sessionHistoryEntries := initialHistoryEntry.toList
        }
        isActive := true
        targetName
    }
    modify fun userAgent =>
      setActiveTraversable
        { userAgent with topLevelTraversableSet := userAgent.topLevelTraversableSet.replace traversable }
        traversable.id

    let userAgent ← get
    let some traversable := traversable? userAgent traversable.id
      | pure traversable

    -- Step 10: If opener is non-null, then legacy-clone a traversable storage shed given opener's top-level traversable and traversable.
    -- TODO: Model legacy-clone a traversable storage shed.

    -- Step 11: Append traversable to the user agent's top-level traversable set.
    -- Modeled above by replacing the freshly appended traversable.

    -- Step 12: Invoke WebDriver BiDi navigable created with traversable and openerNavigableForWebDriver.
    -- TODO: Model the WebDriver BiDi hook.

    -- Step 13: Return traversable.
    pure traversable

inductive UserAgentTaskMessage where
  | freshTopLevelTraversable (destinationURL : String)
  | navigateRequested
      (sourceDocumentId : Nat)
      (destinationURL : String)
      (targetName : String)
      (userInvolvement : UserNavigationInvolvement)
      (noopener : Bool)
  | beforeUnloadCompleted
      (documentId : Nat)
      (checkId : Nat)
      (canceled : Bool)
  | finalizeNavigation
      (documentId : Nat)
      (url : String)
  | abortNavigationRequested (documentId : Nat)
  | dispatchEvent (event : String)
  | renderingOpportunity
  | fetchCompleted (fetchId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

def userAgentTaskMessageOfString? (message : String) : Option UserAgentTaskMessage := do
  let messagePrefix := "FreshTopLevelTraversable|"
  let dispatchEventPrefix := "DispatchEvent|"
  if message.startsWith messagePrefix then
    some (.freshTopLevelTraversable (message.drop messagePrefix.length).toString)
  else if message.startsWith dispatchEventPrefix then
    some (.dispatchEvent (message.drop dispatchEventPrefix.length).toString)
  else
    none

private def navigationFailureDetails
    (context : String)
    (destinationURL : String)
    (userAgent : UserAgent)
    (traversableId : Nat) :
    String :=
  let fetchScheme := isFetchScheme destinationURL
  match traversable? userAgent traversableId with
  | none =>
      s!"{context} for traversable {traversableId} to create a pending fetch; traversable missing after navigate, destinationURL={destinationURL}, fetchScheme={fetchScheme}"
  | some traversable =>
      let ongoingNavigation := traversable.toTraversableNavigable.toNavigable.ongoingNavigation
      let activeDocumentUrl :=
        (traversable.toTraversableNavigable.activeDocument.map (·.url)).getD "<none>"
      let ongoingNavigationDescription :=
        match ongoingNavigation with
        | none => "none"
        | some (.navigationId navigationId) => s!"navigationId({navigationId})"
        | some .traversal => "traversal"
      let pendingNavigationFetchDescription :=
        match ongoingNavigation with
        | some (.navigationId navigationId) =>
            match UserAgent.pendingNavigationFetch? userAgent navigationId with
            | some pendingNavigationFetch =>
                s!"present(navigationId={pendingNavigationFetch.navigationId}, requestUrl={pendingNavigationFetch.request.url}, method={pendingNavigationFetch.request.method})"
            | none =>
                s!"missing(navigationId={navigationId})"
        | some .traversal => "not-applicable(traversal)"
        | none => "not-applicable(no ongoing navigation)"
      s!"{context} for traversable {traversableId} to create a pending fetch; destinationURL={destinationURL}, fetchScheme={fetchScheme}, activeDocumentUrl={activeDocumentUrl}, ongoingNavigation={ongoingNavigationDescription}, pendingNavigationFetch={pendingNavigationFetchDescription}"

def startupNavigationFailureDetails
    (destinationURL : String)
    (userAgent : UserAgent)
    (traversableId : Nat) :
    String :=
  navigationFailureDetails
    "expected startup navigation"
    destinationURL
    userAgent
    traversableId

def beforeUnloadNavigationFailureDetails
    (destinationURL : String)
    (userAgent : UserAgent)
    (traversableId : Nat) :
    String :=
  navigationFailureDetails
    "expected deferred navigation after beforeunload"
    destinationURL
    userAgent
    traversableId

def bootstrapFreshTopLevelTraversable
    (destinationURL : String)
    (userAgent : UserAgent) :
    Except String (UserAgent × Nat × PendingFetchRequest) :=
  let (userAgent, traversable) := createNewTopLevelTraversable userAgent none ""
  let (userAgent, pendingFetchRequest) :=
    navigateWithPendingFetchRequest userAgent traversable destinationURL
  match pendingFetchRequest with
  | some pendingFetchRequest => .ok (userAgent, traversable.id, pendingFetchRequest)
  | none => .error (startupNavigationFailureDetails destinationURL userAgent traversable.id)

def activeTraversableReady?
    (userAgent : UserAgent) :
    Option Nat := do
  let traversable <- activeTraversable? userAgent
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let _document <- traversable.toTraversableNavigable.activeDocument
    pure traversable.id

def dispatchEventFailureDetails
    (userAgent : UserAgent)
    (event : String) :
    String :=
  match activeTraversableId? userAgent with
  | none =>
      s!"cannot dispatch event before an active traversable exists: event={event}"
  | some traversableId =>
      match traversable? userAgent traversableId with
      | none =>
          s!"cannot dispatch event for missing active traversable {traversableId}: event={event}"
      | some traversable =>
          if traversable.toTraversableNavigable.activeDocument.isNone then
            s!"cannot dispatch event for active traversable {traversableId} without an active document: event={event}"
          else
            s!"cannot dispatch event for active traversable {traversableId}: unknown dispatch precondition failure, event={event}"

def renderingOpportunityFailureDetails
    (userAgent : UserAgent) :
    String :=
  match activeTraversableId? userAgent with
  | none =>
      "cannot queue update-the-rendering before an active traversable exists"
  | some traversableId =>
      s!"cannot queue update-the-rendering for active traversable {traversableId}"

def createTopLevelTraversableM
    (targetName : String := "") :
    M TopLevelTraversable := do
  let userAgent ← get
  let (nextUserAgent, traversable) := createNewTopLevelTraversable userAgent none targetName
  set nextUserAgent
  let finish : M TopLevelTraversable := do
    tell #[.notifyTopLevelTraversableCreated traversable.id]
    pure traversable
  let some document := traversable.toTraversableNavigable.activeDocument | do
    tell #[.logError
      s!"createTopLevelTraversableM expected an active document for openerless top-level traversable {traversable.id}"]
    finish
  tell #[.startEventLoop document.eventLoopId]
  let some _eventLoop := nextUserAgent.eventLoop? document.eventLoopId | do
    tell #[.logError
      s!"createTopLevelTraversableM expected event loop {document.eventLoopId} for openerless top-level traversable {traversable.id}"]
    finish
  tell #[.eventLoopMessage
    document.eventLoopId
    (.createEmptyDocument traversable.id document.documentId)]
  finish

def navigateM
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none)
    (userInvolvement : UserNavigationInvolvement := .none) :
    M (Option PendingFetchRequest) := do
  let userAgent ← get
  let (nextUserAgent, pendingFetchRequest?) :=
    navigate userAgent traversable destinationURL documentResource userInvolvement
  set nextUserAgent
  match pendingFetchRequest? with
  | some pendingFetchRequest =>
      tell #[.startFetch pendingFetchRequest]
  | none =>
      pure ()
  pure pendingFetchRequest?

def bootstrapFreshTopLevelTraversableM
    (destinationURL : String) :
    M (Except String Nat) := do
  let traversable ← createTopLevelTraversableM
  let pendingFetchRequest? ← navigateM traversable destinationURL
  match pendingFetchRequest? with
  | some _ =>
      pure (.ok traversable.id)
  | none =>
      let userAgent ← get
      let errorMessage := startupNavigationFailureDetails destinationURL userAgent traversable.id
      tell #[.logError errorMessage]
      pure (.error errorMessage)

private def normalizeNavigationTargetName (targetName : String) : String :=
  if targetName.toLower = "_self" then "" else targetName

/-- https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled -/
def advancePendingBeforeUnloadNavigation
    (userAgent : UserAgent)
    (documentId : Nat)
    (checkId : Nat)
    (canceled : Bool) :
    UserAgent :=
  match UserAgent.pendingBeforeUnloadNavigation? userAgent checkId with
  | some pendingBeforeUnloadNavigation =>
      if pendingBeforeUnloadNavigation.documentId != documentId then
        userAgent
      else
        let status :=
          if canceled then
            BeforeUnloadCheckStatus.canceledByBeforeUnload
          else
            BeforeUnloadCheckStatus.continueNavigation
        UserAgent.setPendingBeforeUnloadNavigation
          userAgent
          { pendingBeforeUnloadNavigation with status }
  | none =>
      userAgent

def takeResolvedPendingBeforeUnloadNavigation
    (userAgent : UserAgent)
    (checkId : Nat) :
    UserAgent × Option PendingBeforeUnloadNavigation :=
  match UserAgent.pendingBeforeUnloadNavigation? userAgent checkId with
  | some pendingBeforeUnloadNavigation =>
      match pendingBeforeUnloadNavigation.status with
      | .pending =>
          (userAgent, none)
      | .continueNavigation
      | .canceledByBeforeUnload =>
          userAgent.takePendingBeforeUnloadNavigation checkId
  | none =>
      (userAgent, none)

/-- https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled -/
def checkIfUnloadingIsCanceledM
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (userInvolvement : UserNavigationInvolvement) :
    M BeforeUnloadCheckStatus := do
  -- Step 1: Let documentsToFireBeforeunload be the active document of each item in navigablesThatNeedBeforeUnload.
  -- TODO: Model descendant navigables and the aggregate documentsToFireBeforeunload list.
  -- Note: The current model stores top-level traversables only, so this collapses to at most the target traversable's active document.
  let some document := traversable.toTraversableNavigable.activeDocument
    | pure .continueNavigation
  if document.isInitialAboutBlank then
    -- Step 3: Let finalStatus be "continue".
    pure .continueNavigation
  else
    -- Step 5: Let totalTasks be the size of documentsToFireBeforeunload.
    -- Note: The current model stores at most one top-level active document here, so `totalTasks` is implicit in the pending check record.

    -- Step 6: Let completedTasks be 0.
    -- Note: The current model tracks the single queued completion through `PendingBeforeUnloadNavigation.status` instead of an explicit counter.

    -- Step 7: For each document of documentsToFireBeforeunload, queue a global task on the navigation and traversal task source given document's relevant global object to run the steps:
    -- TODO: Model unloadPromptShown, the traverse navigate-event branch, and the full finalStatus aggregation once descendant navigables are represented.

    -- Step 8: Wait for completedTasks to be totalTasks.
    let userAgent ← get
    let (userAgent, checkId) := userAgent.allocateBeforeUnloadCheckId
    let pending : PendingBeforeUnloadNavigation := {
      checkId
      documentId := document.documentId.id
      traversableId := traversable.id
      destinationURL
      userInvolvement
    }
    let userAgent := userAgent.appendPendingBeforeUnloadNavigation pending
    let some (nextUserAgent, eventLoopId, documentId) :=
        queueRunBeforeUnload userAgent traversable.id
      | set ((userAgent.takePendingBeforeUnloadNavigation checkId).1)
        pure .continueNavigation
    set nextUserAgent
    tell #[.eventLoopMessage eventLoopId (.runBeforeUnload documentId checkId)]
    pure .pending

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def continueNavigationAfterBeforeUnloadM
    (traversableId : Nat)
    (documentId : Option Nat)
    (destinationURL : String)
    (userInvolvement : UserNavigationInvolvement) :
    M Unit := do
  let userAgent ← get

  -- Step 23.3: Queue a global task on the navigation and traversal task source given navigable's active window to abort a document and its descendants given navigable's active document.
  -- Note: The current model applies the queued task's modeled effect eagerly by marking the active document unsalvageable before continuing navigation.
  let userAgent :=
    match documentId with
    | some documentId => abortDocumentAndDescendants userAgent documentId
    | none => userAgent
  set userAgent
  match traversable? userAgent traversableId with
  | some traversable =>
      let readyToContinue :=
        match documentId with
        | some documentId =>
            traversable.toTraversableNavigable.activeDocument.map (·.documentId.id) == some documentId
        | none =>
            true
      match readyToContinue with
      | true =>
          let pendingFetchRequest? ←
            navigateM
              traversable
              destinationURL
              none
              userInvolvement
          match pendingFetchRequest? with
          | some _ =>
              pure ()
          | none =>
              let resumedUserAgent ← get
              tell #[UserAgentEffect.logError
                (beforeUnloadNavigationFailureDetails
                  destinationURL
                  resumedUserAgent
                  traversableId)]
      | false =>
          pure ()
  | none =>
      pure ()

def chooseTargetTraversableM
    (sourceId : Nat)
    (targetName : String)
    (noopener : Bool) :
    M TopLevelTraversable := do
  let normalizedTargetName := normalizeNavigationTargetName targetName
  if noopener || normalizedTargetName.toLower = "_blank" then
    createTopLevelTraversableM
  else
    let userAgent ← get
    if normalizedTargetName.isEmpty then
      match traversable? userAgent sourceId with
      | some traversable =>
          pure traversable
      | none =>
        match traversableContainingDocument? userAgent sourceId with
        | some traversable =>
            pure traversable
        | none =>
            createTopLevelTraversableM
    else
      match traversableWithTargetName? userAgent normalizedTargetName with
      | some traversable =>
          pure traversable
      | none =>
          createTopLevelTraversableM normalizedTargetName

def navigateRequestedM
    (sourceDocumentId : Nat)
    (destinationURL : String)
    (targetName : String)
    (userInvolvement : UserNavigationInvolvement)
    (noopener : Bool) :
    M Unit := do
  tell #[.notifyNavigationRequested destinationURL]
  let traversable ← chooseTargetTraversableM sourceDocumentId targetName noopener
  let unloadStatus ←
    checkIfUnloadingIsCanceledM traversable destinationURL userInvolvement
  match unloadStatus with
  | .continueNavigation =>
      continueNavigationAfterBeforeUnloadM
        traversable.id
        (traversable.toTraversableNavigable.activeDocument.map (·.documentId.id))
        destinationURL
        userInvolvement
  | .pending
  | .canceledByBeforeUnload =>
      pure ()

/-- https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled -/
def beforeUnloadCompletedM
    (documentId : Nat)
    (checkId : Nat)
    (canceled : Bool) :
    M Unit := do
  -- Step 9: Wait for completedTasks to be totalTasks.
  -- Note: The runtime resumes here once the relevant content process reports the queued `beforeunload` task result.
  let userAgent ← get
  let userAgent := advancePendingBeforeUnloadNavigation userAgent documentId checkId canceled
  let (userAgent, pendingBeforeUnloadNavigation?) :=
    takeResolvedPendingBeforeUnloadNavigation userAgent checkId
  set userAgent
  let some pendingBeforeUnloadNavigation := pendingBeforeUnloadNavigation?
    | pure ()
  match pendingBeforeUnloadNavigation.status with
  | .continueNavigation =>
      continueNavigationAfterBeforeUnloadM
        pendingBeforeUnloadNavigation.traversableId
        (some pendingBeforeUnloadNavigation.documentId)
        pendingBeforeUnloadNavigation.destinationURL
        pendingBeforeUnloadNavigation.userInvolvement
  | .pending
  | .canceledByBeforeUnload => do
      tell #[.notifyBeforeUnloadCompleted documentId checkId true]
      pure ()

def abortNavigationRequestedM
    (documentId : Nat) :
    M Unit := do
  let userAgent ← get
  match traversableContainingDocument? userAgent documentId with
  | some traversable =>
    set (abortNavigation userAgent traversable.id)
  | none =>
    pure ()

def dispatchEventM
    (event : String) :
    M Unit := do
  let userAgent ← get
  let some traversableId := activeTraversableId? userAgent
    | tell #[.logError (dispatchEventFailureDetails userAgent event)]
      pure ()
  let some (nextUserAgent, eventLoopId, documentId) := queueDispatchedEvent userAgent traversableId
    | tell #[.logError (dispatchEventFailureDetails userAgent event)]
      pure ()
  set nextUserAgent
  tell #[.eventLoopMessage eventLoopId (.queueDispatchEvent documentId event)]

def queueRenderingOpportunityM : M Unit := do
  let userAgent ← get
  match activeTraversable? userAgent with
  | none =>
      tell #[.logError (renderingOpportunityFailureDetails userAgent)]
  | some traversable =>
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
        set (notePendingUpdateTheRendering userAgent traversable.id)
      else if traversable.toTraversableNavigable.activeDocument.isNone then
        tell #[.logError (renderingOpportunityFailureDetails userAgent)]
      else
        let (nextUserAgent, dispatch?) := queueUpdateTheRendering userAgent traversable.id
        set nextUserAgent
        match dispatch? with
        | some (eventLoopId, eventLoopMessage) =>
            tell #[.eventLoopMessage eventLoopId eventLoopMessage]
        | none =>
            pure ()

def handleFetchCompletedM
    (fetchId : Nat)
    (response : FetchResponse) :
    M Unit := do
  let userAgent ← get
  let some pendingNavigationFetch := UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId
    | pure ()
  let navigationResponse := navigationResponseOfFetchResponse response
  let processedUserAgent :=
    processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId navigationResponse
  let waitingForFinalization :=
    (UserAgent.pendingNavigationFinalizationByNavigationId?
      processedUserAgent
      pendingNavigationFetch.navigationId).isSome
  let (nextUserAgent, renderingDispatch?) :=
    if waitingForFinalization then
      (processedUserAgent, none)
    else
      resumePendingUpdateTheRenderingAfterNavigation
        processedUserAgent
        pendingNavigationFetch.traversableId
  set nextUserAgent
  if let some pendingNavigationFinalization :=
      UserAgent.pendingNavigationFinalizationByNavigationId?
        nextUserAgent
        pendingNavigationFetch.navigationId then
    if let some document := pendingNavigationFinalization.historyEntry.documentState.document then
      if nextUserAgent.eventLoop? document.eventLoopId |>.isSome then
        let eventLoopMessage :=
          if document.url = "about:blank" then
            EventLoopTaskMessage.createEmptyDocument
              pendingNavigationFinalization.traversableId
              document.documentId
          else
            EventLoopTaskMessage.createLoadedDocument
              pendingNavigationFinalization.traversableId
              document.documentId
              navigationResponse
        tell #[.eventLoopMessage document.eventLoopId eventLoopMessage]
  match renderingDispatch? with
  | some (eventLoopId, eventLoopMessage) =>
      tell #[.eventLoopMessage eventLoopId eventLoopMessage]
  | none =>
      pure ()

def finalizeNavigationM
    (documentId : Nat)
    (url : String) :
    M Unit := do
  let userAgent ← get
  let (userAgent, pendingNavigationFinalization?) :=
    userAgent.takePendingNavigationFinalization documentId
  let some pendingNavigationFinalization := pendingNavigationFinalization?
    | set userAgent; pure ()
  if pendingNavigationFinalization.historyEntry.url != url then
    set userAgent
    pure ()
  else
    match traversable? userAgent pendingNavigationFinalization.traversableId with
    | some traversable =>
        if traversable.toTraversableNavigable.toNavigable.ongoingNavigation =
            some (.navigationId pendingNavigationFinalization.navigationId) then
          let staleDocumentDispatch? :=
            match traversable.toTraversableNavigable.activeDocument with
            | some activeDocument =>
                if activeDocument.documentId.id != documentId && !activeDocument.salvageable then
                  some (activeDocument.eventLoopId, EventLoopTaskMessage.destroyDocument activeDocument.documentId)
                else
                  none
            | none =>
                none
          let finalizedTraversable :=
            FormalWeb.finalizeCrossDocumentNavigation
              traversable.toTraversableNavigable.toNavigable
              traversable.toTraversableNavigable.activeDocument
              traversable.toTraversableNavigable.hasDeferredUpdateTheRendering
              traversable.toTraversableNavigable.currentSessionHistoryStep
              traversable.toTraversableNavigable.sessionHistoryEntries
              pendingNavigationFinalization.historyEntry
              pendingNavigationFinalization.historyHandling
              pendingNavigationFinalization.userInvolvement
          let userAgent :=
            replaceTraversable
              userAgent
              {
                traversable with
                  toTraversableNavigable := finalizedTraversable
                  parentNavigableIdNone := by
                    exact
                      (finalizeCrossDocumentNavigation_preserves_parentNavigableId
                        traversable.toTraversableNavigable.toNavigable
                        traversable.toTraversableNavigable.activeDocument
                        traversable.toTraversableNavigable.hasDeferredUpdateTheRendering
                        traversable.toTraversableNavigable.currentSessionHistoryStep
                        traversable.toTraversableNavigable.sessionHistoryEntries
                        pendingNavigationFinalization.historyEntry
                        pendingNavigationFinalization.historyHandling
                        pendingNavigationFinalization.userInvolvement).trans
                          traversable.parentNavigableIdNone
              }
          let (userAgent, renderingDispatch?) :=
            queueUpdateTheRendering userAgent pendingNavigationFinalization.traversableId
          set userAgent
          tell #[.notifyFinalizeNavigation pendingNavigationFinalization.traversableId url]
          match staleDocumentDispatch? with
          | some (eventLoopId, eventLoopMessage) =>
              tell #[UserAgentEffect.eventLoopMessage eventLoopId eventLoopMessage]
          | none =>
              pure ()
          match renderingDispatch? with
          | some (eventLoopId, eventLoopMessage) =>
              tell #[UserAgentEffect.eventLoopMessage eventLoopId eventLoopMessage]
          | none =>
              pure ()
        else
          set userAgent
    | none =>
        set userAgent

def handleUserAgentTaskMessage
    (message : UserAgentTaskMessage) :
    M Unit := do
  match message with
  | .freshTopLevelTraversable destinationURL =>
      let _ ← bootstrapFreshTopLevelTraversableM destinationURL
      pure ()
  | .navigateRequested sourceDocumentId destinationURL targetName userInvolvement noopener =>
    navigateRequestedM sourceDocumentId destinationURL targetName userInvolvement noopener
  | .beforeUnloadCompleted documentId checkId canceled =>
    beforeUnloadCompletedM documentId checkId canceled
  | .finalizeNavigation documentId url =>
    finalizeNavigationM documentId url
  | .abortNavigationRequested documentId =>
    abortNavigationRequestedM documentId
  | .dispatchEvent event =>
      dispatchEventM event
  | .renderingOpportunity =>
      queueRenderingOpportunityM
  | .fetchCompleted fetchId response =>
      handleFetchCompletedM fetchId response

def runMonadic
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    Array UserAgentEffect × UserAgent :=
  let (((), effects), nextUserAgent) :=
    (handleUserAgentTaskMessage message).run userAgent
  (effects, nextUserAgent)


structure UserAgentRuntimeState where
  userAgent : UserAgent := default
  fetchChannel : Std.CloseableChannel FetchRuntimeMessage
  timerChannel : Std.CloseableChannel TimerRuntimeMessage
  eventLoopWorkers : Std.TreeMap Nat EventLoopWorker := Std.TreeMap.empty

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

private def eventLoopWorkersList
    (state : UserAgentRuntimeState) :
    List EventLoopWorker :=
  state.eventLoopWorkers.foldl (fun workers _ worker => worker :: workers) []


private def dispatchEventLoopMessage
    (state : UserAgentRuntimeState)
    (eventLoopId : Nat)
    (message : EventLoopTaskMessage) :
    IO Unit := do
  let some worker := state.eventLoopWorkers.get? eventLoopId | pure ()
  let _ ← worker.channel.trySend message
  pure ()

private def performUserAgentEffect
    (state : UserAgentRuntimeState)
    (effect : UserAgentEffect) :
    IO UserAgentRuntimeState := do
  match effect with
  | .startEventLoop eventLoopId =>
      -- Eagerly create the event-loop worker and register its channel in the global
      -- registry so Rust FFI callers can route messages without going through the UA channel (§2, §3).
      let some eventLoop := state.userAgent.eventLoop? eventLoopId | pure state
      let worker ←
        FormalWeb.startEventLoopWorker
          state.fetchChannel
          state.timerChannel
          eventLoop
      eventLoopChannelRegistry.modify (·.insert eventLoopId worker.channel)
      pure {
        state with
          eventLoopWorkers := state.eventLoopWorkers.insert eventLoopId worker
      }
  | .startFetch request =>
      trySendAndForget state.fetchChannel (.task (.startFetch request))
      pure state
  | .eventLoopMessage eventLoopId eventLoopMessage =>
      dispatchEventLoopMessage state eventLoopId eventLoopMessage
      pure state
    | .notifyTopLevelTraversableCreated webviewId =>
      FormalWeb.sendEmbedderMessage s!"NewTopLevelTraversable|{webviewId}"
      pure state
  | .notifyNavigationRequested destinationURL =>
      FormalWeb.sendEmbedderMessage s!"NavigationRequested|{destinationURL}"
      pure state
  | .notifyBeforeUnloadCompleted documentId checkId canceled =>
      FormalWeb.sendEmbedderMessage
        s!"BeforeUnloadCompleted|{documentId}|{checkId}|{if canceled then "1" else "0"}"
      pure state
    | .notifyFinalizeNavigation webviewId url =>
      FormalWeb.sendEmbedderMessage s!"FinalizeNavigation|{webviewId}|{url}"
      pure state
  | .logError errorMessage =>
      IO.eprintln errorMessage
      pure state

private def runUserAgentTaskMessage
    (state : UserAgentRuntimeState)
    (message : UserAgentTaskMessage) :
    IO UserAgentRuntimeState := do
  let (effects, nextUserAgent) := runMonadic state.userAgent message
  let mut nextState := { state with userAgent := nextUserAgent }
  for effect in effects do
    nextState ← performUserAgentEffect nextState effect
  pure nextState


partial def runUserAgentLoop
    (channel : Std.CloseableChannel UserAgentTaskMessage)
    (state : UserAgentRuntimeState) :
    IO UserAgentRuntimeState := do
  let nextMessage? ← recvCloseableChannel? channel
  match nextMessage? with
  | none =>
      pure state
  | some message =>
      let nextState ← runUserAgentTaskMessage state message
      runUserAgentLoop channel nextState

private def shutdownUserAgentRuntime
    (state : UserAgentRuntimeState) :
    IO Unit := do
  -- Close each channel; the event loop may have already closed its own channel
  -- when it became idle, so guard against the double-close error.
  for worker in eventLoopWorkersList state do
    try worker.channel.close catch _ => pure ()
  for worker in eventLoopWorkersList state do
    let _ ← IO.wait worker.task

def runUserAgent
    (channel : Std.CloseableChannel UserAgentTaskMessage)
    (fetchChannel : Std.CloseableChannel FetchRuntimeMessage)
    (timerChannel : Std.CloseableChannel TimerRuntimeMessage) :
    IO Unit := do
  let initialState : UserAgentRuntimeState := {
    fetchChannel
    timerChannel
  }
  let finalState ← runUserAgentLoop channel initialState
  shutdownUserAgentRuntime finalState

end FormalWeb
