import FormalWeb.UserAgent
import FormalWeb.Proofs.EventLoopProof
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for the current user-agent navigation model. -/
inductive UserAgentAction
  | createTopLevelTraversable (targetName : String := "")
  | beginNavigation
      (traversableId : Nat)
      (destinationURL : String)
      (documentResource : Option DocumentResource := none)
  | completeNavigation (navigationId : Nat) (response : NavigationResponse)
  | requestDocumentFetch (handler : RustNetHandlerPointer) (request : NavigationRequest)
  | finishDocumentFetch (fetchId : Nat)
  | abortNavigation (traversableId : Nat)
  /-- Models the user agent applying a serialized input event to the active document of a traversable. -/
  | dispatchEvent (traversableId : Nat) (event : String)
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
  Models the event-loop task running to completion for the active document handle.
  Clears `hasPendingUpdateTheRendering` on the event loop. This requires the
  traversable's navigation to have completed.
  -/
  | updateTheRendering (traversableId : Nat) (eventLoopId : Nat) (documentId : DocumentId)
deriving Repr, DecidableEq

/--
Relational LTS for the user-agent navigation model.

This sits above helper algorithms such as `navigate` and
`processNavigationFetchResponse`, which implement the details of each labeled step.
-/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun userAgent => userAgent = default
  trans := fun userAgent action userAgent' =>
    match action with
    | .createTopLevelTraversable targetName =>
        userAgent' = (createNewTopLevelTraversable userAgent none targetName).1
    | .beginNavigation traversableId destinationURL documentResource =>
        ∃ traversable,
          traversable? userAgent traversableId = some traversable ∧
          userAgent' = navigate userAgent traversable destinationURL documentResource
    | .completeNavigation navigationId response =>
        userAgent' = processNavigationFetchResponse userAgent navigationId response
    | .requestDocumentFetch handler request =>
        userAgent' = (requestDocumentFetch userAgent handler request).1
    | .finishDocumentFetch fetchId =>
        ∃ pendingDocumentFetch,
          UserAgent.pendingDocumentFetch? userAgent fetchId = some pendingDocumentFetch ∧
          userAgent' = (userAgent.takePendingDocumentFetch fetchId).1
    | .abortNavigation traversableId =>
        userAgent' = abortNavigation userAgent traversableId
    | .dispatchEvent traversableId event =>
        ∃ eventLoopId message,
          queueDispatchedEvent userAgent traversableId event = some (userAgent', eventLoopId, message)
    | .navigationFinished traversableId =>
        ∃ traversable document,
          traversable? userAgent traversableId = some traversable ∧
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation = none ∧
          traversable.toTraversableNavigable.activeDocument = some document ∧
          userAgent' = userAgent
    | .queueUpdateTheRendering traversableId eventLoopId =>
        ∃ message,
          queueUpdateTheRendering userAgent traversableId = (userAgent', some (eventLoopId, message))
    | .updateTheRendering traversableId eventLoopId documentId =>
        completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some userAgent'

abbrev UserAgentTrace := TransitionSystem.TransitionTrace userAgentLTS

inductive UserAgentTaskMessageActionShape : UserAgentTaskMessage → List UserAgentAction → Prop where
  | freshTopLevelTraversable
      (destinationURL : String)
      (traversableId : Nat) :
      UserAgentTaskMessageActionShape
        (.freshTopLevelTraversable destinationURL)
        [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
  | documentFetchRequested
      (handler : RustNetHandlerPointer)
      (request : NavigationRequest) :
      UserAgentTaskMessageActionShape
        (.documentFetchRequested handler request)
        [.requestDocumentFetch handler request]
  | dispatchEventError
      (event : String) :
      UserAgentTaskMessageActionShape (.dispatchEvent event) []
  | dispatchEvent
      (traversableId : Nat)
      (event : String) :
      UserAgentTaskMessageActionShape (.dispatchEvent event) [.dispatchEvent traversableId event]
  | renderingOpportunityError :
      UserAgentTaskMessageActionShape .renderingOpportunity []
  | renderingOpportunity
      (traversableId eventLoopId : Nat) :
      UserAgentTaskMessageActionShape
        .renderingOpportunity
        [.queueUpdateTheRendering traversableId eventLoopId]
  | updateTheRenderingCompletedError
      (traversableId eventLoopId : Nat)
      (documentId : DocumentId) :
      UserAgentTaskMessageActionShape
        (.updateTheRenderingCompleted traversableId eventLoopId documentId)
        []
  | updateTheRenderingCompleted
      (traversableId eventLoopId : Nat)
      (documentId : DocumentId) :
      UserAgentTaskMessageActionShape
        (.updateTheRenderingCompleted traversableId eventLoopId documentId)
        [.updateTheRendering traversableId eventLoopId documentId]
  | fetchCompletedNavigation
      (fetchId : Nat)
      (pendingNavigationFetch : PendingNavigationFetch)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [.completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response)]
  | fetchCompletedNavigationWithPendingRendering
      (fetchId : Nat)
      (pendingNavigationFetch : PendingNavigationFetch)
      (response : FetchResponse)
      (eventLoopId : Nat) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [
          .completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response),
          .queueUpdateTheRendering pendingNavigationFetch.traversableId eventLoopId
        ]
  | fetchCompletedDocument
      (fetchId : Nat)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [.finishDocumentFetch fetchId]
  | fetchCompletedMissing
      (fetchId : Nat)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        []

  def interpretUserAgentEffect : UserAgentEffect → List UserAgentAction
    | UserAgentEffect.createTopLevelTraversable targetName _ _ =>
      [.createTopLevelTraversable targetName]
    | UserAgentEffect.beginNavigation traversableId destinationURL documentResource _ =>
      [.beginNavigation traversableId destinationURL documentResource]
    | UserAgentEffect.requestDocumentFetch handler request =>
      [.requestDocumentFetch handler request.request]
    | UserAgentEffect.dispatchEvent traversableId _ event _ =>
      [.dispatchEvent traversableId event]
    | UserAgentEffect.queueUpdateTheRendering traversableId eventLoopId _ =>
      [.queueUpdateTheRendering traversableId eventLoopId]
    | UserAgentEffect.updateTheRendering traversableId eventLoopId documentId _ =>
      [.updateTheRendering traversableId eventLoopId documentId]
    | UserAgentEffect.completeNavigation navigationId _ response _ =>
      [.completeNavigation navigationId response]
    | UserAgentEffect.finishDocumentFetch fetchId _ _ =>
      [.finishDocumentFetch fetchId]
    | UserAgentEffect.logError _ =>
      []

  def interpretUserAgentEffects (effects : Array UserAgentEffect) : List UserAgentAction :=
    effects.toList.flatMap interpretUserAgentEffect

theorem createTopLevelTraversable_trace
    (userAgent : UserAgent)
    (targetName : String := "") :
    UserAgentTrace
      userAgent
      [.createTopLevelTraversable targetName]
      (createNewTopLevelTraversable userAgent none targetName).1 := by
  refine TransitionSystem.TransitionTrace.single ?_
  simp [userAgentLTS]

theorem replaceTraversable_lookup_other
    (userAgent : UserAgent)
    (updated : TopLevelTraversable)
    (traversable : TopLevelTraversable)
    (hlookup : traversable? userAgent traversable.id = some traversable)
    (hid : updated.id ≠ traversable.id) :
    traversable? (replaceTraversable userAgent updated) traversable.id = some traversable := by
  unfold replaceTraversable traversable? TopLevelTraversableSet.find? at *
  change (userAgent.topLevelTraversableSet.members.insert updated.id updated).get? traversable.id = some traversable
  have hlookup' : userAgent.topLevelTraversableSet.members[traversable.id]? = some traversable := by
    simpa using hlookup
  simpa [hid, hlookup'] using
    (Std.TreeMap.getElem?_insert
      (t := userAgent.topLevelTraversableSet.members)
      (k := updated.id)
      (a := traversable.id)
      (v := updated))

theorem setActiveTraversable_lookup_active
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (hlookup : traversable? userAgent traversable.id = some traversable)
    (hactive : traversable.isActive = true) :
    traversable? (setActiveTraversable userAgent traversable.id) traversable.id = some traversable := by
  unfold setActiveTraversable
  cases hcurrent : activeTraversable? userAgent with
  | none =>
      simp [hlookup, hactive]
  | some activeTraversable =>
      by_cases hid : activeTraversable.id = traversable.id
      · simp [hlookup, hactive, hid]
      · have hpreserve :
          traversable?
              (replaceTraversable userAgent { activeTraversable with isActive := false })
              traversable.id = some traversable := by
            apply replaceTraversable_lookup_other
            · exact hlookup
            · simpa using hid
        simp [hid, hpreserve, hactive]

theorem createNewTopLevelTraversable_lookup
    (userAgent : UserAgent)
    (targetName : String := "") :
    let result := createNewTopLevelTraversable userAgent none targetName
    traversable? result.1 result.2.id = some result.2 := by
  let p := createNewTopLevelBrowsingContextAndDocument userAgent
  let baseUserAgent := p.1
  let browsingContext := p.2.1
  let document := p.2.2
  let baseTraversable := baseUserAgent.topLevelTraversableSet.appendFresh.snd
  let documentState : DocumentState := {
    document := some document
    initiatorOrigin := none
    origin := some document.origin
    navigableTargetName := targetName
    aboutBaseURL := document.aboutBaseURL
  }
  let initializedNavigable :=
    initializeNavigable baseTraversable.toTraversableNavigable.toNavigable document {
      documentState with everPopulated := true
    }
  let initialHistoryEntry : Option SessionHistoryEntry :=
    initializedNavigable.activeSessionHistoryEntry.map fun entry =>
      { entry with step := 0 }
  let createdTraversable : TopLevelTraversable := {
    baseTraversable with
      toTraversableNavigable := {
        baseTraversable.toTraversableNavigable with
          toNavigable := {
            initializedNavigable with
              currentSessionHistoryEntry := initialHistoryEntry
              activeSessionHistoryEntry := initialHistoryEntry
          }
          activeBrowsingContextId := some browsingContext.id
          activeDocument := some document
          sessionHistoryEntries := initialHistoryEntry.toList
      }
      isActive := true
      targetName
  }
  let preActiveUserAgent : UserAgent := {
    baseUserAgent with
      topLevelTraversableSet := baseUserAgent.topLevelTraversableSet.appendFresh.fst.replace createdTraversable
  }
  let finalUserAgent := setActiveTraversable preActiveUserAgent createdTraversable.id
  have hlookup0 : traversable? preActiveUserAgent createdTraversable.id = some createdTraversable := by
    simp [preActiveUserAgent, createdTraversable, traversable?, TopLevelTraversableSet.replace, TopLevelTraversableSet.find?]
  have hlookup1 : traversable? finalUserAgent createdTraversable.id = some createdTraversable := by
    exact setActiveTraversable_lookup_active preActiveUserAgent createdTraversable hlookup0 rfl
  simp [createNewTopLevelTraversable, createNewTopLevelTraversable.createNewTopLevelTraversableImpl,
    createNewTopLevelTraversable.createNewTopLevelBrowsingContextAndDocumentM,
    p, baseUserAgent, browsingContext, document, baseTraversable, documentState,
    initializedNavigable, initialHistoryEntry, createdTraversable, preActiveUserAgent, finalUserAgent, hlookup1]

theorem beginNavigation_after_createTopLevelTraversable_trace
    (userAgent : UserAgent)
    (destinationURL : String)
    (targetName : String := "") :
    let created := createNewTopLevelTraversable userAgent none targetName
    UserAgentTrace
      created.1
      [.beginNavigation created.2.id destinationURL none]
      (navigate created.1 created.2 destinationURL) := by
  intro created
  refine TransitionSystem.TransitionTrace.single ?_
  have hlookup : traversable? created.1 created.2.id = some created.2 := by
    simpa [created] using createNewTopLevelTraversable_lookup userAgent targetName
  exact ⟨created.2, hlookup, rfl⟩

theorem dispatchEvent_trace
    (userAgent : UserAgent)
    (traversableId : Nat)
    (event : String)
    (nextUserAgent : UserAgent)
    (eventLoopId : Nat)
    (message : EventLoopTaskMessage)
    (hqueue : queueDispatchedEvent userAgent traversableId event = some (nextUserAgent, eventLoopId, message)) :
    UserAgentTrace
      userAgent
      [.dispatchEvent traversableId event]
      nextUserAgent := by
  refine TransitionSystem.TransitionTrace.single ?_
  exact ⟨eventLoopId, message, hqueue⟩

theorem requestDocumentFetch_trace
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    UserAgentTrace
      userAgent
      [.requestDocumentFetch handler request]
      (requestDocumentFetch userAgent handler request).1 := by
  refine TransitionSystem.TransitionTrace.single ?_
  simp [userAgentLTS]

theorem finishDocumentFetch_trace
    (userAgent : UserAgent)
    (fetchId : Nat)
    (pendingDocumentFetch : PendingDocumentFetch)
    (hlookup : UserAgent.pendingDocumentFetch? userAgent fetchId = some pendingDocumentFetch) :
    UserAgentTrace
      userAgent
      [.finishDocumentFetch fetchId]
      (userAgent.takePendingDocumentFetch fetchId).1 := by
  refine TransitionSystem.TransitionTrace.single ?_
  exact ⟨pendingDocumentFetch, hlookup, rfl⟩

theorem queueUpdateTheRendering_step_trace
    (userAgent nextUserAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (message : EventLoopTaskMessage)
    (hqueue :
      queueUpdateTheRendering userAgent traversableId = (nextUserAgent, some (eventLoopId, message))) :
    UserAgentTrace
      userAgent
      [.queueUpdateTheRendering traversableId eventLoopId]
      nextUserAgent := by
  refine TransitionSystem.TransitionTrace.single ?_
  exact ⟨message, hqueue⟩

theorem updateTheRendering_step_trace
    (userAgent nextUserAgent : UserAgent)
    (traversableId eventLoopId : Nat)
  (documentId : DocumentId)
    (hcomplete :
      completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some nextUserAgent) :
    UserAgentTrace
      userAgent
      [.updateTheRendering traversableId eventLoopId documentId]
      nextUserAgent := by
  refine TransitionSystem.TransitionTrace.single ?_
  exact hcomplete

theorem startupSuccess_trace
    (userAgent nextUserAgent : UserAgent)
    (destinationURL : String)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    UserAgentTrace
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
          refine TransitionSystem.TransitionTrace.cons (intermediate := created.1) ?_ ?_
          · simp [created, userAgentLTS]
          · have hlookup : traversable? created.1 created.2.id = some created.2 := by
                simpa [created] using createNewTopLevelTraversable_lookup userAgent ""
            have hnext : actualNextUserAgent = navigate created.1 created.2 destinationURL := by
              simp [navigated, hnav, navigate]
            refine TransitionSystem.TransitionTrace.single ?_
            exact ⟨created.2, hlookup, hnext⟩

theorem runMonadic_updateTheRenderingCompleted_none_state
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId)
    (hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId = none) :
    (runMonadic userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId)).2 = userAgent := by
  simp [runMonadic, handleUserAgentTaskMessage, completeUpdateTheRenderingM, hcomplete]

theorem runMonadic_updateTheRenderingCompleted_none_actions
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId)
    (hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId = none) :
    interpretUserAgentEffects (runMonadic userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId)).1 = [] := by
  simp [runMonadic, handleUserAgentTaskMessage, completeUpdateTheRenderingM, hcomplete,
    interpretUserAgentEffects, interpretUserAgentEffect]

