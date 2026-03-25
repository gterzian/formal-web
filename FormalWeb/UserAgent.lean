namespace FormalWeb

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
  /-- https://html.spec.whatwg.org/multipage/#browsing-context-group-set -/
  browsingContextGroupSet : BrowsingContextGroupSet := {}
  /-- https://html.spec.whatwg.org/multipage/#top-level-traversable-set -/
  topLevelTraversableSet : TopLevelTraversableSet := {}
deriving Repr

namespace BrowsingContextGroup

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

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-browsing-context -/
def createNewBrowsingContextAndDocument
    (_creator : Option Unit)
    (_embedder : Option Unit)
    (group : BrowsingContextGroup) :
    BrowsingContext × Option Unit :=
  -- Step 1: Let browsingContext be a new browsing context.
  let browsingContext : BrowsingContext := { id := group.nextBrowsingContextId }

  -- TODO: Model the remaining steps of creating a new browsing context and document.
  let document : Option Unit := none

  -- Step 20: Return browsingContext and document.
  (browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-browsing-context-group -/
def createNewBrowsingContextGroupAndDocument
    (userAgent : UserAgent) :
    UserAgent × BrowsingContextGroup × BrowsingContext × Option Unit :=
  -- Step 1: Let group be a new browsing context group.
  let (browsingContextGroupSet, group) := userAgent.browsingContextGroupSet.appendFresh

  -- Step 2: Append group to the user agent's browsing context group set.
  let userAgent := { userAgent with browsingContextGroupSet }

  -- Step 3: Let browsingContext and document be the result of creating a new browsing context and document with null, null, and group.
  let (browsingContext, document) := createNewBrowsingContextAndDocument none none group

  -- Step 4: Append browsingContext to group.
  let (group, browsingContext) := group.append browsingContext
  let browsingContextGroupSet := userAgent.browsingContextGroupSet.replace group
  let userAgent := { userAgent with browsingContextGroupSet }

  -- Step 5: Return group and document.
  (userAgent, group, browsingContext, document)

/-- https://html.spec.whatwg.org/multipage/#creating-a-new-top-level-browsing-context -/
def createNewTopLevelBrowsingContextAndDocument
    (userAgent : UserAgent) :
    UserAgent × BrowsingContext × Option Unit :=
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
  let document : Option Unit := none

  -- Step 2: If opener is null, then set document to the second return value of creating a new top-level browsing context and document.
  let (userAgent, document) := match opener with
    | none =>
        let (userAgent, _browsingContext, document) :=
          createNewTopLevelBrowsingContextAndDocument userAgent
        (userAgent, document)
    | some _ =>
        (userAgent, document)

  -- Step 3: Otherwise, set document to the second return value of creating a new auxiliary browsing context and document given opener.
  -- TODO: Model creating a new auxiliary browsing context and document given opener.

  -- Step 4: Let documentState be a new document state, with
  -- TODO: Model document state and its fields.
  let _documentState : Option Unit := document

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
