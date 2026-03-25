import FormalWeb.Navigation

namespace FormalWeb

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

/-- Model-local summary of the work stored in https://html.spec.whatwg.org/multipage/#concept-task-steps -/
inductive TaskStep
  | completeNav (navigationId : Nat)
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

end EventLoop

/-- https://tc39.es/ecma262/#sec-agents -/
structure Agent where
  /-- Model-local identifier standing in for the signifier allocated by https://html.spec.whatwg.org/multipage/#create-an-agent -/
  id : Nat
  /-- https://tc39.es/ecma262/#sec-agents -/
  canBlock : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#concept-agent-event-loop -/
  eventLoop : EventLoop
deriving Repr, DecidableEq

/-- Model-local opaque identifier for a https://html.spec.whatwg.org/multipage/#global-object used by task-queueing helpers. -/
abbrev GlobalObjectId := Nat

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

/-- https://html.spec.whatwg.org/multipage/#obtain-a-site -/
def obtainSite (origin : Origin) : String :=
  origin.site

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
  /-- https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
  ongoingNavigation : Option OngoingNavigation := none
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

/-- https://html.spec.whatwg.org/multipage/#set-the-ongoing-navigation -/
def setOngoingNavigation
    (navigable : Navigable)
    (newValue : Option OngoingNavigation) :
    Navigable :=
  -- Step 1: If navigable's ongoing navigation is equal to newValue, then return.
  if navigable.ongoingNavigation = newValue then
    navigable
  else
    -- Step 2: Inform the navigation API about aborting navigation given navigable.
    -- TODO: Model the navigation-API-facing abort bookkeeping for ongoing navigations.
    -- Step 3: Set navigable's ongoing navigation to newValue.
    {
      navigable with
        ongoingNavigation := newValue
    }

theorem setOngoingNavigation_preserves_parentNavigableId
    (navigable : Navigable)
    (newValue : Option OngoingNavigation) :
    (setOngoingNavigation navigable newValue).parentNavigableId = navigable.parentNavigableId := by
  unfold setOngoingNavigation
  by_cases h : navigable.ongoingNavigation = newValue
  · simp [h]
  · simp [h]

end FormalWeb