theorem runMonadic_updateTheRenderingCompleted_some_state
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId)
    (nextUserAgent : UserAgent)
    (hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some nextUserAgent) :
    (runMonadic userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId)).2 = nextUserAgent := by
  simp [runMonadic, handleUserAgentTaskMessage, completeUpdateTheRenderingM, hcomplete, queuePaintDocument]

theorem runMonadic_updateTheRenderingCompleted_some_actions
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId)
    (nextUserAgent : UserAgent)
    (hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some nextUserAgent) :
    interpretUserAgentEffects (runMonadic userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId)).1 =
      [.updateTheRendering traversableId eventLoopId documentId] := by
  simp [runMonadic, handleUserAgentTaskMessage, completeUpdateTheRenderingM, hcomplete,
    interpretUserAgentEffects, interpretUserAgentEffect, queuePaintDocument]

theorem runMonadic_fetchCompletedNavigation_state
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (pendingNavigationFetch : PendingNavigationFetch)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = some pendingNavigationFetch) :
    (runMonadic userAgent (.fetchCompleted fetchId response)).2 =
      processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response) := by
  simp [runMonadic, handleUserAgentTaskMessage, handleFetchCompletedM, hnavigation,
    processNavigationFetchResponse, navigationResponseOfFetchResponse]

