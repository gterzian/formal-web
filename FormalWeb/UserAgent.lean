namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/#concept-origin -/
structure Origin where
  serialization : String
  site : String
deriving Repr, DecidableEq

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

/-- https://html.spec.whatwg.org/multipage/#similar-origin-window-agent -/
structure SimilarOriginWindowAgent where
  id : Nat
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#agent-cluster-cross-origin-isolation -/
structure AgentCluster where
  id : Nat
  crossOriginIsolationMode : CrossOriginIsolationMode := .none
  /-- https://html.spec.whatwg.org/multipage/#is-origin-keyed -/
  isOriginKeyed : Bool := false
  similarOriginWindowAgent : SimilarOriginWindowAgent
deriving Repr, DecidableEq

/-- Placeholder for the Rust-side DOM object backing a spec-level document. -/
structure RustDocumentHandle where
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
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#top-level-browsing-context -/
structure BrowsingContext where
  id : Nat
  /-- https://html.spec.whatwg.org/multipage/#tlbc-group -/
  groupId : Option Nat := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#browsing-context-group -/
structure BrowsingContextGroup where
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
  members : List BrowsingContextGroup := []
deriving Repr

/-- https://html.spec.whatwg.org/multipage/#top-level-traversable -/
structure TopLevelTraversable where
  id : Nat
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
structure TopLevelTraversableSet where
  members : List TopLevelTraversable := []
deriving Repr

/--
The user agent is the top-level global state for the browser model.
-/
structure UserAgent where
  nextRustDocumentHandleId : Nat := 0
  nextAgentClusterId : Nat := 0
  nextSimilarOriginWindowAgentId : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  browsingContextGroupSet : BrowsingContextGroupSet := {}
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  topLevelTraversableSet : TopLevelTraversableSet := {}
deriving Repr

namespace UserAgent

def allocateRustDocumentHandle (userAgent : UserAgent) : UserAgent × RustDocumentHandle :=
  let handle : RustDocumentHandle := { id := userAgent.nextRustDocumentHandleId }
  let userAgent := { userAgent with nextRustDocumentHandleId := userAgent.nextRustDocumentHandleId + 1 }
  (userAgent, handle)

def allocateAgentClusterId (userAgent : UserAgent) : UserAgent × Nat :=
  let agentClusterId := userAgent.nextAgentClusterId
  let userAgent := { userAgent with nextAgentClusterId := userAgent.nextAgentClusterId + 1 }
  (userAgent, agentClusterId)

def allocateSimilarOriginWindowAgentId (userAgent : UserAgent) : UserAgent × Nat :=
  let windowAgentId := userAgent.nextSimilarOriginWindowAgentId
  let userAgent := {
    userAgent with
      nextSimilarOriginWindowAgentId := userAgent.nextSimilarOriginWindowAgentId + 1
  }
  (userAgent, windowAgentId)

end UserAgent

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

private def nextIdFromMembers (members : List TopLevelTraversable) : Nat :=
  members.foldl (fun nextId traversable => max nextId (traversable.id + 1)) 0

def nextId (topLevelTraversableSet : TopLevelTraversableSet) : Nat :=
  nextIdFromMembers topLevelTraversableSet.members

def appendFresh
    (topLevelTraversableSet : TopLevelTraversableSet) :
    TopLevelTraversableSet × TopLevelTraversable :=
  let traversable : TopLevelTraversable := { id := topLevelTraversableSet.nextId }
  let members := topLevelTraversableSet.members.concat traversable
  ({ members }, traversable)

end TopLevelTraversableSet

/-- https://html.spec.whatwg.org/multipage/#navigate -/
def navigate
    (userAgent : UserAgent)
    (_traversable : TopLevelTraversable)
    (_destinationURL : String)
    (_documentResource : Option Unit := none) :
    UserAgent :=
  -- TODO: Implement the navigate algorithm.
  userAgent

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

/-- https://html.spec.whatwg.org/multipage/#obtain-similar-origin-window-agent -/
def obtainSimilarOriginWindowAgent
    (userAgent : UserAgent)
    (origin : Origin)
    (group : BrowsingContextGroup)
    (requestsOAC : Bool) :
    UserAgent × BrowsingContextGroup × SimilarOriginWindowAgent :=
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
        -- Step 6.4: Add the result of creating an agent, given false, to agentCluster.
        let (userAgent, windowAgentId) := userAgent.allocateSimilarOriginWindowAgentId
        let similarOriginWindowAgent : SimilarOriginWindowAgent := { id := windowAgentId }
        let agentCluster : AgentCluster := {
          id := agentClusterId
          similarOriginWindowAgent
          crossOriginIsolationMode := group.crossOriginIsolationMode
          isOriginKeyed := match key with
            | .origin _ => true
            | .site _ => false
        }
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
    (_targetName : String)
    (_openerNavigableForWebDriver : Option Unit := none) :
    UserAgent × TopLevelTraversable :=
  -- Step 1: Let document be null.
  let document : Option Document := none

  -- Step 2: If opener is null, then set document to the second return value of creating a new top-level browsing context and document.
  let (userAgent, document) := match opener with
    | none =>
        let (userAgent, _browsingContext, document) :=
          createNewTopLevelBrowsingContextAndDocument userAgent
        (userAgent, some document)
    | some _ =>
        (userAgent, document)

  -- Step 3: Otherwise, set document to the second return value of creating a new auxiliary browsing context and document given opener.
  -- TODO: Model creating a new auxiliary browsing context and document given opener.

  -- Step 4: Let documentState be a new document state, with
  -- TODO: Model document state and its fields.
  let _documentState : Option Document := document

  -- Step 5: Let traversable be a new traversable navigable.
  let (topLevelTraversableSet, traversable) := userAgent.topLevelTraversableSet.appendFresh

  -- Step 6: Initialize the navigable traversable given documentState.
  -- TODO: Model initialize the navigable.

  -- Step 7: Let initialHistoryEntry be traversable's active session history entry.
  -- TODO: Model the active session history entry.
  let _initialHistoryEntry : Option Unit := none

  -- Step 8: Set initialHistoryEntry's step to 0.
  -- TODO: Model the session history entry step update.

  -- Step 9: Append initialHistoryEntry to traversable's session history entries.
  -- TODO: Model traversable session history entries.

  -- Step 10: If opener is non-null, then legacy-clone a traversable storage shed given opener's top-level traversable and traversable.
  -- TODO: Model legacy-clone a traversable storage shed.

  -- Step 11: Append traversable to the user agent's top-level traversable set.
  let userAgent := { userAgent with topLevelTraversableSet }

  -- Step 12: Invoke WebDriver BiDi navigable created with traversable and openerNavigableForWebDriver.
  -- TODO: Model the WebDriver BiDi hook.

  -- Step 13: Return traversable.
  (userAgent, traversable)

/-- https://html.spec.whatwg.org/multipage/#create-a-fresh-top-level-traversable -/
def createFreshTopLevelTraversable
    (userAgent : UserAgent)
    (initialNavigationURL : String)
    (initialNavigationPostResource : Option Unit := none) :
    UserAgent × TopLevelTraversable :=
  -- Step 1: Let traversable be the result of creating a new top-level traversable given null and the empty string.
  let (userAgent, traversable) := createNewTopLevelTraversable userAgent none ""

  -- Step 2: Navigate traversable to initialNavigationURL using traversable's active document, with documentResource set to initialNavigationPostResource.
  -- TODO: Model traversable's active document.
  let userAgent := navigate userAgent traversable initialNavigationURL initialNavigationPostResource

  -- Step 3: Return traversable.
  (userAgent, traversable)

end FormalWeb
