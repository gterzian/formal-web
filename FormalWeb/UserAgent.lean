namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/#concept-origin -/
structure Origin where
  /-- https://html.spec.whatwg.org/multipage/#ascii-serialisation-of-an-origin -/
  serialization : String
  /-- Model-local cache of the result of https://html.spec.whatwg.org/multipage/#obtain-a-site -/
  site : String
deriving Repr, DecidableEq

def aboutBlankOrigin : Origin :=
  { serialization := "about:blank", site := "about:blank" }

/-- https://html.spec.whatwg.org/multipage/#cross-origin-isolation-mode -/
inductive CrossOriginIsolationMode
  | none
  | logical
  | concrete
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#agent-cluster-key -/
inductive AgentClusterKey
  | site (site : String)
  | origin (origin : Origin)
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#task-source -/
inductive TaskSource
  | generic
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#concept-task -/
structure Task where
  /-- Model-local summary of https://html.spec.whatwg.org/multipage/#concept-task-steps -/
  stepsDescription : String
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
deriving Repr, DecidableEq

/-- https://tc39.es/ecma262/#sec-agents -/
structure Agent where
  /-- Model-local identifier standing in for the signifier allocated by https://html.spec.whatwg.org/multipage/#create-an-agent -/
  id : Nat
  /-- https://tc39.es/ecma262/#sec-agents -/
  canBlock : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#concept-agent-event-loop -/
  eventLoop : EventLoop
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation -/
structure AgentCluster where
  /-- Model-local identifier for https://html.spec.whatwg.org/multipage/#agent-cluster -/
  id : Nat
  crossOriginIsolationMode : CrossOriginIsolationMode := .none
  /-- https://html.spec.whatwg.org/multipage/#is-origin-keyed -/
  isOriginKeyed : Bool := false
  /-- The single https://html.spec.whatwg.org/multipage/#similar-origin-window-agent contained in this browsing context agent cluster. -/
  similarOriginWindowAgent : Agent
deriving Repr, DecidableEq

/-- Placeholder for the Rust-side DOM object backing a spec-level document. -/
structure RustDocumentHandle where
  /-- Model-local handle for https://dom.spec.whatwg.org/#concept-document -/
  id : Nat
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document-load-timing-info -/
structure DocumentLoadTimingInfo where
  /-- https://html.spec.whatwg.org/multipage/#navigation-start-time -/
  navigationStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-interactive-time -/
  domInteractiveTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-content-loaded-event-start-time -/
  domContentLoadedEventStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-content-loaded-event-end-time -/
  domContentLoadedEventEndTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-complete-time -/
  domCompleteTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#load-event-start-time -/
  loadEventStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#load-event-end-time -/
  loadEventEndTime : Nat := 0
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#cross-origin-opener-policy-value -/
inductive OpenerPolicyValue
  | unsafeNone
  | sameOriginAllowPopups
  | sameOrigin
  | noopenerAllowPopups
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#cross-origin-opener-policy -/
structure OpenerPolicy where
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-value -/
  value : OpenerPolicyValue := .unsafeNone
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-endpoint -/
  reportingEndpoint : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-only-value -/
  reportOnlyValue : OpenerPolicyValue := .unsafeNone
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-only-endpoint -/
  reportOnlyReportingEndpoint : Option String := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#policy-container -/
structure PolicyContainer where
  /-- https://html.spec.whatwg.org/multipage/#policy-container-csp-list -/
  cspList : List String := []
  /-- https://html.spec.whatwg.org/multipage/#policy-container-embedder-policy -/
  embedderPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-referrer-policy -/
  referrerPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-integrity-policy -/
  integrityPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-report-only-integrity-policy -/
  reportOnlyIntegrityPolicy : Option String := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
structure PermissionsPolicy where
  placeholder : Bool := true
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#active-sandboxing-flag-set -/
abbrev SandboxingFlagSet := List String

