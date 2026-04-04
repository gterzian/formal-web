import Std.Data.TreeMap
import Std.Sync.Channel
import FormalWeb.EventLoop
import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.Traversable

namespace FormalWeb

/-- Model-local routing metadata for a document-driven fetch initiated by the embedder runtime. -/
structure PendingDocumentFetch where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- Model-local opaque pointer to the boxed Rust-side Blitz `NetHandler`. -/
  handler : RustNetHandlerPointer
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/--
The user agent is the top-level global state for the browser model.
-/
structure UserAgent where
  /-- Model-local allocator state for https://dom.spec.whatwg.org/#concept-document -/
  nextRustDocumentHandleId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#agent-cluster -/
  nextAgentClusterId : Nat := 0
  /-- Model-local allocator state for https://tc39.es/ecma262/#sec-agents -/
  nextAgentId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#event-loop -/
  nextEventLoopId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
  nextNavigationId : Nat := 0
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
  /-- Model-local queue of document-driven fetches waiting to hand results back to embedder-side handlers. -/
  pendingDocumentFetches : Std.TreeMap Nat PendingDocumentFetch := Std.TreeMap.empty
deriving Repr

instance : Inhabited UserAgent where
  default := {}

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

def allocateRustDocumentHandle (userAgent : UserAgent) : UserAgent × RustDocumentHandle :=
  let handle : RustDocumentHandle := { id := userAgent.nextRustDocumentHandleId }
  let userAgent := {
    userAgent with
      nextRustDocumentHandleId := userAgent.nextRustDocumentHandleId + 1
  }
  (userAgent, handle)

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

def allocateFetchId (userAgent : UserAgent) : UserAgent × Nat :=
  let fetchId := userAgent.nextFetchId
  let userAgent := { userAgent with nextFetchId := userAgent.nextFetchId + 1 }
  (userAgent, fetchId)

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

def appendPendingDocumentFetch
    (userAgent : UserAgent)
    (pendingDocumentFetch : PendingDocumentFetch) :
    UserAgent :=
  {
    userAgent with
      pendingDocumentFetches :=
        userAgent.pendingDocumentFetches.insert pendingDocumentFetch.fetchId pendingDocumentFetch
  }

def pendingDocumentFetch?
    (userAgent : UserAgent)
    (fetchId : Nat) :
    Option PendingDocumentFetch :=
  userAgent.pendingDocumentFetches.get? fetchId

def takePendingDocumentFetch
    (userAgent : UserAgent)
    (fetchId : Nat) :
    UserAgent × Option PendingDocumentFetch :=
  let result := userAgent.pendingDocumentFetch? fetchId
  let userAgent := {
    userAgent with
      pendingDocumentFetches := userAgent.pendingDocumentFetches.erase fetchId
  }
  (userAgent, result)

private def allocateRustDocumentHandleM : M RustDocumentHandle := do
  let userAgent ← get
  let (userAgent, handle) := userAgent.allocateRustDocumentHandle
  set userAgent
  pure handle

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

def activeDocumentHandle?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option RustDocumentHandle := do
  let some traversable := traversable? userAgent traversableId | none
  let some document := traversable.toTraversableNavigable.activeDocument | none
  pure document.ffiHandle

def queueCreateEmptyDocument
    (userAgent : UserAgent)
    (document : Document) :
    Option (Nat × EventLoopTaskMessage) := do
  let _eventLoop <- userAgent.eventLoop? document.eventLoopId
  pure (document.eventLoopId, .createEmptyDocument document.ffiHandle)

def queueCreateLoadedDocument
    (userAgent : UserAgent)
    (document : Document)
    (response : NavigationResponse) :
    Option (Nat × EventLoopTaskMessage) := do
  let _eventLoop <- userAgent.eventLoop? document.eventLoopId
  pure (document.eventLoopId, .createLoadedDocument document.ffiHandle response.url response.body)

def queuePaintDocument
    (eventLoopId : Nat)
    (documentId : RustDocumentHandle) :
    Option (Nat × EventLoopTaskMessage) :=
  some (eventLoopId, .queuePaint documentId)

def queueUpdateTheRendering
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option (UserAgent × Nat × EventLoopTaskMessage) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  let documentId <- activeDocumentHandle? userAgent traversableId
  let eventLoop <- userAgent.eventLoop? document.eventLoopId
  if eventLoop.hasPendingUpdateTheRendering then
    none
  else
    let eventLoop := eventLoop.enqueueUpdateTheRenderingTask
    pure (
      userAgent.setEventLoop eventLoop,
      document.eventLoopId,
      EventLoopTaskMessage.queueUpdateTheRendering traversableId documentId
    )

