import FormalWeb.UserAgent
import FormalWeb.TransitionTrace

namespace FormalWeb

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
  | .requestDocumentFetch handler request =>
      pure (requestDocumentFetch userAgent handler request).1
  | .finishDocumentFetch fetchId =>
      let (userAgent, pendingDocumentFetch?) := userAgent.takePendingDocumentFetch fetchId
      let _pendingDocumentFetch <- pendingDocumentFetch?
      pure userAgent
  | .abortNavigation traversableId =>
      pure (abortNavigation userAgent traversableId)
  | .dispatchEvent traversableId _event =>
      let traversable <- traversable? userAgent traversableId
      if traversable.toTraversableNavigable.activeDocument.isNone then
        none
      else
        pure userAgent
  | .navigationFinished traversableId =>
      let traversable <- traversable? userAgent traversableId
      if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
        none
      else if traversable.toTraversableNavigable.activeDocument.isNone then
        none
      else
        pure userAgent
  | .queueUpdateTheRendering traversableId eventLoopId =>
      let _traversable <- traversable? userAgent traversableId
      let eventLoop <- userAgent.eventLoop? eventLoopId
      let eventLoop := eventLoop.enqueueUpdateTheRenderingTask
      pure (userAgent.setEventLoop eventLoop)
  | .updateTheRendering traversableId eventLoopId baseDocPointer =>
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
  | renderingOpportunity :
      UserAgentTaskMessageActionShape .renderingOpportunity []
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
    TransitionTrace
      step
      userAgent
      [.createTopLevelTraversable targetName]
      (createNewTopLevelTraversable userAgent none targetName).1 := by
  refine TransitionTrace.single ?_
  simp [step, createNewTopLevelTraversable]

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
    TransitionTrace
      step
      created.1
      [.beginNavigation created.2.id destinationURL none]
      (navigate created.1 created.2 destinationURL) := by
  intro created
  refine TransitionTrace.single ?_
  have hlookup : traversable? created.1 created.2.id = some created.2 := by
    simpa [created] using createNewTopLevelTraversable_lookup userAgent targetName
  simp [step, navigate, hlookup]

theorem dispatchEvent_trace
    (userAgent : UserAgent)
    (traversableId : Nat)
    (event : String)
    (traversable : TopLevelTraversable)
    (hlookup : traversable? userAgent traversableId = some traversable)
    (document : Document)
    (hactive : traversable.toTraversableNavigable.activeDocument = some document) :
    TransitionTrace
      step
      userAgent
      [.dispatchEvent traversableId event]
      userAgent := by
  refine TransitionTrace.single ?_
  simp [step, hlookup, hactive]

theorem requestDocumentFetch_trace
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    TransitionTrace
      step
      userAgent
      [.requestDocumentFetch handler request]
      (requestDocumentFetch userAgent handler request).1 := by
  refine TransitionTrace.single ?_
  simp [step]