/-- Placeholder for the created custom element registry. -/
structure CustomElementRegistry where
  initialized : Bool := true
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document -/
structure Document where
  /-- Model-local handle for the host-side DOM object for https://dom.spec.whatwg.org/#concept-document -/
  ffiHandle : RustDocumentHandle
  /-- https://dom.spec.whatwg.org/#concept-document-type -/
  type : String := "html"
  /-- https://dom.spec.whatwg.org/#concept-document-content-type -/
  contentType : String := "text/html"
  /-- https://dom.spec.whatwg.org/#concept-document-mode -/
  mode : String := "quirks"
  /-- https://dom.spec.whatwg.org/#concept-document-origin -/
  origin : Origin
  /-- https://html.spec.whatwg.org/multipage/#concept-document-bc -/
  browsingContextId : Nat
  /-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
  permissionsPolicy : PermissionsPolicy := {}
  /-- https://html.spec.whatwg.org/multipage/#active-sandboxing-flag-set -/
  activeSandboxingFlagSet : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#load-timing-info -/
  loadTimingInfo : DocumentLoadTimingInfo := {}
  /-- https://html.spec.whatwg.org/multipage/#is-initial-about:blank -/
  isInitialAboutBlank : Bool := true
  /-- https://html.spec.whatwg.org/multipage/#concept-document-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://dom.spec.whatwg.org/#concept-document-allow-declarative-shadow-roots -/
  allowDeclarativeShadowRoots : Bool := true
  /-- https://dom.spec.whatwg.org/#document-custom-element-registry -/
  customElementRegistry : CustomElementRegistry := {}
  /-- https://html.spec.whatwg.org/multipage/#concept-document-internal-ancestor-origin-objects-list -/
  internalAncestorOriginObjectsList : List Origin := []
  /-- https://html.spec.whatwg.org/multipage/#concept-document-ancestor-origins-list -/
  ancestorOriginsList : Option (List Origin) := none
  /-- https://html.spec.whatwg.org/multipage/#the-document's-referrer -/
  referrer : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#concept-document-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://html.spec.whatwg.org/multipage/#concept-document-coop -/
  openerPolicy : OpenerPolicy := {}
  /-- https://dom.spec.whatwg.org/#concept-document-url -/
  url : String := "about:blank"
deriving Repr, DecidableEq

/-- https://w3c.github.io/navigation-timing/#dom-navigationtimingtype -/
inductive NavigationTimingType
  | navigate
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#user-navigation-involvement -/
inductive UserNavigationInvolvement
  | none
  | browserUI
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#source-snapshot-params -/
structure SourceSnapshotParams where
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-activation -/
  hasTransientActivation : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-client -/
  fetchClientId : Option Nat := none
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-policy-container -/
  sourcePolicyContainer : PolicyContainer := {}
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#target-snapshot-params -/
structure TargetSnapshotParams where
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params-sandbox -/
  sandboxingFlags : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params-iframe-referrer-policy -/
  iframeElementReferrerPolicy : Option String := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document-state-2 -/
structure DocumentState where
  /-- https://html.spec.whatwg.org/multipage/#document-state-request-referrer-policy -/
  requestReferrerPolicy : String := ""
  /-- https://html.spec.whatwg.org/multipage/#document-state-initiator-origin -/
  initiatorOrigin : Option Origin := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-resource -/
  resource : Option Unit := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-nav-target-name -/
  navigableTargetName : String := ""
  /-- https://html.spec.whatwg.org/multipage/#document-state-document -/
  document : Option Document := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-origin -/
  origin : Option Origin := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-history-policy-container -/
  historyPolicyContainer : Option PolicyContainer := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-request-referrer -/
  requestReferrer : String := "client"
  /-- https://html.spec.whatwg.org/multipage/#document-state-ever-populated -/
  everPopulated : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#document-state-reload-pending -/
  reloadPending : Bool := false
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#session-history-entry -/
structure SessionHistoryEntry where
  /-- https://html.spec.whatwg.org/multipage/#she-url -/
  url : String
  /-- https://html.spec.whatwg.org/multipage/#she-document-state -/
  documentState : DocumentState
  /-- https://html.spec.whatwg.org/multipage/#she-step -/
  step : Nat := 0
deriving Repr, DecidableEq

/-- https://fetch.spec.whatwg.org/#concept-request -/
structure NavigationRequest where
  /-- https://fetch.spec.whatwg.org/#concept-request-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-request-method -/
  method : String := "GET"
  /-- https://fetch.spec.whatwg.org/#concept-request-referrer -/
  referrer : String := "client"
  /-- https://fetch.spec.whatwg.org/#concept-request-referrer-policy -/
  referrerPolicy : String := ""
  /-- https://fetch.spec.whatwg.org/#concept-request-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://fetch.spec.whatwg.org/#concept-request-body -/
  body : Option String := none