def queueDispatchedEvent
    (userAgent : UserAgent)
    (traversableId : Nat)
    (event : String) :
    Option (UserAgent × Nat × EventLoopTaskMessage) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  let documentId <- activeDocumentHandle? userAgent traversableId
  let eventLoop <- userAgent.eventLoop? document.eventLoopId
  let eventLoop := eventLoop.enqueueTask { step := .dispatchEvent }
  pure (
    userAgent.setEventLoop eventLoop,
    document.eventLoopId,
    .queueDispatchEvent documentId event
  )

def completeUpdateTheRendering
    (userAgent : UserAgent)
    (traversableId : Nat)
    (eventLoopId : Nat)
    (documentId : RustDocumentHandle) :
    Option UserAgent := do
  let traversable <- traversable? userAgent traversableId
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let document <- traversable.toTraversableNavigable.activeDocument
    if document.eventLoopId != eventLoopId || document.ffiHandle != documentId then
      none
    else
      let eventLoop <- userAgent.eventLoop? eventLoopId
      if !eventLoop.hasPendingUpdateTheRendering then
        none
      else
        let eventLoop := eventLoop.dequeueUpdateTheRenderingTask
        pure (userAgent.setEventLoop eventLoop)

def dropPendingUpdateTheRenderingCompletion
    (userAgent : UserAgent)
    (eventLoopId : Nat) :
    Option UserAgent := do
  let eventLoop <- userAgent.eventLoop? eventLoopId
  if !eventLoop.hasPendingUpdateTheRendering then
    none
  else
    let eventLoop := eventLoop.dequeueUpdateTheRenderingTask
    pure (userAgent.setEventLoop eventLoop)

def requestDocumentFetch
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    UserAgent × PendingDocumentFetch × DocumentFetchRequest :=
  let (userAgent, fetchId) := userAgent.allocateFetchId
  let pendingDocumentFetch : PendingDocumentFetch := {
    fetchId
    handler
    request
  }
  let userAgent := userAgent.appendPendingDocumentFetch pendingDocumentFetch
  let documentFetchRequest : DocumentFetchRequest := {
    fetchId
    request
  }
  (userAgent, pendingDocumentFetch, documentFetchRequest)

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

