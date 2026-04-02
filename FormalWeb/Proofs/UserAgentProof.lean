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
  | updateTheRendering (traversableId : Nat) (eventLoopId : Nat) (documentId : RustDocumentHandle)
deriving Repr, DecidableEq

/--
Relational LTS for the user-agent navigation model.

This sits above helper algorithms such as `navigate` and
`processNavigationFetchResponse`, which implement the details of each labeled step.
-/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun _ => True
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
      (documentId : RustDocumentHandle) :
      UserAgentTaskMessageActionShape
        (.updateTheRenderingCompleted traversableId eventLoopId documentId)
        []
  | updateTheRenderingCompleted
      (traversableId eventLoopId : Nat)
      (documentId : RustDocumentHandle) :
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
  simp [TopLevelTraversableSet.appendFresh, TopLevelTraversableSet.nextId, TopLevelTraversableSet.replace]

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
    (documentId : RustDocumentHandle)
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

theorem handleUserAgentTaskMessagePure_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      UserAgentTrace
        state.userAgent
        actions
        (handleUserAgentTaskMessagePure state message).state.userAgent := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | error _ =>
          refine ⟨[], .freshTopLevelTraversableError destinationURL, ?_⟩
          simp [handleUserAgentTaskMessagePure, hbootstrap, TransitionSystem.TransitionTrace.nil]
      | ok result =>
          refine ⟨
            [.createTopLevelTraversable "", .beginNavigation result.2.1 destinationURL none],
            .freshTopLevelTraversableSuccess destinationURL result.2.1,
            ?_
          ⟩
          simpa [handleUserAgentTaskMessagePure, hbootstrap] using
            startupSuccess_trace state.userAgent result.1 destinationURL result.2.1 result.2.2 hbootstrap
  | documentFetchRequested handler request =>
      refine ⟨
        [.requestDocumentFetch handler request],
        .documentFetchRequested handler request,
        ?_
      ⟩
      simpa [handleUserAgentTaskMessagePure, requestDocumentFetch] using
        requestDocumentFetch_trace state.userAgent handler request
  | dispatchEvent event =>
      match hstartup : state.startupTraversableId with
      | none =>
          refine ⟨[], .dispatchEventError event, ?_⟩
          simpa [handleUserAgentTaskMessagePure, hstartup] using
          (TransitionSystem.TransitionTrace.nil state.userAgent)
      | some traversableId =>
          match hlookup : traversable? state.userAgent traversableId with
          | none =>
              refine ⟨[], .dispatchEventError event, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hstartup, hlookup] using
            (TransitionSystem.TransitionTrace.nil state.userAgent)
          | some traversable =>
              match hactive : traversable.toTraversableNavigable.activeDocument with
              | none =>
                  refine ⟨[], .dispatchEventError event, ?_⟩
                  simpa [handleUserAgentTaskMessagePure, hstartup, hlookup, hactive] using
              (TransitionSystem.TransitionTrace.nil state.userAgent)
              | some _document =>
                  match hqueue : queueDispatchedEvent state.userAgent traversableId event with
                  | none =>
                      refine ⟨[], .dispatchEventError event, ?_⟩
                      simpa [handleUserAgentTaskMessagePure, hstartup, hlookup, hactive, hqueue] using
                (TransitionSystem.TransitionTrace.nil state.userAgent)
                  | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
                      refine ⟨[.dispatchEvent traversableId event], .dispatchEvent traversableId event, ?_⟩
                      simpa [handleUserAgentTaskMessagePure, hstartup, hlookup, hactive, hqueue] using
                        dispatchEvent_trace state.userAgent traversableId event nextUserAgent eventLoopId eventLoopMessage hqueue
  | renderingOpportunity =>
      match hstartup : state.startupTraversableId with
      | none =>
          refine ⟨[], .renderingOpportunityError, ?_⟩
          simpa [handleUserAgentTaskMessagePure, hstartup] using
            (TransitionSystem.TransitionTrace.nil state.userAgent)
      | some traversableId =>
          match hqueue : queueUpdateTheRendering state.userAgent traversableId with
          | none =>
              refine ⟨[], .renderingOpportunityError, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hstartup, hqueue] using
                (TransitionSystem.TransitionTrace.nil state.userAgent)
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              refine ⟨
                [.queueUpdateTheRendering traversableId eventLoopId],
                .renderingOpportunity traversableId eventLoopId,
                ?_
              ⟩
              simpa [handleUserAgentTaskMessagePure, hstartup, hqueue] using
                queueUpdateTheRendering_step_trace state.userAgent nextUserAgent traversableId eventLoopId eventLoopMessage hqueue
  | updateTheRenderingCompleted traversableId eventLoopId documentId =>
      match hcomplete : completeUpdateTheRendering state.userAgent traversableId eventLoopId documentId with
      | none =>
          refine ⟨[], .updateTheRenderingCompletedError traversableId eventLoopId documentId, ?_⟩
          simpa [handleUserAgentTaskMessagePure, hcomplete] using
            (TransitionSystem.TransitionTrace.nil state.userAgent)
      | some nextUserAgent =>
          refine ⟨
            [.updateTheRendering traversableId eventLoopId documentId],
            .updateTheRenderingCompleted traversableId eventLoopId documentId,
            ?_
          ⟩
          simpa [handleUserAgentTaskMessagePure, hcomplete] using
            updateTheRendering_step_trace state.userAgent nextUserAgent traversableId eventLoopId documentId hcomplete
  | fetchCompleted fetchId response =>
      match hnavigation : UserAgent.pendingNavigationFetchByFetchId? state.userAgent fetchId with
      | some pendingNavigationFetch =>
          refine ⟨
            [.completeNavigation
                pendingNavigationFetch.navigationId
                (navigationResponseOfFetchResponse response)],
            .fetchCompletedNavigation fetchId pendingNavigationFetch response,
            ?_
          ⟩
          refine TransitionSystem.TransitionTrace.single ?_
          simp [handleUserAgentTaskMessagePure, userAgentLTS, hnavigation, processNavigationFetchResponse,
            navigationResponseOfFetchResponse]
      | none =>
          match hdocument : UserAgent.pendingDocumentFetch? state.userAgent fetchId with
          | some pendingDocumentFetch =>
              have hdocument' : state.userAgent.pendingDocumentFetches[fetchId]? = some pendingDocumentFetch := by
                simpa [UserAgent.pendingDocumentFetch?] using hdocument
              have hstateField :
                  (handleUserAgentTaskMessagePure state (.fetchCompleted fetchId response)).state =
                    { state with userAgent := (state.userAgent.takePendingDocumentFetch fetchId).1 } := by
                cases hdispatch : documentFetchCompletionDispatch? state fetchId pendingDocumentFetch response <;>
                  simp [handleUserAgentTaskMessagePure, hnavigation, UserAgent.takePendingDocumentFetch,
                    UserAgent.pendingDocumentFetch?, hdocument', hdispatch]
              have hstate :
                  (handleUserAgentTaskMessagePure state (.fetchCompleted fetchId response)).state.userAgent =
                    (state.userAgent.takePendingDocumentFetch fetchId).1 := by
                simpa using congrArg UserAgentTaskState.userAgent hstateField
              refine ⟨[.finishDocumentFetch fetchId], .fetchCompletedDocument fetchId response, ?_⟩
              simpa [hstate] using
                finishDocumentFetch_trace state.userAgent fetchId pendingDocumentFetch hdocument
          | none =>
              refine ⟨[], .fetchCompletedMissing fetchId response, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hnavigation, hdocument] using
                (TransitionSystem.TransitionTrace.nil state.userAgent)

theorem userAgentTaskStep_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      UserAgentTrace
        state.userAgent
        actions
        (userAgentTaskStep state message).userAgent := by
  simpa [userAgentTaskStep] using handleUserAgentTaskMessagePure_refines state message

theorem userAgentTaskExec_refines
    (state : UserAgentTaskState)
    (messages : List UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTrace
        state.userAgent
        actions
        (userAgentTaskExec state messages).userAgent := by
  induction messages generalizing state with
  | nil =>
      refine ⟨[], ?_⟩
      simp [userAgentTaskExec, TransitionSystem.TransitionTrace.nil]
  | cons message messages ih =>
      have hstep := userAgentTaskStep_refines state message
      have htail := ih (userAgentTaskStep state message)
      rcases hstep with ⟨actions₁, _shape, htrace₁⟩
      rcases htail with ⟨actions₂, htrace₂⟩
      refine ⟨actions₁ ++ actions₂, ?_⟩
      simpa [userAgentTaskExec] using TransitionSystem.TransitionTrace.append htrace₁ htrace₂

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
    UserAgentTrace
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

end FormalWeb