deriving Repr, DecidableEq

/-- https://fetch.spec.whatwg.org/#concept-response -/
structure NavigationResponse where
  /-- https://fetch.spec.whatwg.org/#concept-response-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-response-status -/
  status : Nat := 200
  /-- Minimal MIME type surface for loading-a-document dispatch. -/
  contentType : String := "text/html"
  /-- Placeholder response body for the future parser/runtime model. -/
  body : String := ""
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#navigation-params -/
structure NavigationParams where
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  id : Nat
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-navigable -/
  traversableId : Nat
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-request -/
  request : Option NavigationRequest := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-response -/
  response : NavigationResponse
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-fetch-controller -/
  fetchControllerId : Option Nat := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-origin -/
  origin : Origin
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-sandboxing -/
  finalSandboxingFlagSet : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-iframe-referrer-policy -/
  iframeElementReferrerPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-coop -/
  coop : OpenerPolicy := {}
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-nav-timing-type -/
  navigationTimingType : NavigationTimingType := .navigate
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-user-involvement -/
  userInvolvement : UserNavigationInvolvement := .none
deriving Repr, DecidableEq

/-- Pending fetch-backed navigation paused at the spec's wait-for-response point. -/
structure PendingNavigationFetch where
  /-- Model-local identifier corresponding to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- Model-local reference to https://html.spec.whatwg.org/multipage/#navigation-params-navigable -/
  traversableId : Nat
  /-- https://html.spec.whatwg.org/multipage/#session-history-entry -/
  historyEntry : SessionHistoryEntry
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params -/
  sourceSnapshotParams : SourceSnapshotParams
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params -/
  targetSnapshotParams : TargetSnapshotParams
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-nav-timing-type -/
  navTimingType : NavigationTimingType := .navigate
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-user-involvement -/
  userInvolvement : UserNavigationInvolvement := .none
  /-- Model-local summary of CSP navigation type from https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
  cspNavigationType : String := "other"
  /-- Model-local flag for the POST special-case in https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document -/
  allowPOST : Bool := false
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/--
LTS-style actions for the current user-agent navigation model.
-/
inductive UserAgentAction
  | createTopLevelTraversable (targetName : String := "")
  | beginNavigation (traversableId : Nat) (destinationURL : String)
  | completeNavigation (navigationId : Nat) (response : NavigationResponse)
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#top-level-browsing-context -/
structure BrowsingContext where
  /-- Model-local identifier for https://html.spec.whatwg.org/multipage/#browsing-context -/
  id : Nat
  /-- https://html.spec.whatwg.org/multipage/#tlbc-group -/
  groupId : Option Nat := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#browsing-context-group -/
structure BrowsingContextGroup where
  /-- Model-local identifier for https://html.spec.whatwg.org/multipage/#browsing-context-group -/
  id : Nat
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-set -/
  browsingContextSet : List BrowsingContext := []
  /-- https://html.spec.whatwg.org/multipage/#agent-cluster-map -/
  agentClusterMap : List (AgentClusterKey × AgentCluster) := []
  /-- https://html.spec.whatwg.org/multipage/#historical-agent-cluster-key-map -/
  historicalAgentClusterKeyMap : List (Origin × AgentClusterKey) := []
  /-- https://html.spec.whatwg.org/multipage/#bcg-cross-origin-isolation -/
  crossOriginIsolationMode : CrossOriginIsolationMode := .none
deriving Repr

/-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
structure BrowsingContextGroupSet where
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  members : List BrowsingContextGroup := []
deriving Repr

/-- https://html.spec.whatwg.org/multipage/#navigable -/
structure Navigable where
  /-- https://html.spec.whatwg.org/multipage/#nav-parent -/
  parentNavigableId : Option Nat := none
  /-- https://html.spec.whatwg.org/multipage/#nav-current-history-entry -/
  currentSessionHistoryEntry : Option SessionHistoryEntry := none
  /-- https://html.spec.whatwg.org/multipage/#nav-active-history-entry -/
  activeSessionHistoryEntry : Option SessionHistoryEntry := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#traversable-navigable -/