/-- Model-local helper that installs a fetched HTML document from the response body. -/
def populateWithLoadedHtmlDocument
    (userAgent : UserAgent)
    (document : Document)
    (response : NavigationResponse) :
    UserAgent :=
  let _ := document
  let _ := response
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
    let browsingContextId := traversable.toTraversableNavigable.activeBrowsingContextId.getD 0
    -- TODO: Model obtaining a browsing context to use for a navigation response, including COOP-triggered group switches.

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
    let ffiHandle ← UserAgent.allocateRustDocumentHandleM
    let document : Document := {
      ffiHandle
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
    let userAgent := populateWithLoadedHtmlDocument userAgent document navigationParams.response
    -- Notes: Incremental parser input and streaming fetch delivery remain future work.
    (userAgent, document)

/-- https://html.spec.whatwg.org/multipage/#loading-a-document -/
def loadDocument
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams)
    (_sourceSnapshotParams : SourceSnapshotParams)
    (_initiatorOrigin : Option Origin) :
    UserAgent × Option Document :=
  if navigationParams.response.contentType = "text/html" then
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
  let (userAgent, document) :=
    loadDocument
      userAgent
      traversable
      navigationParams
      sourceSnapshotParams
      entry.documentState.initiatorOrigin
  let documentState := {
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
  let historyEntry : SessionHistoryEntry := {
    entry with
      documentState
      step := traversable.toTraversableNavigable.currentSessionHistoryStep + 1
  }
  let updatedNavigable :=
    {
      traversable.toTraversableNavigable.toNavigable with
        currentSessionHistoryEntry := some historyEntry
        activeSessionHistoryEntry := some historyEntry
        ongoingNavigation := none
    }
  let traversable := {
    traversable with
      toTraversableNavigable := {
        traversable.toTraversableNavigable with
          toNavigable := updatedNavigable
          sessionHistoryEntries :=
            traversable.toTraversableNavigable.sessionHistoryEntries.concat historyEntry
          currentSessionHistoryStep := historyEntry.step
          activeDocument := document
      }
      parentNavigableIdNone := by
        simpa [updatedNavigable] using
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
  finishCreatingNavigationParamsByFetching userAgent navigationId none

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
def navigateWithPendingFetchRequest
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none) :
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
      .none
      navigationId
      none
      "other"
      true
    (userAgent, UserAgent.pendingNavigationFetchRequest? userAgent navigationId)

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigate
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none) :
    UserAgent :=
  (navigateWithPendingFetchRequest userAgent traversable destinationURL documentResource).1

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
    let ffiHandle ← UserAgent.allocateRustDocumentHandleM
    let document : Document := {
      ffiHandle
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
        targetName
    }
    modify fun userAgent =>
      { userAgent with topLevelTraversableSet := userAgent.topLevelTraversableSet.replace traversable }

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
  | documentFetchRequested (handler : RustNetHandlerPointer) (request : NavigationRequest)
  | dispatchEvent (event : String)
  | renderingOpportunity
  | updateTheRenderingCompleted
      (traversableId : Nat)
      (eventLoopId : Nat)
      (documentId : RustDocumentHandle)
  | fetchCompleted (fetchId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

structure UserAgentTaskState where
  userAgent : UserAgent := default
  startupTraversableId : Option Nat := none
  lastDispatchedEvent : Option String := none
deriving Repr, Inhabited

structure DocumentFetchCompletion where
  handler : RustNetHandlerPointer
  resolvedUrl : String
  body : ByteArray
deriving Repr

structure EventLoopDispatch where
  eventLoopId : Nat
  message : EventLoopTaskMessage
deriving Repr, DecidableEq

structure UserAgentTaskResult where
  state : UserAgentTaskState
  fetchMessages : List FetchTaskMessage := []
  eventLoopDispatches : List EventLoopDispatch := []
  documentFetchCompletions : List DocumentFetchCompletion := []
  sentNewTopLevelTraversable : Bool := false
  error : Option String := none
deriving Repr

def userAgentTaskMessageOfString? (message : String) : Option UserAgentTaskMessage := do
  let messagePrefix := "FreshTopLevelTraversable|"
  let dispatchEventPrefix := "DispatchEvent|"
  if message.startsWith messagePrefix then
    some (.freshTopLevelTraversable (message.drop messagePrefix.length).toString)
  else if message.startsWith dispatchEventPrefix then
    some (.dispatchEvent (message.drop dispatchEventPrefix.length).toString)
  else
    none

def startupNavigationFailureDetails
    (destinationURL : String)
    (userAgent : UserAgent)
    (traversableId : Nat) :
    String :=
  let fetchScheme := isFetchScheme destinationURL
  match traversable? userAgent traversableId with
  | none =>
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; traversable missing after navigate, destinationURL={destinationURL}, fetchScheme={fetchScheme}"
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
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; destinationURL={destinationURL}, fetchScheme={fetchScheme}, activeDocumentUrl={activeDocumentUrl}, ongoingNavigation={ongoingNavigationDescription}, pendingNavigationFetch={pendingNavigationFetchDescription}"

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

def startupDocumentDispatch?
    (userAgent : UserAgent)
    (traversableId : Nat) :
  Option (Nat × EventLoopTaskMessage) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  queueCreateEmptyDocument userAgent document

def navigationDocumentDispatch?
    (userAgent : UserAgent)
    (traversableId : Nat)
    (response : NavigationResponse) :
  Option (Nat × EventLoopTaskMessage) := do
  let traversable <- traversable? userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  if document.url = "about:blank" then
    queueCreateEmptyDocument userAgent document
  else
    queueCreateLoadedDocument userAgent document response

def startupTraversableReady?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option Unit := do
  let traversable <- traversable? userAgent traversableId
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let _document <- traversable.toTraversableNavigable.activeDocument
    pure ()

def dispatchEventFailureDetails
    (state : UserAgentTaskState)
    (event : String) :
    String :=
  match state.startupTraversableId with
  | none =>
      s!"cannot dispatch event before startup traversable exists: event={event}"
  | some traversableId =>
      match traversable? state.userAgent traversableId with
      | none =>
          s!"cannot dispatch event for missing startup traversable {traversableId}: event={event}"
      | some traversable =>
          if traversable.toTraversableNavigable.activeDocument.isNone then
            s!"cannot dispatch event for startup traversable {traversableId} without an active document: event={event}"
          else
            s!"cannot dispatch event for startup traversable {traversableId}: unknown dispatch precondition failure, event={event}"

def renderingOpportunityFailureDetails
    (state : UserAgentTaskState) :
    String :=
  match state.startupTraversableId with
  | none =>
      "cannot queue update-the-rendering before startup traversable exists"
  | some traversableId =>
      s!"cannot queue update-the-rendering for startup traversable {traversableId}"

def updateTheRenderingCompletionFailureDetails
    (traversableId : Nat)
    (eventLoopId : Nat) :
    String :=
  s!"cannot apply completed update-the-rendering for traversable {traversableId} on event loop {eventLoopId}"

def documentFetchCompletionDispatch?
    (state : UserAgentTaskState)
  (_fetchId : Nat)
    (pendingDocumentFetch : PendingDocumentFetch)
    (response : FetchResponse) :
    Option (Nat × EventLoopTaskMessage) := do
  let traversableId <- state.startupTraversableId
  let traversable <- traversable? state.userAgent traversableId
  let document <- traversable.toTraversableNavigable.activeDocument
  let _eventLoop <- state.userAgent.eventLoop? document.eventLoopId
  pure (
    document.eventLoopId,
    .queueDocumentFetchCompletion pendingDocumentFetch.handler response.url response.body
  )

def handleUserAgentTaskMessagePure
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    UserAgentTaskResult :=
  match message with
  | .freshTopLevelTraversable destinationURL =>
      match bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | .ok (userAgent, traversableId, pendingFetchRequest) =>
          let eventLoopDispatches :=
          match startupDocumentDispatch? userAgent traversableId with
          | some (eventLoopId, eventLoopMessage) =>
            [{ eventLoopId, message := eventLoopMessage }]
          | none =>
            []
          {
            state := {
              state with
                userAgent
                startupTraversableId := some traversableId
            }
            fetchMessages := [.startFetch pendingFetchRequest]
            eventLoopDispatches
          }
      | .error error =>
          { state, error := some error }
  | .documentFetchRequested handler request =>
      let (userAgent, _pendingDocumentFetch, documentFetchRequest) :=
        requestDocumentFetch state.userAgent handler request
      {
        state := { state with userAgent }
        fetchMessages := [.startDocumentFetch documentFetchRequest]
      }
  | .dispatchEvent event =>
        match state.startupTraversableId with
        | none =>
          { state, error := some (dispatchEventFailureDetails state event) }
        | some traversableId =>
          match traversable? state.userAgent traversableId with
          | none =>
            { state, error := some (dispatchEventFailureDetails state event) }
          | some traversable =>
            match traversable.toTraversableNavigable.activeDocument with
            | none =>
              { state, error := some (dispatchEventFailureDetails state event) }
            | some _document =>
              match queueDispatchedEvent state.userAgent traversableId event with
              | some (userAgent, eventLoopId, eventLoopMessage) =>
                  {
                    state := {
                      state with
                        userAgent
                        lastDispatchedEvent := some event
                    }
                    eventLoopDispatches := [{ eventLoopId, message := eventLoopMessage }]
                  }
              | none =>
                  { state, error := some (dispatchEventFailureDetails state event) }
  | .renderingOpportunity =>
      match state.startupTraversableId with
      | none =>
          { state, error := some (renderingOpportunityFailureDetails state) }
      | some traversableId =>
        match startupTraversableReady? state.userAgent traversableId with
        | none =>
          { state }
        | some _ =>
          match queueUpdateTheRendering state.userAgent traversableId with
          | some (userAgent, eventLoopId, eventLoopMessage) =>
            {
            state := { state with userAgent }
            eventLoopDispatches := [{ eventLoopId, message := eventLoopMessage }]
            }
          | none =>
                  { state }
  | .updateTheRenderingCompleted traversableId eventLoopId documentId =>
      match completeUpdateTheRendering state.userAgent traversableId eventLoopId documentId with
      | some userAgent =>
          match queuePaintDocument eventLoopId documentId with
          | some (paintEventLoopId, eventLoopMessage) =>
              {
                state := { state with userAgent }
                eventLoopDispatches := [{ eventLoopId := paintEventLoopId, message := eventLoopMessage }]
              }
          | none =>
              {
                state := { state with userAgent }
                error := some (updateTheRenderingCompletionFailureDetails traversableId eventLoopId)
              }
      | none =>
          match dropPendingUpdateTheRenderingCompletion state.userAgent eventLoopId with
          | some userAgent =>
              { state := { state with userAgent } }
          | none =>
              {
                state
                error := some (updateTheRenderingCompletionFailureDetails traversableId eventLoopId)
              }
  | .fetchCompleted fetchId response =>
      match UserAgent.pendingNavigationFetchByFetchId? state.userAgent fetchId with
      | some pendingNavigationFetch =>
          let navigationResponse := navigationResponseOfFetchResponse response
          let userAgent :=
            processNavigationFetchResponse state.userAgent pendingNavigationFetch.navigationId navigationResponse
          let eventLoopDispatches :=
            match navigationDocumentDispatch? userAgent pendingNavigationFetch.traversableId navigationResponse with
            | some (eventLoopId, eventLoopMessage) =>
                [{ eventLoopId, message := eventLoopMessage }]
            | none =>
                []
          let sentNewTopLevelTraversable :=
            match state.startupTraversableId with
            | none => false
            | some traversableId =>
                (startupTraversableReady? userAgent traversableId).isSome
          {
            state := { state with userAgent }
            eventLoopDispatches
            sentNewTopLevelTraversable
          }
      | none =>
          match UserAgent.pendingDocumentFetch? state.userAgent fetchId with
          | some pendingDocumentFetch =>
            let userAgent := (state.userAgent.takePendingDocumentFetch fetchId).1
            match documentFetchCompletionDispatch? state fetchId pendingDocumentFetch response with
            | some (eventLoopId, eventLoopMessage) =>
              {
              state := { state with userAgent }
              eventLoopDispatches := [{ eventLoopId, message := eventLoopMessage }]
              }
            | none =>
              { state := { state with userAgent } }
          | none =>
              { state }

def userAgentTaskStep
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    UserAgentTaskState :=
  (handleUserAgentTaskMessagePure state message).state

def userAgentTaskExec
    (state : UserAgentTaskState)
    (messages : List UserAgentTaskMessage) :
    UserAgentTaskState :=
  messages.foldl userAgentTaskStep state

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def notifyStartupTraversableReady
    (userAgent : UserAgent)
    (traversableId : Nat) :
    IO Unit := do
  let some _ := startupTraversableReady? userAgent traversableId | pure ()
  FormalWeb.sendEmbedderMessage "NewTopLevelTraversable"

def runUserAgentMessage
    (enqueueFetchMessage : FetchTaskMessage -> IO Unit)
  (ensureEventLoopWorker : EventLoop -> IO Unit)
  (enqueueEventLoopMessage : Nat -> EventLoopTaskMessage -> IO Unit)
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    IO UserAgentTaskState := do
  let result := handleUserAgentTaskMessagePure state message
  let nextState := result.state
  if let some error := result.error then
    IO.eprintln s!"handleUserAgentTaskMessagePure failed: {error}"
  for fetchMessage in result.fetchMessages do
    enqueueFetchMessage fetchMessage
  for eventLoopDispatch in result.eventLoopDispatches do
    let some eventLoop := nextState.userAgent.eventLoop? eventLoopDispatch.eventLoopId | pure ()
    ensureEventLoopWorker eventLoop
    enqueueEventLoopMessage eventLoopDispatch.eventLoopId eventLoopDispatch.message
  match message with
  | .freshTopLevelTraversable _ =>
      pure ()
  | .documentFetchRequested _ _ =>
      pure ()
  | .dispatchEvent _event =>
      pure ()
  | .renderingOpportunity =>
      pure ()
  | .updateTheRenderingCompleted _ _ _ =>
      pure ()
  | .fetchCompleted _ _ =>
      if result.sentNewTopLevelTraversable then
        let some traversableId := nextState.startupTraversableId | pure ()
        notifyStartupTraversableReady nextState.userAgent traversableId
  pure nextState

partial def runUserAgent
    (channel : Std.CloseableChannel UserAgentTaskMessage)
    (enqueueFetchMessage : FetchTaskMessage -> IO Unit)
    (ensureEventLoopWorker : EventLoop -> IO Unit)
    (enqueueEventLoopMessage : Nat -> EventLoopTaskMessage -> IO Unit)
    (state : UserAgentTaskState := default) :
    IO Unit := do
  let some message ← recvCloseableChannel? channel | pure ()
  let state ← runUserAgentMessage enqueueFetchMessage ensureEventLoopWorker enqueueEventLoopMessage state message
  runUserAgent channel enqueueFetchMessage ensureEventLoopWorker enqueueEventLoopMessage state

end FormalWeb
