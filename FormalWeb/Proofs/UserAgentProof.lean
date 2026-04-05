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
          queueUpdateTheRendering userAgent traversableId = some (userAgent', eventLoopId, message)
    | .updateTheRendering traversableId eventLoopId documentId =>
        completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some userAgent'

abbrev UserAgentTrace := TransitionSystem.TransitionTrace userAgentLTS

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

theorem createTopLevelTraversable_trace
    (userAgent : UserAgent)
    (targetName : String := "") :
    UserAgentTrace
      userAgent
      [.createTopLevelTraversable targetName]
      (createNewTopLevelTraversable userAgent none targetName).1 := by
  refine TransitionSystem.TransitionTrace.single ?_
  simp [userAgentLTS]

theorem createNewTopLevelTraversable_lookup
    (userAgent : UserAgent)
    (targetName : String := "") :
    let result := createNewTopLevelTraversable userAgent none targetName
    traversable? result.1 result.2.id = some result.2 := by
  simp [createNewTopLevelTraversable, traversable?, TopLevelTraversableSet.find?]
  unfold createNewTopLevelTraversable.createNewTopLevelTraversableImpl
  simp [TopLevelTraversableSet.appendFresh, TopLevelTraversableSet.nextId, TopLevelTraversableSet.replace,
    setActiveTraversable]

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
      queueUpdateTheRendering userAgent traversableId = some (nextUserAgent, eventLoopId, message)) :
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

theorem handleUserAgentTaskMessageTransition_refines
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      UserAgentTrace
        userAgent
        actions
        (handleUserAgentTaskMessage userAgent message) := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL userAgent with
      | error _ =>
          refine ⟨[], .freshTopLevelTraversableError destinationURL, ?_⟩
          simp [handleUserAgentTaskMessage, hbootstrap, TransitionSystem.TransitionTrace.nil]
      | ok result =>
          refine ⟨
            [.createTopLevelTraversable "", .beginNavigation result.2.1 destinationURL none],
            .freshTopLevelTraversableSuccess destinationURL result.2.1,
            ?_
          ⟩
          simpa [handleUserAgentTaskMessage, hbootstrap] using
            startupSuccess_trace userAgent result.1 destinationURL result.2.1 result.2.2 hbootstrap
  | documentFetchRequested handler request =>
      refine ⟨
        [.requestDocumentFetch handler request],
        .documentFetchRequested handler request,
        ?_
      ⟩
      simpa [handleUserAgentTaskMessage, requestDocumentFetch] using
        requestDocumentFetch_trace userAgent handler request
  | dispatchEvent event =>
      match hactiveId : activeTraversableId? userAgent with
      | none =>
          refine ⟨[], .dispatchEventError event, ?_⟩
          simpa [handleUserAgentTaskMessage, hactiveId] using
          (TransitionSystem.TransitionTrace.nil userAgent)
      | some traversableId =>
          match hqueue : queueDispatchedEvent userAgent traversableId event with
          | none =>
              refine ⟨[], .dispatchEventError event, ?_⟩
              simpa [handleUserAgentTaskMessage, hactiveId, hqueue] using
                (TransitionSystem.TransitionTrace.nil userAgent)
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              refine ⟨[.dispatchEvent traversableId event], .dispatchEvent traversableId event, ?_⟩
              simpa [handleUserAgentTaskMessage, hactiveId, hqueue] using
                dispatchEvent_trace userAgent traversableId event nextUserAgent eventLoopId eventLoopMessage hqueue
  | renderingOpportunity =>
      match hready : activeTraversableReady? userAgent with
      | none =>
          refine ⟨[], .renderingOpportunityError, ?_⟩
          simpa [handleUserAgentTaskMessage, hready] using
            (TransitionSystem.TransitionTrace.nil userAgent)
      | some traversableId =>
          match hqueue : queueUpdateTheRendering userAgent traversableId with
          | none =>
              refine ⟨[], .renderingOpportunityError, ?_⟩
              simpa [handleUserAgentTaskMessage, hready, hqueue] using
                (TransitionSystem.TransitionTrace.nil userAgent)
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              refine ⟨
                [.queueUpdateTheRendering traversableId eventLoopId],
                .renderingOpportunity traversableId eventLoopId,
                ?_
              ⟩
              simpa [handleUserAgentTaskMessage, hready, hqueue] using
                queueUpdateTheRendering_step_trace userAgent nextUserAgent traversableId eventLoopId eventLoopMessage hqueue
  | updateTheRenderingCompleted traversableId eventLoopId documentId =>
      match hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
      | none =>
          refine ⟨[], .updateTheRenderingCompletedError traversableId eventLoopId documentId, ?_⟩
          simpa [handleUserAgentTaskMessage, hcomplete] using
            (TransitionSystem.TransitionTrace.nil userAgent)
      | some nextUserAgent =>
          refine ⟨
            [.updateTheRendering traversableId eventLoopId documentId],
            .updateTheRenderingCompleted traversableId eventLoopId documentId,
            ?_
          ⟩
          simpa [handleUserAgentTaskMessage, hcomplete] using
            updateTheRendering_step_trace userAgent nextUserAgent traversableId eventLoopId documentId hcomplete
  | fetchCompleted fetchId response =>
      match hnavigation : UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
      | some pendingNavigationFetch =>
          refine ⟨
            [.completeNavigation
                pendingNavigationFetch.navigationId
                (navigationResponseOfFetchResponse response)],
            .fetchCompletedNavigation fetchId pendingNavigationFetch response,
            ?_
          ⟩
          refine TransitionSystem.TransitionTrace.single ?_
          simp [handleUserAgentTaskMessage, userAgentLTS, hnavigation, processNavigationFetchResponse,
            navigationResponseOfFetchResponse]
      | none =>
          match hdocument : UserAgent.pendingDocumentFetch? userAgent fetchId with
          | some pendingDocumentFetch =>
              have hdocument' : userAgent.pendingDocumentFetches[fetchId]? = some pendingDocumentFetch := by
                simpa [UserAgent.pendingDocumentFetch?] using hdocument
              refine ⟨[.finishDocumentFetch fetchId], .fetchCompletedDocument fetchId response, ?_⟩
              simpa [handleUserAgentTaskMessage, hnavigation, UserAgent.takePendingDocumentFetch,
                UserAgent.pendingDocumentFetch?, hdocument'] using
                finishDocumentFetch_trace userAgent fetchId pendingDocumentFetch hdocument
          | none =>
              refine ⟨[], .fetchCompletedMissing fetchId response, ?_⟩
              simpa [handleUserAgentTaskMessage, hnavigation, hdocument] using
                (TransitionSystem.TransitionTrace.nil userAgent)

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

theorem handleUserAgentTaskMessage_startup_from_default_success
    (destinationURL : String)
    (nextUserAgent : UserAgent)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL (default : UserAgent) =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    handleUserAgentTaskMessage (default : UserAgent) (.freshTopLevelTraversable destinationURL) = nextUserAgent ∧
    UserAgentTrace
      (default : UserAgent)
      [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
      (handleUserAgentTaskMessage (default : UserAgent) (.freshTopLevelTraversable destinationURL)) := by
  let _ := pendingFetchRequest
  refine ⟨?_, ?_⟩
  · simp [handleUserAgentTaskMessage, hbootstrap]
  · simpa [handleUserAgentTaskMessage, hbootstrap] using
      startupSuccess_trace
        (default : UserAgent)
        nextUserAgent
        destinationURL
        traversableId
        pendingFetchRequest
        hbootstrap

end FormalWeb