structure TraversableNavigable extends Navigable where
  /-- Model-local reference to the browsing context controlled by this traversable.
      Related spec concept: https://html.spec.whatwg.org/multipage/#browsing-context -/
  activeBrowsingContextId : Option Nat := none
  /-- Model-local cache of the Document presented via https://html.spec.whatwg.org/multipage/#nav-active-history-entry -/
  activeDocument : Option Document := none
  /-- https://html.spec.whatwg.org/multipage/#tn-current-session-history-step -/
  currentSessionHistoryStep : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#tn-session-history-entries -/
  sessionHistoryEntries : List SessionHistoryEntry := []
  /-- Model-local identifier for the navigation currently in flight for this traversable.
      Related spec concept: https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
  ongoingNavigationId : Option Nat := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#top-level-traversable -/
structure TopLevelTraversable where
  /-- https://html.spec.whatwg.org/multipage/#traversable-navigable -/
  toTraversableNavigable : TraversableNavigable := {}
  /-- Model-local identifier for https://html.spec.whatwg.org/multipage/#top-level-traversable -/
  id : Nat
  /-- Model-local mirror of https://html.spec.whatwg.org/multipage/#document-state-nav-target-name for the active entry. -/
  targetName : String := ""
  /-- https://html.spec.whatwg.org/multipage/#nav-parent -/
  parentNavigableIdNone : toTraversableNavigable.toNavigable.parentNavigableId = none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
structure TopLevelTraversableSet where
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  members : List TopLevelTraversable := []
deriving Repr

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
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  browsingContextGroupSet : BrowsingContextGroupSet := {}
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  topLevelTraversableSet : TopLevelTraversableSet := {}
  /-- Model-local map from https://html.spec.whatwg.org/multipage/#event-loop identifiers to event-loop objects. -/
  eventLoops : List (Nat × EventLoop) := []
  /-- Model-local queue of fetch-backed navigations suspended in https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
  pendingNavigationFetches : List PendingNavigationFetch := []
deriving Repr

namespace UserAgent

private def setEventLoopEntry
    (entries : List (Nat × EventLoop))
    (eventLoop : EventLoop) :
    List (Nat × EventLoop) :=
  match entries with
  | [] => [(eventLoop.id, eventLoop)]
  | (entryId, entryEventLoop) :: rest =>
      if entryId = eventLoop.id then
        (eventLoop.id, eventLoop) :: rest
      else
        (entryId, entryEventLoop) :: setEventLoopEntry rest eventLoop

def setEventLoop
    (userAgent : UserAgent)
    (eventLoop : EventLoop) :
    UserAgent :=
  {
    userAgent with
      eventLoops := setEventLoopEntry userAgent.eventLoops eventLoop
  }

def allocateRustDocumentHandle (userAgent : UserAgent) : UserAgent × RustDocumentHandle :=
  let handle : RustDocumentHandle := { id := userAgent.nextRustDocumentHandleId }
  let userAgent := { userAgent with nextRustDocumentHandleId := userAgent.nextRustDocumentHandleId + 1 }
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

private def takePendingNavigationFetchEntries
    (pendingNavigationFetches : List PendingNavigationFetch)
    (navigationId : Nat) :
    Option PendingNavigationFetch × List PendingNavigationFetch :=
  match pendingNavigationFetches with
  | [] => (none, [])
  | pendingNavigationFetch :: rest =>
      if pendingNavigationFetch.navigationId = navigationId then
        (some pendingNavigationFetch, rest)
      else
        let (result, rest) := takePendingNavigationFetchEntries rest navigationId
        (result, pendingNavigationFetch :: rest)

def appendPendingNavigationFetch
    (userAgent : UserAgent)
    (pendingNavigationFetch : PendingNavigationFetch) :
    UserAgent :=
  {
    userAgent with
      pendingNavigationFetches := userAgent.pendingNavigationFetches.concat pendingNavigationFetch
  }

def takePendingNavigationFetch
    (userAgent : UserAgent)
    (navigationId : Nat) :
    UserAgent × Option PendingNavigationFetch :=
  let (pendingNavigationFetch, pendingNavigationFetches) :=
    takePendingNavigationFetchEntries userAgent.pendingNavigationFetches navigationId
  ({ userAgent with pendingNavigationFetches }, pendingNavigationFetch)

end UserAgent

namespace EventLoop

def enqueueTask
    (eventLoop : EventLoop)
    (task : Task) :
    EventLoop :=
  {
    eventLoop with
      taskQueue := eventLoop.taskQueue.concat task
  }

end EventLoop

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

namespace BrowsingContextGroup

