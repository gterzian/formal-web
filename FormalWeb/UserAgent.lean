import Std.Data.TreeMap
import Std.Sync.Channel
import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.TransitionTrace
import FormalWeb.Traversable

namespace FormalWeb

/--
The user agent is the top-level global state for the browser model.
-/
structure UserAgent where
  /-- Model-local allocator state for https://dom.spec.whatwg.org/#concept-document -/
  nextRustDocumentHandleId : Nat := 0
  /-- Model-local map from document handles to opaque Rust-side document pointers. -/
  rustDocumentPointers : Std.TreeMap RustDocumentHandle RustDocumentPointer := Std.TreeMap.empty
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#agent-cluster -/
  nextAgentClusterId : Nat := 0
  /-- Model-local allocator state for https://tc39.es/ecma262/#sec-agents -/
  nextAgentId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#event-loop -/
  nextEventLoopId : Nat := 0
  /-- Model-local allocator state for https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
  nextNavigationId : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  browsingContextGroupSet : BrowsingContextGroupSet := {}
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  topLevelTraversableSet : TopLevelTraversableSet := {}
  /-- Model-local map from https://html.spec.whatwg.org/multipage/#event-loop identifiers to event-loop objects. -/
  eventLoops : Std.TreeMap Nat EventLoop := Std.TreeMap.empty
  /-- Model-local queue of fetch-backed navigations suspended in https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
  pendingNavigationFetches : Std.TreeMap Nat PendingNavigationFetch := Std.TreeMap.empty
  /-- Model-local map from traversable id to the latest `BaseDocument` pointer produced by an UpdateTheRendering task. -/
  baseDocumentPointers : Std.TreeMap Nat RustBaseDocumentPointer := Std.TreeMap.empty
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

def setRustDocumentPointer
    (userAgent : UserAgent)
    (handle : RustDocumentHandle)
    (pointer : RustDocumentPointer) :
    UserAgent :=
  {
    userAgent with
      rustDocumentPointers := userAgent.rustDocumentPointers.insert handle pointer
  }

def rustDocumentPointer?
    (userAgent : UserAgent)
    (handle : RustDocumentHandle) :
    Option RustDocumentPointer :=
  userAgent.rustDocumentPointers.get? handle

def documentHtml
    (userAgent : UserAgent)
    (document : Document) :
    String :=
  match userAgent.rustDocumentPointer? document.ffiHandle with
  | none => "<missing rust document pointer>"
  | some pointer =>
      if pointer = RustDocumentPointer.null then
        "<null rust document pointer>"
      else
        renderHtmlDocument pointer.raw

def allocateRustDocumentHandle (userAgent : UserAgent) : UserAgent × RustDocumentHandle :=
  let handle : RustDocumentHandle := { id := userAgent.nextRustDocumentHandleId }
  let userAgent := {
    userAgent with
      nextRustDocumentHandleId := userAgent.nextRustDocumentHandleId + 1
      rustDocumentPointers := userAgent.rustDocumentPointers.insert handle RustDocumentPointer.null
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
  }

def takePendingNavigationFetch
    (userAgent : UserAgent)
    (navigationId : Nat) :
    UserAgent × Option PendingNavigationFetch :=
  let result := userAgent.pendingNavigationFetches.get? navigationId
  let userAgent := { userAgent with pendingNavigationFetches := userAgent.pendingNavigationFetches.erase navigationId }
  (userAgent, result)

def pendingNavigationFetch?
    (userAgent : UserAgent)
    (navigationId : Nat) :
    Option PendingNavigationFetch :=
  userAgent.pendingNavigationFetches.get? navigationId

/-- Model-local bridge from the HTML navigation wait state to the runtime fetch worker. -/
def pendingNavigationFetchRequest?
    (userAgent : UserAgent)
    (navigationId : Nat) :
    Option PendingFetchRequest := do
  let pendingNavigationFetch <- userAgent.pendingNavigationFetch? navigationId
  pure {
    navigationId := pendingNavigationFetch.navigationId
    request := pendingNavigationFetch.request
  }

def setBaseDocumentPointer
    (userAgent : UserAgent)
    (traversableId : Nat)
    (pointer : RustBaseDocumentPointer) :
    UserAgent :=
  { userAgent with
      baseDocumentPointers := userAgent.baseDocumentPointers.insert traversableId pointer }

def baseDocumentPointer?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option RustBaseDocumentPointer :=
  userAgent.baseDocumentPointers.get? traversableId

private def allocateRustDocumentHandleM : M RustDocumentHandle := do
  let userAgent ← get
  let (userAgent, handle) := userAgent.allocateRustDocumentHandle
  set userAgent
  pure handle

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