theorem runMonadic_fetchCompletedNavigation_actions
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (pendingNavigationFetch : PendingNavigationFetch)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = some pendingNavigationFetch) :
    interpretUserAgentEffects (runMonadic userAgent (.fetchCompleted fetchId response)).1 =
      [.completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response)] := by
  simp [runMonadic, handleUserAgentTaskMessage, handleFetchCompletedM, hnavigation,
    interpretUserAgentEffects, interpretUserAgentEffect, navigationResponseOfFetchResponse]

theorem runMonadic_fetchCompletedDocument_state
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = none)
    (pendingDocumentFetch : PendingDocumentFetch)
    (hdocument : userAgent.pendingDocumentFetch? fetchId = some pendingDocumentFetch) :
    (runMonadic userAgent (.fetchCompleted fetchId response)).2 = (userAgent.takePendingDocumentFetch fetchId).1 := by
  simp [runMonadic, handleUserAgentTaskMessage, handleFetchCompletedM, hnavigation, hdocument,
    UserAgent.pendingDocumentFetch?, UserAgent.takePendingDocumentFetch]

theorem runMonadic_fetchCompletedDocument_actions
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = none)
    (pendingDocumentFetch : PendingDocumentFetch)
    (hdocument : userAgent.pendingDocumentFetch? fetchId = some pendingDocumentFetch) :
    interpretUserAgentEffects (runMonadic userAgent (.fetchCompleted fetchId response)).1 = [.finishDocumentFetch fetchId] := by
  change interpretUserAgentEffects ((handleFetchCompletedM fetchId response).run userAgent).1.2 =
    [.finishDocumentFetch fetchId]
  change interpretUserAgentEffects ((match userAgent.pendingNavigationFetchByFetchId? fetchId with
      | some pendingNavigationFetch => do
          set (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
          M.completeNavigation pendingNavigationFetch.navigationId pendingNavigationFetch.traversableId
            (navigationResponseOfFetchResponse response)
            (navigationDocumentDispatch?
              (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
              pendingNavigationFetch.traversableId
              (navigationResponseOfFetchResponse response))
      | none =>
          match userAgent.pendingDocumentFetch? fetchId with
          | some pendingDocumentFetch => do
              set (userAgent.takePendingDocumentFetch fetchId).1
              M.finishDocumentFetch fetchId pendingDocumentFetch
                (documentFetchCompletionDispatch? (userAgent.takePendingDocumentFetch fetchId).1 pendingDocumentFetch response)
          | none => pure ()).run userAgent).1.2 = [.finishDocumentFetch fetchId]
  rw [hnavigation, hdocument]
  change interpretUserAgentEffects
      (#[UserAgentEffect.finishDocumentFetch
          fetchId
          pendingDocumentFetch
          (documentFetchCompletionDispatch? (userAgent.takePendingDocumentFetch fetchId).1 pendingDocumentFetch response)] :
        Array UserAgentEffect) = [.finishDocumentFetch fetchId]
  simp [interpretUserAgentEffects, interpretUserAgentEffect]

theorem runMonadic_fetchCompletedMissing_state
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = none)
    (hdocument : userAgent.pendingDocumentFetch? fetchId = none) :
    (runMonadic userAgent (.fetchCompleted fetchId response)).2 = userAgent := by
  change ((handleFetchCompletedM fetchId response).run userAgent).2 = userAgent
  change ((match userAgent.pendingNavigationFetchByFetchId? fetchId with
      | some pendingNavigationFetch => do
          set (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
          M.completeNavigation pendingNavigationFetch.navigationId pendingNavigationFetch.traversableId
            (navigationResponseOfFetchResponse response)
            (navigationDocumentDispatch?
              (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
              pendingNavigationFetch.traversableId
              (navigationResponseOfFetchResponse response))
      | none =>
          match userAgent.pendingDocumentFetch? fetchId with
          | some pendingDocumentFetch => do
              set (userAgent.takePendingDocumentFetch fetchId).1
              M.finishDocumentFetch fetchId pendingDocumentFetch
                (documentFetchCompletionDispatch? (userAgent.takePendingDocumentFetch fetchId).1 pendingDocumentFetch response)
          | none => pure ()).run userAgent).2 = userAgent
  rw [hnavigation, hdocument]
  rfl

theorem runMonadic_fetchCompletedMissing_actions
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse)
    (hnavigation : userAgent.pendingNavigationFetchByFetchId? fetchId = none)
    (hdocument : userAgent.pendingDocumentFetch? fetchId = none) :
    interpretUserAgentEffects (runMonadic userAgent (.fetchCompleted fetchId response)).1 = [] := by
  change interpretUserAgentEffects ((handleFetchCompletedM fetchId response).run userAgent).1.2 = []
  change interpretUserAgentEffects ((match userAgent.pendingNavigationFetchByFetchId? fetchId with
      | some pendingNavigationFetch => do
          set (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
          M.completeNavigation pendingNavigationFetch.navigationId pendingNavigationFetch.traversableId
            (navigationResponseOfFetchResponse response)
            (navigationDocumentDispatch?
              (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
              pendingNavigationFetch.traversableId
              (navigationResponseOfFetchResponse response))
      | none =>
          match userAgent.pendingDocumentFetch? fetchId with
          | some pendingDocumentFetch => do
              set (userAgent.takePendingDocumentFetch fetchId).1
              M.finishDocumentFetch fetchId pendingDocumentFetch
                (documentFetchCompletionDispatch? (userAgent.takePendingDocumentFetch fetchId).1 pendingDocumentFetch response)
          | none => pure ()).run userAgent).1.2 = []
  rw [hnavigation, hdocument]
  change interpretUserAgentEffects (#[] : Array UserAgentEffect) = []
  simp [interpretUserAgentEffects]

theorem runMonadic_dispatchEvent_none_state
    (userAgent : UserAgent)
    (event : String)
    (hactiveId : activeTraversableId? userAgent = none) :
    (runMonadic userAgent (.dispatchEvent event)).2 = userAgent := by
  change ((dispatchEventM event).run userAgent).2 = userAgent
  change ((match activeTraversableId? userAgent with
      | none => M.logError (dispatchEventFailureDetails userAgent event)
      | some traversableId =>
          match queueDispatchedEvent userAgent traversableId event with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) => do
              set nextUserAgent
              M.dispatchEvent traversableId eventLoopId event eventLoopMessage
          | none => M.logError (dispatchEventFailureDetails userAgent event)).run userAgent).2 = userAgent
  rw [hactiveId]
  rfl

theorem runMonadic_dispatchEvent_none_actions
    (userAgent : UserAgent)
    (event : String)
    (hactiveId : activeTraversableId? userAgent = none) :
    interpretUserAgentEffects (runMonadic userAgent (.dispatchEvent event)).1 = [] := by
  change interpretUserAgentEffects ((dispatchEventM event).run userAgent).1.2 = []
  change interpretUserAgentEffects ((match activeTraversableId? userAgent with
      | none => M.logError (dispatchEventFailureDetails userAgent event)
      | some traversableId =>
          match queueDispatchedEvent userAgent traversableId event with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) => do
              set nextUserAgent
              M.dispatchEvent traversableId eventLoopId event eventLoopMessage
          | none => M.logError (dispatchEventFailureDetails userAgent event)).run userAgent).1.2 = []
  rw [hactiveId]
  change interpretUserAgentEffects
          (#[UserAgentEffect.logError (dispatchEventFailureDetails userAgent event)] : Array UserAgentEffect) = []
  simp [interpretUserAgentEffects, interpretUserAgentEffect]

theorem runMonadic_renderingOpportunity_none_state
    (userAgent : UserAgent)
    (hready : activeTraversableReady? userAgent = none) :
    (runMonadic userAgent .renderingOpportunity).2 = userAgent := by
  change (queueRenderingOpportunityM.run userAgent).2 = userAgent
  change ((match activeTraversableReady? userAgent with
      | none => M.logError (renderingOpportunityFailureDetails userAgent)
      | some traversableId =>
          match queueUpdateTheRendering userAgent traversableId with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) => do
              set nextUserAgent
              M.queueUpdateTheRendering traversableId eventLoopId eventLoopMessage
          | none => pure ()).run userAgent).2 = userAgent
  rw [hready]
  rfl

theorem runMonadic_renderingOpportunity_none_actions
    (userAgent : UserAgent)
    (hready : activeTraversableReady? userAgent = none) :
    interpretUserAgentEffects (runMonadic userAgent .renderingOpportunity).1 = [] := by
  change interpretUserAgentEffects (queueRenderingOpportunityM.run userAgent).1.2 = []
  change interpretUserAgentEffects ((match activeTraversableReady? userAgent with
      | none => M.logError (renderingOpportunityFailureDetails userAgent)
      | some traversableId =>
          match queueUpdateTheRendering userAgent traversableId with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) => do
              set nextUserAgent
              M.queueUpdateTheRendering traversableId eventLoopId eventLoopMessage
          | none => pure ()).run userAgent).1.2 = []
  rw [hready]
  change interpretUserAgentEffects
      (#[UserAgentEffect.logError (renderingOpportunityFailureDetails userAgent)] : Array UserAgentEffect) = []
  simp [interpretUserAgentEffects, interpretUserAgentEffect]

theorem runMonadic_freshTopLevelTraversable_actions
    (userAgent : UserAgent)
    (destinationURL : String) :
    let created := createNewTopLevelTraversable userAgent none ""
    let navigated := navigateWithPendingFetchRequest created.1 created.2 destinationURL
    interpretUserAgentEffects (runMonadic userAgent (.freshTopLevelTraversable destinationURL)).1 =
      [.createTopLevelTraversable "", .beginNavigation created.2.id destinationURL none] := by
  intro created navigated
  let createEffect : UserAgentEffect :=
    .createTopLevelTraversable "" (activeDocumentDispatch? created.1)
  let beginEffect : UserAgentEffect :=
    .beginNavigation created.2.id destinationURL none navigated.2
  cases hpending : navigated.2 with
  | none =>
      let errorMessage := startupNavigationFailureDetails destinationURL navigated.1 created.2.id
      change interpretUserAgentEffects #[createEffect, beginEffect, .logError errorMessage] =
        [.createTopLevelTraversable "", .beginNavigation created.2.id destinationURL none]
      simp [interpretUserAgentEffects, interpretUserAgentEffect, createEffect, beginEffect, errorMessage]
  | some pendingFetchRequest =>
      let _ := pendingFetchRequest
      change interpretUserAgentEffects #[createEffect, beginEffect] =
        [.createTopLevelTraversable "", .beginNavigation created.2.id destinationURL none]
      simp [interpretUserAgentEffects, interpretUserAgentEffect, createEffect, beginEffect]

theorem runMonadic_documentFetchRequested_actions
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    interpretUserAgentEffects (runMonadic userAgent (.documentFetchRequested handler request)).1 =
      [.requestDocumentFetch handler request] := by
  let result := requestDocumentFetch userAgent handler request
  change interpretUserAgentEffects #[.requestDocumentFetch handler result.2.2] =
    [.requestDocumentFetch handler request]
  simp [interpretUserAgentEffects, interpretUserAgentEffect, result, requestDocumentFetch]

theorem handleUserAgentTaskMessage_full_refinement
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      UserAgentTrace
        userAgent
        actions
        (runMonadic userAgent message).2 ∧
      interpretUserAgentEffects (runMonadic userAgent message).1 = actions := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      let created := createNewTopLevelTraversable userAgent none ""
      let navigated := navigateWithPendingFetchRequest created.1 created.2 destinationURL
      refine ⟨
        [.createTopLevelTraversable "", .beginNavigation created.2.id destinationURL none],
        .freshTopLevelTraversable destinationURL created.2.id,
        ?_,
        ?_
      ⟩
      · have htrace :
            UserAgentTrace
              userAgent
              [.createTopLevelTraversable "", .beginNavigation created.2.id destinationURL none]
              (navigate created.1 created.2 destinationURL) := by
            refine TransitionSystem.TransitionTrace.append ?_ ?_
            · simpa [created] using createTopLevelTraversable_trace userAgent ""
            · simpa [created, navigate] using
                beginNavigation_after_createTopLevelTraversable_trace userAgent destinationURL ""
        simpa [runMonadic, bootstrapFreshTopLevelTraversableM, createTopLevelTraversableM,
          navigateM, created, navigated, navigate] using htrace
      · simpa [created, navigated] using
          runMonadic_freshTopLevelTraversable_actions userAgent destinationURL
  | documentFetchRequested handler request =>
      refine ⟨
        [.requestDocumentFetch handler request],
        .documentFetchRequested handler request,
        ?_,
        ?_
      ⟩
      · simpa [runMonadic, handleUserAgentTaskMessage, requestDocumentFetchM, requestDocumentFetch] using
        requestDocumentFetch_trace userAgent handler request
        · exact runMonadic_documentFetchRequested_actions userAgent handler request
  | dispatchEvent event =>
      match hactiveId : activeTraversableId? userAgent with
      | none =>
          refine ⟨[], .dispatchEventError event, ?_, ?_⟩
          · simpa [runMonadic_dispatchEvent_none_state userAgent event hactiveId] using
            (TransitionSystem.TransitionTrace.nil userAgent)
          · exact runMonadic_dispatchEvent_none_actions userAgent event hactiveId
      | some traversableId =>
          match hqueue : queueDispatchedEvent userAgent traversableId event with
          | none =>
              refine ⟨[], .dispatchEventError event, ?_, ?_⟩
              · simpa [runMonadic, handleUserAgentTaskMessage, dispatchEventM, hactiveId, hqueue] using
                  (TransitionSystem.TransitionTrace.nil userAgent)
              · simp [runMonadic, handleUserAgentTaskMessage, dispatchEventM, hactiveId, hqueue,
                  interpretUserAgentEffects, interpretUserAgentEffect]
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              refine ⟨[.dispatchEvent traversableId event], .dispatchEvent traversableId event, ?_, ?_⟩
              · simpa [runMonadic, handleUserAgentTaskMessage, dispatchEventM, hactiveId, hqueue] using
                  dispatchEvent_trace userAgent traversableId event nextUserAgent eventLoopId eventLoopMessage hqueue
              · simp [runMonadic, handleUserAgentTaskMessage, dispatchEventM, hactiveId, hqueue,
                  interpretUserAgentEffects, interpretUserAgentEffect]
  | renderingOpportunity =>
      match hready : activeTraversableReady? userAgent with
      | none =>
          refine ⟨[], .renderingOpportunityError, ?_, ?_⟩
          · simpa [runMonadic_renderingOpportunity_none_state userAgent hready] using
            (TransitionSystem.TransitionTrace.nil userAgent)
          · exact runMonadic_renderingOpportunity_none_actions userAgent hready
      | some traversableId =>
          match hqueue : queueUpdateTheRendering userAgent traversableId with
          | none =>
              refine ⟨[], .renderingOpportunityError, ?_, ?_⟩
              · simpa [runMonadic, handleUserAgentTaskMessage, queueRenderingOpportunityM, hready, hqueue] using
                  (TransitionSystem.TransitionTrace.nil userAgent)
              · simp [runMonadic, handleUserAgentTaskMessage, queueRenderingOpportunityM, hready, hqueue,
                  interpretUserAgentEffects, interpretUserAgentEffect]
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              refine ⟨
                [.queueUpdateTheRendering traversableId eventLoopId],
                .renderingOpportunity traversableId eventLoopId,
                ?_,
                ?_
              ⟩
              · simpa [runMonadic, handleUserAgentTaskMessage, queueRenderingOpportunityM, hready, hqueue] using
                  queueUpdateTheRendering_step_trace userAgent nextUserAgent traversableId eventLoopId eventLoopMessage hqueue
              · simp [runMonadic, handleUserAgentTaskMessage, queueRenderingOpportunityM, hready, hqueue,
                  interpretUserAgentEffects, interpretUserAgentEffect]
  | updateTheRenderingCompleted traversableId eventLoopId documentId =>
      match hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
      | none =>
          have hstate :=
            runMonadic_updateTheRenderingCompleted_none_state
              userAgent traversableId eventLoopId documentId hcomplete
          have hactions :=
            runMonadic_updateTheRenderingCompleted_none_actions
              userAgent traversableId eventLoopId documentId hcomplete
          refine ⟨[], .updateTheRenderingCompletedError traversableId eventLoopId documentId, ?_, ?_⟩
          · simpa [hstate] using (TransitionSystem.TransitionTrace.nil userAgent)
          · exact hactions
      | some nextUserAgent =>
          have hstate :=
            runMonadic_updateTheRenderingCompleted_some_state
              userAgent traversableId eventLoopId documentId nextUserAgent hcomplete
          have hactions :=
            runMonadic_updateTheRenderingCompleted_some_actions
              userAgent traversableId eventLoopId documentId nextUserAgent hcomplete
          refine ⟨
            [.updateTheRendering traversableId eventLoopId documentId],
            .updateTheRenderingCompleted traversableId eventLoopId documentId,
            ?_,
            ?_
          ⟩
          · simpa [hstate] using
              updateTheRendering_step_trace userAgent nextUserAgent traversableId eventLoopId documentId hcomplete
          · exact hactions
  | fetchCompleted fetchId response =>
      match hnavigation : UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
      | some pendingNavigationFetch =>
          have hstate :=
            runMonadic_fetchCompletedNavigation_state
              userAgent fetchId response pendingNavigationFetch hnavigation
          have hactions :=
            runMonadic_fetchCompletedNavigation_actions
              userAgent fetchId response pendingNavigationFetch hnavigation
          refine ⟨
            [.completeNavigation
                pendingNavigationFetch.navigationId
                (navigationResponseOfFetchResponse response)],
            .fetchCompletedNavigation fetchId pendingNavigationFetch response,
            ?_,
            ?_
          ⟩
          · simpa [hstate, userAgentLTS, processNavigationFetchResponse, navigationResponseOfFetchResponse] using
              (TransitionSystem.TransitionTrace.single
                (show userAgentLTS.trans
                    userAgent
                    (.completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response))
                    (processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response)) by
                  simp [userAgentLTS, processNavigationFetchResponse]))
          · exact hactions
      | none =>
          match hdocument : UserAgent.pendingDocumentFetch? userAgent fetchId with
          | some pendingDocumentFetch =>
              have hdocument' : userAgent.pendingDocumentFetches[fetchId]? = some pendingDocumentFetch := by
                simpa [UserAgent.pendingDocumentFetch?] using hdocument
              have hstate :=
                runMonadic_fetchCompletedDocument_state
                  userAgent fetchId response hnavigation pendingDocumentFetch hdocument
              have hactions :=
                runMonadic_fetchCompletedDocument_actions
                  userAgent fetchId response hnavigation pendingDocumentFetch hdocument
              refine ⟨[.finishDocumentFetch fetchId], .fetchCompletedDocument fetchId response, ?_, ?_⟩
              · simpa [hstate, UserAgent.takePendingDocumentFetch, UserAgent.pendingDocumentFetch?, hdocument'] using
                  finishDocumentFetch_trace userAgent fetchId pendingDocumentFetch hdocument
              · exact hactions
          | none =>
              have hstate :=
                runMonadic_fetchCompletedMissing_state userAgent fetchId response hnavigation hdocument
              have hactions :=
                runMonadic_fetchCompletedMissing_actions userAgent fetchId response hnavigation hdocument
              refine ⟨[], .fetchCompletedMissing fetchId response, ?_, ?_⟩
              · simpa [hstate] using (TransitionSystem.TransitionTrace.nil userAgent)
              · exact hactions

theorem handleUserAgentTaskMessageTransition_refines
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      UserAgentTrace
        userAgent
        actions
        (runMonadic userAgent message).2 := by
  rcases handleUserAgentTaskMessage_full_refinement userAgent message with
    ⟨actions, hshape, htrace, _⟩
  exact ⟨actions, hshape, htrace⟩

theorem default_userAgent_empty
    (traversableId navigationId : Nat) :
    (activeTraversableId? (default : UserAgent)).isNone = true ∧
    (traversable? (default : UserAgent) traversableId).isNone = true ∧
    (UserAgent.pendingNavigationFetch? (default : UserAgent) navigationId).isNone = true := by
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

end FormalWeb