private def lookupAgentCluster
    (entries : List (AgentClusterKey × AgentCluster))
    (key : AgentClusterKey) :
    Option AgentCluster :=
  match entries with
  | [] => none
  | (entryKey, agentCluster) :: rest =>
      if entryKey = key then some agentCluster else lookupAgentCluster rest key

private def setAgentClusterEntry
    (entries : List (AgentClusterKey × AgentCluster))
    (key : AgentClusterKey)
    (agentCluster : AgentCluster) :
    List (AgentClusterKey × AgentCluster) :=
  match entries with
  | [] => [(key, agentCluster)]
  | (entryKey, entryValue) :: rest =>
      if entryKey = key then
        (key, agentCluster) :: rest
      else
        (entryKey, entryValue) :: setAgentClusterEntry rest key agentCluster

private def lookupHistoricalAgentClusterKey
    (entries : List (Origin × AgentClusterKey))
    (origin : Origin) :
    Option AgentClusterKey :=
  match entries with
  | [] => none
  | (entryOrigin, key) :: rest =>
      if entryOrigin = origin then some key else lookupHistoricalAgentClusterKey rest origin

private def setHistoricalAgentClusterKeyEntry
    (entries : List (Origin × AgentClusterKey))
    (origin : Origin)
    (key : AgentClusterKey) :
    List (Origin × AgentClusterKey) :=
  match entries with
  | [] => [(origin, key)]
  | (entryOrigin, entryKey) :: rest =>
      if entryOrigin = origin then
        (origin, key) :: rest
      else
        (entryOrigin, entryKey) :: setHistoricalAgentClusterKeyEntry rest origin key

private def nextBrowsingContextIdFromMembers (members : List BrowsingContext) : Nat :=
  members.foldl (fun nextId browsingContext => max nextId (browsingContext.id + 1)) 0

def nextBrowsingContextId (group : BrowsingContextGroup) : Nat :=
  nextBrowsingContextIdFromMembers group.browsingContextSet

def append
    (group : BrowsingContextGroup)
    (browsingContext : BrowsingContext) :
    BrowsingContextGroup × BrowsingContext :=
  let browsingContext := { browsingContext with groupId := some group.id }
  let browsingContextSet := group.browsingContextSet.concat browsingContext
  ({ group with browsingContextSet }, browsingContext)

def historicalAgentClusterKey
    (group : BrowsingContextGroup)
    (origin : Origin) :
    Option AgentClusterKey :=
  lookupHistoricalAgentClusterKey group.historicalAgentClusterKeyMap origin

def setHistoricalAgentClusterKey
    (group : BrowsingContextGroup)
    (origin : Origin)
    (key : AgentClusterKey) :
    BrowsingContextGroup :=
  {
    group with
      historicalAgentClusterKeyMap :=
        setHistoricalAgentClusterKeyEntry group.historicalAgentClusterKeyMap origin key
  }

def agentCluster
    (group : BrowsingContextGroup)
    (key : AgentClusterKey) :
    Option AgentCluster :=
  lookupAgentCluster group.agentClusterMap key

def setAgentCluster
    (group : BrowsingContextGroup)
    (key : AgentClusterKey)
    (agentCluster : AgentCluster) :
    BrowsingContextGroup :=
  { group with agentClusterMap := setAgentClusterEntry group.agentClusterMap key agentCluster }

end BrowsingContextGroup

namespace BrowsingContextGroupSet

private def nextIdFromMembers (members : List BrowsingContextGroup) : Nat :=
  members.foldl (fun nextId group => max nextId (group.id + 1)) 0

def nextId (groupSet : BrowsingContextGroupSet) : Nat :=
  nextIdFromMembers groupSet.members

def appendFresh
    (groupSet : BrowsingContextGroupSet) :
    BrowsingContextGroupSet × BrowsingContextGroup :=
  let group : BrowsingContextGroup := { id := groupSet.nextId }
  let members := groupSet.members.concat group
  ({ members }, group)

def replace
    (groupSet : BrowsingContextGroupSet)
    (updatedGroup : BrowsingContextGroup) :
    BrowsingContextGroupSet :=
  let members := groupSet.members.map fun group =>
    if group.id = updatedGroup.id then updatedGroup else group
  { members }

end BrowsingContextGroupSet

namespace TopLevelTraversableSet