def noteRenderingOpportunity
    (userAgent : UserAgent)
    (traversableId : Nat) :
    IO Unit := do
  let some traversable := traversable? userAgent traversableId | pure ()
  let some document := traversable.toTraversableNavigable.activeDocument | pure ()
  let some pointer := userAgent.rustDocumentPointer? document.ffiHandle | pure ()
  if pointer = RustDocumentPointer.null then
    pure ()
  else
    let baseDocumentPointer := FormalWeb.extractBaseDocument pointer.raw
    FormalWeb.queuePaint baseDocumentPointer.raw

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
  -- Notes: The current model performs those DOM allocations and append operations with one Rust-side helper that creates the initial html/head/body skeleton.
  let pointer := createEmptyHtmlDocument ()
  -- Notes: Store the resulting opaque Rust document pointer on the Lean-side document handle.
  userAgent.setRustDocumentPointer document.ffiHandle pointer

/-- Model-local helper that installs a fixed fetched HTML document for navigation demos. -/
def populateWithLoadedHtmlDocument
    (userAgent : UserAgent)
    (document : Document) :
    UserAgent :=
  let pointer := createLoadedHtmlDocument ()
  userAgent.setRustDocumentPointer document.ffiHandle pointer

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

    -- Step 12: Let document be a new Document, with:
    let ffiHandle ← UserAgent.allocateRustDocumentHandleM
    let document : Document := {
      ffiHandle
      origin := navigationParams.origin
      browsingContextId
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
    -- Notes: The current model stubs the parser-driven branch by installing a fixed Rust-side HTML document that contains the text "Loaded!".
    let userAgent := populateWithLoadedHtmlDocument userAgent document
    -- Notes: Fetch delivery and incremental parser input remain future work.
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
  let pendingNavigationFetch : PendingNavigationFetch := {
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
    let (group, _agent) ← obtainSimilarOriginWindowAgentM origin group false

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

/--
LTS-style actions for the current user-agent navigation model.
-/
inductive UserAgentAction
  | createTopLevelTraversable (targetName : String := "")
  | beginNavigation
      (traversableId : Nat)
      (destinationURL : String)
      (documentResource : Option DocumentResource := none)
  | completeNavigation (navigationId : Nat) (response : NavigationResponse)
  | abortNavigation (traversableId : Nat)
  /--
  Models the user agent sending a NavigationFinished user event to the winit app.
  Pre-condition: the traversable has an active document and no ongoing navigation.
  The app responds by calling `request_redraw()` and sending an UpdateTheRendering message.
  -/
  | navigationFinished (traversableId : Nat)
  /--
  Models the user agent receiving an UpdateTheRendering message from the winit app
  and enqueuing an UpdateTheRendering task on the given event loop, deduplicating if
  one is already pending. This can happen at any time, but only if the event loop exists.
  -/
  | queueUpdateTheRendering (traversableId : Nat) (eventLoopId : Nat)
  /--
  Models the event-loop task running: Rust has extracted the `BaseDocument` pointer
  from the `HtmlDocument`, stored it, and sent a Paint user event to the winit app.
  Clears `hasPendingUpdateTheRendering` on the event loop. This requires the
  traversable's navigation to have completed.
  -/
  | updateTheRendering (traversableId : Nat) (eventLoopId : Nat) (baseDocPointer : RustBaseDocumentPointer)
deriving Repr, DecidableEq

/--
Apply one user-agent transition.

This sits above helper algorithms such as `navigate` and
`processNavigationFetchResponse`, which implement the details of each labeled step.
-/
def step
    (userAgent : UserAgent)
    (action : UserAgentAction) :
    Option UserAgent := do
  match action with
  | .createTopLevelTraversable targetName =>
      let (userAgent, _traversable) := createNewTopLevelTraversable userAgent none targetName
      pure userAgent
  | .beginNavigation traversableId destinationURL documentResource =>
      let traversable <- traversable? userAgent traversableId
      pure (navigate userAgent traversable destinationURL documentResource)
  | .completeNavigation navigationId response =>
      pure (processNavigationFetchResponse userAgent navigationId response)
  | .abortNavigation traversableId =>
      pure (abortNavigation userAgent traversableId)
  | .navigationFinished traversableId =>
      -- Pre-condition: traversable exists, has an active document, and has no ongoing navigation.
      -- The action label models the UA sending NavigationFinished to the winit app.
      let traversable <- traversable? userAgent traversableId
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
        none
      else if traversable.toTraversableNavigable.activeDocument.isNone then
        none
      else
        pure userAgent
  | .queueUpdateTheRendering traversableId eventLoopId =>
      -- Models the UA receiving UpdateTheRendering from the winit app and enqueuing the task (dedup).
      -- This action is allowed regardless of navigation state, but only if the referenced event loop exists.
      let _traversable <- traversable? userAgent traversableId
      let eventLoop <- userAgent.eventLoop? eventLoopId
      let eventLoop := eventLoop.enqueueUpdateTheRenderingTask
      pure (userAgent.setEventLoop eventLoop)
  | .updateTheRendering traversableId eventLoopId baseDocPointer =>
      -- Models the event-loop task running: BaseDocument extracted, Paint user event sent to winit.
      -- Clears hasPendingUpdateTheRendering and records the latest base document pointer.
      -- This requires the traversable to have an active document and no ongoing navigation.
      let traversable <- traversable? userAgent traversableId
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
        none
      else if traversable.toTraversableNavigable.activeDocument.isNone then
        none
      else
        let eventLoop <- userAgent.eventLoop? eventLoopId
        if !eventLoop.hasPendingUpdateTheRendering then
          none
        else
          let eventLoop := eventLoop.dequeueUpdateTheRenderingTask
          let userAgent := userAgent.setEventLoop eventLoop
          pure (userAgent.setBaseDocumentPointer traversableId baseDocPointer)

inductive UserAgentTaskMessage where
  | freshTopLevelTraversable (destinationURL : String)
  | renderingOpportunity
  | fetchCompleted (navigationId : Nat) (response : NavigationResponse)
deriving Repr, DecidableEq

structure UserAgentTaskState where
  userAgent : UserAgent := default
  startupTraversableId : Option Nat := none
deriving Repr, Inhabited

structure UserAgentTaskResult where
  state : UserAgentTaskState
  fetchMessages : List FetchTaskMessage := []
  sentNewTopLevelTraversable : Bool := false
  error : Option String := none
deriving Repr

def userAgentTaskMessageOfString? (message : String) : Option UserAgentTaskMessage := do
  let messagePrefix := "FreshTopLevelTraversable|"
  if message.startsWith messagePrefix then
    some (.freshTopLevelTraversable (message.drop messagePrefix.length).toString)
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

def startupTraversableReadyHtml?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option String := do
  let traversable <- traversable? userAgent traversableId
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let document <- traversable.toTraversableNavigable.activeDocument
    pure (UserAgent.documentHtml userAgent document)

def handleUserAgentTaskMessagePure
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    UserAgentTaskResult :=
  match message with
  | .freshTopLevelTraversable destinationURL =>
      match bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | .ok (userAgent, traversableId, pendingFetchRequest) =>
          {
            state := {
              state with
                userAgent
                startupTraversableId := some traversableId
            }
            fetchMessages := [.startFetch pendingFetchRequest]
          }
      | .error error =>
          { state, error := some error }
  | .renderingOpportunity =>
      { state }
  | .fetchCompleted navigationId response =>
      let userAgent := processNavigationFetchResponse state.userAgent navigationId response
      let sentNewTopLevelTraversable :=
        match state.startupTraversableId with
        | none => false
        | some traversableId =>
            (startupTraversableReadyHtml? userAgent traversableId).isSome
      {
        state := { state with userAgent }
        sentNewTopLevelTraversable
      }

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

inductive UserAgentTaskMessageActionShape : UserAgentTaskMessage → List UserAgentAction → Prop where
  | freshTopLevelTraversableError
      (destinationURL : String) :
      UserAgentTaskMessageActionShape (.freshTopLevelTraversable destinationURL) []
  | freshTopLevelTraversableSuccess
      (destinationURL : String)
      (traversableId : Nat) :
      UserAgentTaskMessageActionShape
        (.freshTopLevelTraversable destinationURL)
        [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
  | renderingOpportunity :
      UserAgentTaskMessageActionShape .renderingOpportunity []
  | fetchCompleted
      (navigationId : Nat)
      (response : NavigationResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted navigationId response)
        [.completeNavigation navigationId response]

theorem createTopLevelTraversable_trace
    (userAgent : UserAgent)
    (targetName : String := "") :
    TransitionTrace
      step
      userAgent
      [.createTopLevelTraversable targetName]
      (createNewTopLevelTraversable userAgent none targetName).1 := by
  refine TransitionTrace.single ?_
  simp [step, createNewTopLevelTraversable]

theorem createNewTopLevelTraversable_lookup
    (userAgent : UserAgent)
    (targetName : String := "") :
    let result := createNewTopLevelTraversable userAgent none targetName
    traversable? result.1 result.2.id = some result.2 := by
  simp [createNewTopLevelTraversable, traversable?, TopLevelTraversableSet.find?]
  unfold createNewTopLevelTraversable.createNewTopLevelTraversableImpl
  simp [TopLevelTraversableSet.appendFresh, TopLevelTraversableSet.nextId, TopLevelTraversableSet.replace]

theorem beginNavigation_after_createTopLevelTraversable_trace
    (userAgent : UserAgent)
    (destinationURL : String)
    (targetName : String := "") :
    let created := createNewTopLevelTraversable userAgent none targetName
    TransitionTrace
      step
      created.1
      [.beginNavigation created.2.id destinationURL none]
      (navigate created.1 created.2 destinationURL) := by
  intro created
  refine TransitionTrace.single ?_
  have hlookup : traversable? created.1 created.2.id = some created.2 := by
    simpa [created] using createNewTopLevelTraversable_lookup userAgent targetName
  simp [step, navigate, hlookup]

theorem startupSuccess_trace
    (userAgent nextUserAgent : UserAgent)
    (destinationURL : String)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    TransitionTrace
      step
      userAgent
      [
        .createTopLevelTraversable "",
        .beginNavigation traversableId destinationURL none
      ]
      nextUserAgent := by
  unfold bootstrapFreshTopLevelTraversable at hbootstrap
  let created := createNewTopLevelTraversable userAgent none ""
  let navigated := navigateWithPendingFetchRequest created.1 created.2 destinationURL
  cases hpending : navigated.2 with
  | none =>
      simp [created, navigated, hpending] at hbootstrap
  | some actualPendingFetchRequest =>
      cases hnav : navigated with
      | mk actualNextUserAgent actualPendingFetchRequest? =>
          have hpending' : actualPendingFetchRequest? = some actualPendingFetchRequest := by
            simpa [hnav] using hpending
          simp [created, navigated, hnav, hpending'] at hbootstrap
          rcases hbootstrap with ⟨hnextUserAgent, htraversableId, hpendingFetchRequest⟩
          subst hnextUserAgent
          subst htraversableId
          subst hpendingFetchRequest
          refine TransitionTrace.cons (intermediate := created.1) ?_ ?_
          · simpa [created] using
              (show step userAgent (.createTopLevelTraversable "") = some (createNewTopLevelTraversable userAgent none "").1 by
                simp [step, createNewTopLevelTraversable])
          · simpa [created, navigate, navigated, hnav] using
              beginNavigation_after_createTopLevelTraversable_trace userAgent destinationURL ""

theorem handleUserAgentTaskMessagePure_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      TransitionTrace
        step
        state.userAgent
        actions
        (handleUserAgentTaskMessagePure state message).state.userAgent := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | error _ =>
          refine ⟨[], .freshTopLevelTraversableError destinationURL, ?_⟩
          simp [handleUserAgentTaskMessagePure, hbootstrap, TransitionTrace.nil]
      | ok result =>
          refine ⟨
            [.createTopLevelTraversable "", .beginNavigation result.2.1 destinationURL none],
            .freshTopLevelTraversableSuccess destinationURL result.2.1,
            ?_
          ⟩
          simpa [handleUserAgentTaskMessagePure, hbootstrap] using
            startupSuccess_trace state.userAgent result.1 destinationURL result.2.1 result.2.2 hbootstrap
  | renderingOpportunity =>
      refine ⟨[], .renderingOpportunity, ?_⟩
      simp [handleUserAgentTaskMessagePure, TransitionTrace.nil]
  | fetchCompleted navigationId response =>
      refine ⟨[.completeNavigation navigationId response], .fetchCompleted navigationId response, ?_⟩
      refine TransitionTrace.single ?_
      simp [handleUserAgentTaskMessagePure, step, processNavigationFetchResponse]

theorem userAgentTaskStep_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      TransitionTrace
        step
        state.userAgent
        actions
        (userAgentTaskStep state message).userAgent := by
  simpa [userAgentTaskStep] using handleUserAgentTaskMessagePure_refines state message

theorem userAgentTaskExec_refines
    (state : UserAgentTaskState)
    (messages : List UserAgentTaskMessage) :
    ∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
        (userAgentTaskExec state messages).userAgent := by
  induction messages generalizing state with
  | nil =>
      refine ⟨[], ?_⟩
      simp [userAgentTaskExec, TransitionTrace.nil]
  | cons message messages ih =>
      have hstep := userAgentTaskStep_refines state message
      have htail := ih (userAgentTaskStep state message)
      rcases hstep with ⟨actions₁, _shape, htrace₁⟩
      rcases htail with ⟨actions₂, htrace₂⟩
      refine ⟨actions₁ ++ actions₂, ?_⟩
      simpa [userAgentTaskExec] using TransitionTrace.append htrace₁ htrace₂

theorem default_userAgentTaskState_empty
    (traversableId navigationId : Nat) :
    (default : UserAgentTaskState).startupTraversableId = none ∧
    (traversable? (default : UserAgentTaskState).userAgent traversableId).isNone = true ∧
    (UserAgent.pendingNavigationFetch? (default : UserAgentTaskState).userAgent navigationId).isNone = true := by
  refine ⟨rfl, ?_, ?_⟩
  · change
      (match (Std.TreeMap.empty : Std.TreeMap Nat TopLevelTraversable)[traversableId]? with
        | some _ => false
        | none => true) = true
    simp
  · change
      (match (Std.TreeMap.empty : Std.TreeMap Nat PendingNavigationFetch)[navigationId]? with
        | some _ => false
        | none => true) = true
    simp

theorem userAgentTaskExec_startup_from_default_success
    (destinationURL : String)
    (nextUserAgent : UserAgent)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL (default : UserAgentTaskState).userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    (userAgentTaskExec (default : UserAgentTaskState) [.freshTopLevelTraversable destinationURL]).startupTraversableId = some traversableId ∧
    (userAgentTaskExec (default : UserAgentTaskState) [.freshTopLevelTraversable destinationURL]).userAgent = nextUserAgent ∧
    (handleUserAgentTaskMessagePure (default : UserAgentTaskState) (.freshTopLevelTraversable destinationURL)).fetchMessages =
      [.startFetch pendingFetchRequest] ∧
    (handleUserAgentTaskMessagePure (default : UserAgentTaskState) (.freshTopLevelTraversable destinationURL)).error = none ∧
    TransitionTrace
      step
      (default : UserAgentTaskState).userAgent
      [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
      (userAgentTaskExec (default : UserAgentTaskState) [.freshTopLevelTraversable destinationURL]).userAgent := by
  refine ⟨?_, ?_, ?_, ?_, ?_⟩
  · simp [userAgentTaskExec, userAgentTaskStep, handleUserAgentTaskMessagePure, hbootstrap]
  · simp [userAgentTaskExec, userAgentTaskStep, handleUserAgentTaskMessagePure, hbootstrap]
  · simp [handleUserAgentTaskMessagePure, hbootstrap]
  · simp [handleUserAgentTaskMessagePure, hbootstrap]
  · simpa [userAgentTaskExec, userAgentTaskStep, handleUserAgentTaskMessagePure, hbootstrap] using
      startupSuccess_trace
        (default : UserAgentTaskState).userAgent
        nextUserAgent
        destinationURL
        traversableId
        pendingFetchRequest
        hbootstrap

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def notifyStartupTraversableReady
    (userAgent : UserAgent)
    (traversableId : Nat) :
    IO Unit := do
  let some _html := startupTraversableReadyHtml? userAgent traversableId | pure ()
  FormalWeb.sendRuntimeMessage "NewTopLevelTraversable"

def runUserAgentMessage
    (enqueueFetchMessage : FetchTaskMessage -> IO Unit)
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    IO UserAgentTaskState := do
  let result := handleUserAgentTaskMessagePure state message
  let nextState := result.state
  if let some error := result.error then
    IO.eprintln s!"handleUserAgentTaskMessagePure failed: {error}"
  for fetchMessage in result.fetchMessages do
    enqueueFetchMessage fetchMessage
  match message with
  | .freshTopLevelTraversable _ =>
      pure ()
  | .renderingOpportunity =>
      let some traversableId := nextState.startupTraversableId | pure ()
      noteRenderingOpportunity nextState.userAgent traversableId
  | .fetchCompleted _ _ =>
      if result.sentNewTopLevelTraversable then
        let some traversableId := nextState.startupTraversableId | pure ()
        notifyStartupTraversableReady nextState.userAgent traversableId
  pure nextState

partial def runUserAgent
    (channel : Std.CloseableChannel UserAgentTaskMessage)
    (enqueueFetchMessage : FetchTaskMessage -> IO Unit)
    (state : UserAgentTaskState := default) :
    IO Unit := do
  let some message ← recvCloseableChannel? channel | pure ()
  let state ← runUserAgentMessage enqueueFetchMessage state message
  runUserAgent channel enqueueFetchMessage state

end FormalWeb
