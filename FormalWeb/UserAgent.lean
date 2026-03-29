import Std.Data.TreeMap
import FormalWeb.FFI
import FormalWeb.Fetch
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
  -- Step 1: Let signifier be a new unique internal value.
  let (userAgent, agentId) := userAgent.allocateAgentId
  -- Step 2: Let candidateExecution be a new candidate execution.
  -- TODO: Model candidate execution if scheduling between agents becomes explicit.
  -- Step 3: Let agent be a new agent whose [[CanBlock]] is canBlock, [[Signifier]] is signifier, [[CandidateExecution]] is candidateExecution, and [[IsLockFree1]], [[IsLockFree2]], and [[LittleEndian]] are set at the implementation's discretion.
  -- Step 4: Set agent's event loop to a new event loop.
  let (userAgent, eventLoopId) := userAgent.allocateEventLoopId
  let eventLoop : EventLoop := { id := eventLoopId }
  let userAgent := userAgent.setEventLoop eventLoop
  let agent : Agent := {
    id := agentId
    canBlock
    eventLoop
  }
  -- Step 5: Return agent.
  (userAgent, agent)

/-- https://html.spec.whatwg.org/multipage/#initialise-the-document-object -/
def createAndInitializeDocumentObject
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (navigationParams : NavigationParams) :
    UserAgent × Document :=
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
  let (userAgent, ffiHandle) := userAgent.allocateRustDocumentHandle
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
    referrer := match navigationParams.request with
      | some request => some request.referrer
      | none => none
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

  (userAgent, document)

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
      requestReferrer := match navigationParams.request with
        | some request => request.referrer
        | none => entry.documentState.requestReferrer
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
      match traversable? userAgent pendingNavigationFetch.traversableId with
      | some traversable =>
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation = some (.navigationId navigationId)
      | none => false
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
    else
      let documentResource := entry.documentState.resource
      match documentResource with
      | some (.srcdoc _) =>
          -- TODO: Model create-navigation-params-from-a-srcdoc-resource.
          userAgent
      | _ =>
          let mayFetch :=
            match documentResource with
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

  -- Step 3: If group's cross-origin isolation mode is not "none", then set key to origin.
  let key := if group.crossOriginIsolationMode != .none then AgentClusterKey.origin origin else defaultKey

  -- Step 4: Otherwise, if group's historical agent cluster key map[origin] exists, then set key to group's historical agent cluster key map[origin].
  let key := if group.crossOriginIsolationMode != .none then
    key
  else
    match group.historicalAgentClusterKey origin with
    | some historicalKey => historicalKey
    | none => key

  -- Step 5: Otherwise:
  let (group, key) := if group.crossOriginIsolationMode != .none || (group.historicalAgentClusterKey origin).isSome then
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
          isOriginKeyed := match key with
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
  -- Step 1: Let browsingContext be a new browsing context.
  let browsingContext : BrowsingContext := { id := group.nextBrowsingContextId }

  -- Step 2: Let unsafeContextCreationTime be the unsafe shared current time.
  let unsafeContextCreationTime : Nat := 0

  -- Step 3: Let creatorOrigin be null.
  let creatorOrigin : Option Origin := none

  -- Step 4: Let creatorBaseURL be null.
  let creatorBaseURL : Option String := none

  -- Step 5: If creator is non-null, then:
  let (creatorOrigin, creatorBaseURL, browsingContext) := match creator with
    | some creator =>
        -- Step 5.1: Set creatorOrigin to creator's origin.
        -- Step 5.2: Set creatorBaseURL to creator's document base URL.
        -- Step 5.3: Set browsingContext's virtual browsing context group ID to creator's browsing context's top-level browsing context's virtual browsing context group ID.
        -- TODO: Model virtual browsing context group IDs.
        (some creator.origin, creator.aboutBaseURL, browsingContext)
    | none =>
        (creatorOrigin, creatorBaseURL, browsingContext)

  -- Step 6: Let sandboxFlags be the result of determining the creation sandboxing flags given browsingContext and embedder.
  let sandboxFlags := determineCreationSandboxingFlags browsingContext embedder

  -- Step 7: Let origin be the result of determining the origin given about:blank, sandboxFlags, and creatorOrigin.
  let origin := determineOrigin "about:blank" sandboxFlags creatorOrigin

  -- Step 8: Let permissionsPolicy be the result of creating a permissions policy given embedder and origin.
  let permissionsPolicy := createPermissionsPolicy embedder origin

  -- Step 9: Let agent be the result of obtaining a similar-origin window agent given origin, group, and false.
  let (userAgent, group, _agent) := obtainSimilarOriginWindowAgent userAgent origin group false

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
  let (userAgent, ffiHandle) := userAgent.allocateRustDocumentHandle
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
  let document := match creator with
    | some creator =>
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
    | none => document

  -- Step 20: Assert: document's URL and document's relevant settings object's creation URL are about:blank.
  -- TODO: Model document URL and creation URL.

  -- Step 21: Mark document as ready for post-load tasks.
  -- TODO: Model readiness for post-load tasks.

  -- Step 22: Populate with html/head/body given document.
  let userAgent := populateWithHtmlHeadBody userAgent document

  -- Step 23: Make active document.
  -- TODO: Model make active.

  -- Step 24: Completely finish loading document.
  -- TODO: Model completely finish loading.

  -- Step 25: Return browsingContext and document.
  (userAgent, group, browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-browsing-context-group -/
def createNewBrowsingContextGroupAndDocument
    (userAgent : UserAgent) :
    UserAgent × BrowsingContextGroup × BrowsingContext × Document :=
  -- Step 1: Let group be a new browsing context group.
  let (browsingContextGroupSet, group) := userAgent.browsingContextGroupSet.appendFresh

  -- Step 2: Append group to the user agent's browsing context group set.
  let userAgent := { userAgent with browsingContextGroupSet }

  -- Step 3: Let browsingContext and document be the result of creating a new browsing context and document with null, null, and group.
  let (userAgent, group, browsingContext, document) :=
    createNewBrowsingContextAndDocument userAgent none none group

  -- Step 4: Append browsingContext to group.
  let (group, browsingContext) := group.append browsingContext
  let browsingContextGroupSet := userAgent.browsingContextGroupSet.replace group
  let userAgent := { userAgent with browsingContextGroupSet }

  -- Step 5: Return group and document.
  (userAgent, group, browsingContext, document)

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
  -- Step 1: Let document be null.
  let document : Option Document := none

  -- Step 2: If opener is null, then set document to the second return value of creating a new top-level browsing context and document.
  let (userAgent, browsingContextId, document) := match opener with
    | none =>
        let (userAgent, browsingContext, document) :=
          createNewTopLevelBrowsingContextAndDocument userAgent
        (userAgent, some browsingContext.id, some document)
    | some _ =>
        (userAgent, none, document)

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
  let (topLevelTraversableSet, traversable) := userAgent.topLevelTraversableSet.appendFresh
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
    traversable.toTraversableNavigable.toNavigable.activeSessionHistoryEntry

  -- Steps 8-9: Set the initial history entry's step to 0 and append it.
  let traversable := {
    traversable with
      toTraversableNavigable := {
        traversable.toTraversableNavigable with
          activeBrowsingContextId := browsingContextId
          toNavigable := {
            traversable.toTraversableNavigable.toNavigable with
              currentSessionHistoryEntry := match initialHistoryEntry with
                | some initialHistoryEntry => some { initialHistoryEntry with step := 0 }
                | none => none
              activeSessionHistoryEntry := match initialHistoryEntry with
                | some initialHistoryEntry => some { initialHistoryEntry with step := 0 }
                | none => none
          }
          sessionHistoryEntries := match initialHistoryEntry with
            | some initialHistoryEntry => [{ initialHistoryEntry with step := 0 }]
            | none => []
      }
      targetName
  }
  let topLevelTraversableSet := topLevelTraversableSet.replace traversable

  -- Step 10: If opener is non-null, then legacy-clone a traversable storage shed given opener's top-level traversable and traversable.
  -- TODO: Model legacy-clone a traversable storage shed.

  -- Step 11: Append traversable to the user agent's top-level traversable set.
  let userAgent := { userAgent with topLevelTraversableSet }

  -- Step 12: Invoke WebDriver BiDi navigable created with traversable and openerNavigableForWebDriver.
  -- TODO: Model the WebDriver BiDi hook.

  -- Step 13: Return traversable.
  (userAgent, traversable)

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

end FormalWeb