private def lookupTraversableById
    (members : List TopLevelTraversable)
    (id : Nat) :
    Option TopLevelTraversable :=
  match members with
  | [] => none
  | traversable :: rest =>
      if traversable.id = id then some traversable else lookupTraversableById rest id

private def nextIdFromMembers (members : List TopLevelTraversable) : Nat :=
  members.foldl (fun nextId traversable => max nextId (traversable.id + 1)) 0

def nextId (topLevelTraversableSet : TopLevelTraversableSet) : Nat :=
  nextIdFromMembers topLevelTraversableSet.members

def appendFresh
    (topLevelTraversableSet : TopLevelTraversableSet) :
    TopLevelTraversableSet × TopLevelTraversable :=
  let traversable : TopLevelTraversable := {
    toTraversableNavigable := {}
    id := topLevelTraversableSet.nextId
    parentNavigableIdNone := rfl
  }
  let members := topLevelTraversableSet.members.concat traversable
  ({ members }, traversable)

def replace
    (topLevelTraversableSet : TopLevelTraversableSet)
    (updatedTraversable : TopLevelTraversable) :
    TopLevelTraversableSet :=
  let members := topLevelTraversableSet.members.map fun traversable =>
    if traversable.id = updatedTraversable.id then updatedTraversable else traversable
  { members }

def find?
    (topLevelTraversableSet : TopLevelTraversableSet)
    (id : Nat) :
    Option TopLevelTraversable :=
  lookupTraversableById topLevelTraversableSet.members id

end TopLevelTraversableSet

/-- https://html.spec.whatwg.org/multipage/#obtain-a-site -/
def obtainSite (origin : Origin) : String :=
  origin.site

/-- https://html.spec.whatwg.org/multipage/#determining-the-creation-sandboxing-flags -/
def determineCreationSandboxingFlags
    (_browsingContext : BrowsingContext)
    (_embedder : Option Unit) :
    SandboxingFlagSet :=
  -- TODO: Model the creation sandboxing flags algorithm.
  []

/-- https://html.spec.whatwg.org/multipage/#determining-the-origin -/
def determineOrigin
    (_url : String)
    (_sandboxFlags : SandboxingFlagSet)
    (creatorOrigin : Option Origin) :
    Origin :=
  -- TODO: Model the determining the origin algorithm.
  creatorOrigin.getD { serialization := "about:blank", site := "about:blank" }

/-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
def createPermissionsPolicy
    (_embedder : Option Unit)
    (_origin : Origin) :
    PermissionsPolicy :=
  -- TODO: Model creating a permissions policy.
  {}

/-- https://html.spec.whatwg.org/multipage/#internal-ancestor-origin-objects-list-creation-steps -/
def internalAncestorOriginObjectsListCreationSteps
    (_document : Document)
    (_iframeReferrerPolicy : Option String) :
    List Origin :=
  -- TODO: Model the internal ancestor origin objects list creation steps.
  []

/-- https://html.spec.whatwg.org/multipage/#ancestor-origins-list-creation-steps -/
def ancestorOriginsListCreationSteps
    (_document : Document) :
    List Origin :=
  -- TODO: Model the ancestor origins list creation steps.
  []

/-- https://html.spec.whatwg.org/multipage/#initialize-the-navigable -/
def initializeNavigable
    (navigable : Navigable)
    (document : Document)
    (documentState : DocumentState)
    (parentNavigableId : Option Nat := none) :
    Navigable :=
  -- Step 1 is the caller-side assertion that documentState.document is non-null.
  -- Step 2: Let entry be a new session history entry.
  let entry : SessionHistoryEntry := {
    url := document.url
    documentState
  }
  -- Steps 3-5: Set current/active session history entry and parent.
  {
    navigable with
      parentNavigableId
      currentSessionHistoryEntry := some entry
      activeSessionHistoryEntry := some entry
  }

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

/-- https://fetch.spec.whatwg.org/#fetch-scheme -/
def isFetchScheme (url : String) : Bool :=
  url.startsWith "http://" || url.startsWith "https://"

/-- https://html.spec.whatwg.org/multipage/#snapshotting-source-snapshot-params -/
def snapshotSourceSnapshotParams (sourceDocument : Document) : SourceSnapshotParams :=
  {
    sourcePolicyContainer := sourceDocument.policyContainer
  }