theorem finishDocumentFetch_trace
    (userAgent : UserAgent)
    (fetchId : Nat)
    (pendingDocumentFetch : PendingDocumentFetch)
    (hlookup : UserAgent.pendingDocumentFetch? userAgent fetchId = some pendingDocumentFetch) :
    TransitionTrace
      step
      userAgent
      [.finishDocumentFetch fetchId]
      (userAgent.takePendingDocumentFetch fetchId).1 := by
  refine TransitionTrace.single ?_
  have hlookup' : userAgent.pendingDocumentFetches[fetchId]? = some pendingDocumentFetch := by
    simpa [UserAgent.pendingDocumentFetch?] using hlookup
  simp [step, UserAgent.takePendingDocumentFetch, UserAgent.pendingDocumentFetch?, hlookup']

theorem startupSuccess_trace
    (userAgent nextUserAgent : UserAgent)
    (destinationURL : String)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    TransitionTrace
      step
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
          refine TransitionTrace.cons (intermediate := created.1) ?_ ?_
          · simpa [created] using
              (show step userAgent (.createTopLevelTraversable "") = some (createNewTopLevelTraversable userAgent none "").1 by
                simp [step, createNewTopLevelTraversable])
          · simpa [created, navigate, navigated, hnav] using
              beginNavigation_after_createTopLevelTraversable_trace userAgent destinationURL ""

theorem handleUserAgentTaskMessagePure_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      TransitionTrace
        step
        state.userAgent
        actions
        (handleUserAgentTaskMessagePure state message).state.userAgent := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | error _ =>
          refine ⟨[], .freshTopLevelTraversableError destinationURL, ?_⟩
          simp [handleUserAgentTaskMessagePure, hbootstrap, TransitionTrace.nil]
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
            (TransitionTrace.nil state.userAgent)
      | some traversableId =>
          match hlookup : traversable? state.userAgent traversableId with
          | none =>
              refine ⟨[], .dispatchEventError event, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hstartup, hlookup] using
                (TransitionTrace.nil state.userAgent)
          | some traversable =>
              match hactive : traversable.toTraversableNavigable.activeDocument with
              | none =>
                  refine ⟨[], .dispatchEventError event, ?_⟩
                  simpa [handleUserAgentTaskMessagePure, hstartup, hlookup, hactive] using
                    (TransitionTrace.nil state.userAgent)
              | some document =>
                  refine ⟨[.dispatchEvent traversableId event], .dispatchEvent traversableId event, ?_⟩
                  simpa [handleUserAgentTaskMessagePure, hstartup, hlookup, hactive] using
                    dispatchEvent_trace state.userAgent traversableId event traversable hlookup document hactive
  | renderingOpportunity =>
      refine ⟨[], .renderingOpportunity, ?_⟩
      simp [handleUserAgentTaskMessagePure, TransitionTrace.nil]
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
          refine TransitionTrace.single ?_
          simp [handleUserAgentTaskMessagePure, step, hnavigation, processNavigationFetchResponse,
            navigationResponseOfFetchResponse]
      | none =>
          match hdocument : UserAgent.pendingDocumentFetch? state.userAgent fetchId with
          | some pendingDocumentFetch =>
              have hdocument' : state.userAgent.pendingDocumentFetches[fetchId]? = some pendingDocumentFetch := by
                simpa [UserAgent.pendingDocumentFetch?] using hdocument
              refine ⟨[.finishDocumentFetch fetchId], .fetchCompletedDocument fetchId response, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hnavigation, UserAgent.takePendingDocumentFetch,
                UserAgent.pendingDocumentFetch?, hdocument'] using
                finishDocumentFetch_trace state.userAgent fetchId pendingDocumentFetch hdocument
          | none =>
              refine ⟨[], .fetchCompletedMissing fetchId response, ?_⟩
              simpa [handleUserAgentTaskMessagePure, hnavigation, hdocument] using
                (TransitionTrace.nil state.userAgent)

theorem userAgentTaskStep_refines
    (state : UserAgentTaskState)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTaskMessageActionShape message actions ∧
      TransitionTrace
        step
        state.userAgent
        actions
        (userAgentTaskStep state message).userAgent := by
  simpa [userAgentTaskStep] using handleUserAgentTaskMessagePure_refines state message

theorem userAgentTaskExec_refines
    (state : UserAgentTaskState)
    (messages : List UserAgentTaskMessage) :
    ∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
        (userAgentTaskExec state messages).userAgent := by
  induction messages generalizing state with
  | nil =>
      refine ⟨[], ?_⟩
      simp [userAgentTaskExec, TransitionTrace.nil]
  | cons message messages ih =>
      have hstep := userAgentTaskStep_refines state message
      have htail := ih (userAgentTaskStep state message)
      rcases hstep with ⟨actions₁, _shape, htrace₁⟩
      rcases htail with ⟨actions₂, htrace₂⟩
      refine ⟨actions₁ ++ actions₂, ?_⟩
      simpa [userAgentTaskExec] using TransitionTrace.append htrace₁ htrace₂

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
    TransitionTrace
      step
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
