import Std.Data.TreeMap
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
deriving Repr, DecidableEq, Ord

/-- https://html.spec.whatwg.org/multipage/#task-source -/
inductive TaskSource
  | generic
deriving Repr, DecidableEq

/-- Model-local summary of the work stored in https://html.spec.whatwg.org/multipage/#concept-task-steps -/
inductive TaskStep
  | completeNav (navigationId : Nat)
  /-- Model-local UpdateTheRendering task step queued when rendering should be updated. -/
  | updateTheRendering
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

/-- Dequeue the UpdateTheRendering task and clear the pending flag. -/
def dequeueUpdateTheRenderingTask (eventLoop : EventLoop) : EventLoop :=
  { eventLoop with
      taskQueue := eventLoop.taskQueue.filter (fun t => t.step ≠ .updateTheRendering)
      hasPendingUpdateTheRendering := false }

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
  browsingContextSet : Std.TreeMap Nat BrowsingContext := Std.TreeMap.empty
  /-- https://html.spec.whatwg.org/multipage/#agent-cluster-map -/
  agentClusterMap : Std.TreeMap AgentClusterKey AgentCluster := Std.TreeMap.empty
  /-- https://html.spec.whatwg.org/multipage/#historical-agent-cluster-key-map -/
  historicalAgentClusterKeyMap : Std.TreeMap Origin AgentClusterKey := Std.TreeMap.empty
  /-- https://html.spec.whatwg.org/multipage/#bcg-cross-origin-isolation -/
  crossOriginIsolationMode : CrossOriginIsolationMode := .none
deriving Repr

/-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
structure BrowsingContextGroupSet where
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  members : Std.TreeMap Nat BrowsingContextGroup := Std.TreeMap.empty
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
  members : Std.TreeMap Nat TopLevelTraversable := Std.TreeMap.empty
deriving Repr

namespace BrowsingContextGroup

def nextBrowsingContextId (group : BrowsingContextGroup) : Nat :=
  group.browsingContextSet.size

def append
    (group : BrowsingContextGroup)
    (browsingContext : BrowsingContext) :
    BrowsingContextGroup × BrowsingContext :=
  let browsingContext := { browsingContext with groupId := some group.id }
  let browsingContextSet := group.browsingContextSet.insert browsingContext.id browsingContext
  ({ group with browsingContextSet }, browsingContext)

def historicalAgentClusterKey
    (group : BrowsingContextGroup)
    (origin : Origin) :
    Option AgentClusterKey :=
  group.historicalAgentClusterKeyMap.get? origin

def setHistoricalAgentClusterKey
    (group : BrowsingContextGroup)
    (origin : Origin)
    (key : AgentClusterKey) :
    BrowsingContextGroup :=
  {
    group with
      historicalAgentClusterKeyMap := group.historicalAgentClusterKeyMap.insert origin key
  }

def agentCluster
    (group : BrowsingContextGroup)
    (key : AgentClusterKey) :
    Option AgentCluster :=
  group.agentClusterMap.get? key

def setAgentCluster
    (group : BrowsingContextGroup)
    (key : AgentClusterKey)
    (agentCluster : AgentCluster) :
    BrowsingContextGroup :=
  { group with agentClusterMap := group.agentClusterMap.insert key agentCluster }

end BrowsingContextGroup

namespace BrowsingContextGroupSet

def nextId (groupSet : BrowsingContextGroupSet) : Nat :=
  groupSet.members.size

def appendFresh
    (groupSet : BrowsingContextGroupSet) :
    BrowsingContextGroupSet × BrowsingContextGroup :=
  let group : BrowsingContextGroup := { id := groupSet.nextId }
  let members := groupSet.members.insert group.id group
  ({ members }, group)

def replace
    (groupSet : BrowsingContextGroupSet)
    (updatedGroup : BrowsingContextGroup) :
    BrowsingContextGroupSet :=
  { groupSet with members := groupSet.members.insert updatedGroup.id updatedGroup }

end BrowsingContextGroupSet

namespace TopLevelTraversableSet

def nextId (topLevelTraversableSet : TopLevelTraversableSet) : Nat :=
  topLevelTraversableSet.members.size

def appendFresh
    (topLevelTraversableSet : TopLevelTraversableSet) :
    TopLevelTraversableSet × TopLevelTraversable :=
  let traversable : TopLevelTraversable := {
    toTraversableNavigable := {}
    id := topLevelTraversableSet.nextId
    parentNavigableIdNone := rfl
  }
  let members := topLevelTraversableSet.members.insert traversable.id traversable
  ({ members }, traversable)

def replace
    (topLevelTraversableSet : TopLevelTraversableSet)
    (updatedTraversable : TopLevelTraversable) :
    TopLevelTraversableSet :=
  { topLevelTraversableSet with members := topLevelTraversableSet.members.insert updatedTraversable.id updatedTraversable }

def find?
    (topLevelTraversableSet : TopLevelTraversableSet)
    (id : Nat) :
    Option TopLevelTraversable :=
  topLevelTraversableSet.members.get? id

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