/-- https://html.spec.whatwg.org/multipage/#snapshotting-target-snapshot-params -/
def snapshotTargetSnapshotParams (_traversable : TopLevelTraversable) : TargetSnapshotParams :=
  {}

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
    -- TODO: Model populate with html/head/body for non-initial about:blank documents.
    (userAgent, document)
  else
    -- Step 2 otherwise: Create and feed the HTML parser.
    -- TODO: Model the parser-facing concurrent stream processing for HTML bytes.
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
  let _documentResource := entry.documentState.resource

  -- Step 2: Let request be a new request, with:
  let request : NavigationRequest := {
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
  match navigationParams with
  | none =>
      let documentResource := entry.documentState.resource
      if isFetchScheme entry.url && (documentResource.isNone || allowPOST) then
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
        -- TODO: Model srcdoc and non-fetch-scheme branches.
        userAgent
  | some navigationParams =>
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
      let traversable := {
        traversable with
          toTraversableNavigable := {
            traversable.toTraversableNavigable with
              toNavigable := {
                traversable.toTraversableNavigable.toNavigable with
                  currentSessionHistoryEntry := some historyEntry
                  activeSessionHistoryEntry := some historyEntry
              }
              sessionHistoryEntries :=
                traversable.toTraversableNavigable.sessionHistoryEntries.concat historyEntry
              currentSessionHistoryStep := historyEntry.step
              activeDocument := document
              ongoingNavigationId := none
          }
      }
      replaceTraversable userAgent traversable

/-- Resume a fetch-backed navigation after the concurrent fetch produces a response. -/
def processNavigationFetchResponse
    (userAgent : UserAgent)
    (navigationId : Nat)
    (response : NavigationResponse) :
    UserAgent :=
  let (userAgent, pendingNavigationFetch) := userAgent.takePendingNavigationFetch navigationId
  match pendingNavigationFetch with
  | none => userAgent
  | some pendingNavigationFetch =>
      match traversable? userAgent pendingNavigationFetch.traversableId with
      | none => userAgent
      | some traversable =>
          if traversable.toTraversableNavigable.ongoingNavigationId != some navigationId then
            userAgent
          else
            let navigationParams := createNavigationParamsFromResponse pendingNavigationFetch response
            attemptToPopulateHistoryEntryDocument
              userAgent
              pendingNavigationFetch.historyEntry
              traversable
              pendingNavigationFetch.navTimingType
              pendingNavigationFetch.sourceSnapshotParams
              pendingNavigationFetch.targetSnapshotParams
              pendingNavigationFetch.userInvolvement
              navigationId
              (some navigationParams)
              pendingNavigationFetch.cspNavigationType
              pendingNavigationFetch.allowPOST

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigate
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (_documentResource : Option Unit := none) :
    UserAgent :=
  match traversable.toTraversableNavigable.activeDocument with
  | none =>
      -- TODO: Model the browser-UI/sourceDocument-null branch of beginning navigation.
      userAgent
  | some sourceDocument =>
      let (userAgent, navigationId) := userAgent.allocateNavigationId
      let traversable := {
        traversable with
          toTraversableNavigable := {
            traversable.toTraversableNavigable with
              ongoingNavigationId := some navigationId
          }
      }
      let userAgent := replaceTraversable userAgent traversable
      let sourceSnapshotParams := snapshotSourceSnapshotParams sourceDocument
      let targetSnapshotParams := snapshotTargetSnapshotParams traversable
      let documentState : DocumentState := {
        initiatorOrigin := some sourceDocument.origin
        aboutBaseURL := sourceDocument.aboutBaseURL
        navigableTargetName := traversable.targetName
      }
      let historyEntry : SessionHistoryEntry := {
        url := destinationURL
        documentState
      }
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
  -- TODO: Model populating the initial DOM tree.

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
  Apply one user-agent transition.

  This sits above helper algorithms such as `navigate` and
  `processNavigationFetchResponse`, which implement the details of each labeled step.
  -/
  def step
    (userAgent : UserAgent)
    (action : UserAgentAction) :
    Option UserAgent :=
    match action with
    | .createTopLevelTraversable targetName =>
      let (userAgent, _traversable) := createNewTopLevelTraversable userAgent none targetName
      some userAgent
    | .beginNavigation traversableId destinationURL =>
      match traversable? userAgent traversableId with
      | none => none
      | some traversable => some (navigate userAgent traversable destinationURL)
    | .completeNavigation navigationId response =>
      some (processNavigationFetchResponse userAgent navigationId response)

end FormalWeb
