import FormalWeb.Traversable

namespace FormalWeb

private def clearForwardSessionHistoryEntries
    (currentSessionHistoryStep : Nat)
    (entries : List SessionHistoryEntry) :
    List SessionHistoryEntry :=
  entries.filter fun historyEntry =>
    historyEntry.step <= currentSessionHistoryStep

private def replaceSessionHistoryEntryAtStep
    (entries : List SessionHistoryEntry)
    (step : Nat)
    (historyEntry : SessionHistoryEntry) :
    List SessionHistoryEntry :=
  entries.map fun entry => if entry.step = step then historyEntry else entry

private def finalizeCommittedNavigable
    (navigable : Navigable)
    (historyEntry : SessionHistoryEntry) :
    Navigable :=
  let committedNavigable := {
    navigable with
      currentSessionHistoryEntry := some historyEntry
      activeSessionHistoryEntry := some historyEntry
  }
  setOngoingNavigation committedNavigable none

private theorem finalizeCommittedNavigable_preserves_parentNavigableId
    (navigable : Navigable)
    (historyEntry : SessionHistoryEntry) :
    (finalizeCommittedNavigable navigable historyEntry).parentNavigableId =
      navigable.parentNavigableId := by
  unfold finalizeCommittedNavigable setOngoingNavigation
  by_cases hongoing : navigable.ongoingNavigation = none <;> simp [hongoing]

/-- https://html.spec.whatwg.org/multipage/#finalize-a-cross-document-navigation -/
def finalizeCrossDocumentNavigation
    (navigable : Navigable)
    (activeDocument : Option Document)
    (hasDeferredUpdateTheRendering : Bool)
    (currentSessionHistoryStep : Nat)
    (sessionHistoryEntries : List SessionHistoryEntry)
    (historyEntry : SessionHistoryEntry)
    (historyHandling : HistoryHandlingBehavior)
    (_userInvolvement : UserNavigationInvolvement := .none) :
    TraversableNavigable :=
  Id.run do
    let baseTraversable : TraversableNavigable := {
      toNavigable := navigable
      activeBrowsingContextId := activeDocument.map (·.browsingContextId)
      activeDocument
      hasDeferredUpdateTheRendering
      currentSessionHistoryStep
      sessionHistoryEntries
    }
    let some document := historyEntry.documentState.document
      | baseTraversable

    -- Step 2: Set navigable's is delaying `load` events to false.
    -- TODO: Model `load`-event delaying mode on navigables.

    -- Step 4: If all of the following are true:
    -- Note: The current model stores only top-level traversables, so the auxiliary-browsing-context branch is not yet represented.
    let historyEntry :=
      match activeDocument with
      | some currentDocument =>
          if navigable.parentNavigableId.isNone && document.origin != currentDocument.origin then
            {
              historyEntry with
                documentState := {
                  historyEntry.documentState with
                    navigableTargetName := ""
                }
            }
          else
            historyEntry
      | none =>
          historyEntry

    -- Step 5: Let entryToReplace be navigable's active session history entry if historyHandling is "replace", otherwise null.
    let entryToReplace :=
      match historyHandling with
      | .replace => navigable.activeSessionHistoryEntry
      | .push => none

    -- Steps 7-9: Compute targetStep and targetEntries.
    let (historyEntry, targetStep, targetEntries) :=
      match entryToReplace with
      | none =>
          let targetStep := currentSessionHistoryStep + 1
          let historyEntry := { historyEntry with step := targetStep }
          let targetEntries :=
            (clearForwardSessionHistoryEntries currentSessionHistoryStep sessionHistoryEntries).concat
              historyEntry
          (historyEntry, targetStep, targetEntries)
      | some entryToReplace =>
          let historyEntry := { historyEntry with step := entryToReplace.step }
          let targetEntries :=
            replaceSessionHistoryEntryAtStep
              sessionHistoryEntries
              entryToReplace.step
              historyEntry
          (historyEntry, currentSessionHistoryStep, targetEntries)

    -- Step 10: Apply the push/replace history step targetStep to traversable given historyHandling and userInvolvement.
    -- Note: The current model commits the history entry and active document directly on the traversable instead of separately modeling the shared traversal queue.
    let updatedNavigable := finalizeCommittedNavigable navigable historyEntry
    {
      toNavigable := updatedNavigable
      activeBrowsingContextId := some document.browsingContextId
      activeDocument := some document
      hasDeferredUpdateTheRendering
      currentSessionHistoryStep := targetStep
      sessionHistoryEntries := targetEntries
    }

theorem finalizeCrossDocumentNavigation_preserves_parentNavigableId
    (navigable : Navigable)
    (activeDocument : Option Document)
    (hasDeferredUpdateTheRendering : Bool)
    (currentSessionHistoryStep : Nat)
    (sessionHistoryEntries : List SessionHistoryEntry)
    (historyEntry : SessionHistoryEntry)
    (historyHandling : HistoryHandlingBehavior)
    (userInvolvement : UserNavigationInvolvement := .none) :
    (finalizeCrossDocumentNavigation
      navigable
      activeDocument
      hasDeferredUpdateTheRendering
      currentSessionHistoryStep
      sessionHistoryEntries
      historyEntry
      historyHandling
      userInvolvement).toNavigable.parentNavigableId =
      navigable.parentNavigableId := by
  cases hdocument : historyEntry.documentState.document with
  | none =>
      unfold finalizeCrossDocumentNavigation
      simp [hdocument]
      rfl
  | some document =>
      simpa [finalizeCrossDocumentNavigation, hdocument] using
        (finalizeCommittedNavigable_preserves_parentNavigableId navigable (historyEntry := _))

end FormalWeb
