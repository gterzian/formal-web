import FormalWeb.UserAgent

namespace FormalWeb

/-- Proof-only combined runtime state for reasoning about fetch and user-agent interaction. -/
structure RuntimeState where
  userAgentState : UserAgentTaskState := default
  fetch : Fetch := default
  pendingUserAgentMessages : List UserAgentTaskMessage := []
  pendingFetchMessages : List FetchTaskMessage := []
deriving Repr, Inhabited

/-- Proof-only combined runtime queue item. -/
inductive RuntimeMessage where
  | userAgent (message : UserAgentTaskMessage)
  | fetch (message : FetchTaskMessage)
deriving Repr, DecidableEq

def userAgentMessagesOfFetchNotification
    (notification : FetchNotification) :
    List UserAgentTaskMessage :=
  match notification with
  | .fetchCompleted navigationId response =>
      [.fetchCompleted navigationId response]

def userAgentMessagesOfFetchNotifications
    (notifications : List FetchNotification) :
    List UserAgentTaskMessage :=
  match notifications with
  | [] => []
  | notification :: notifications =>
      userAgentMessagesOfFetchNotification notification ++
        userAgentMessagesOfFetchNotifications notifications

def handleRuntimeMessagePure
    (state : RuntimeState)
    (message : RuntimeMessage) :
    RuntimeState :=
  match message with
  | .userAgent message =>
      let result := handleUserAgentTaskMessagePure state.userAgentState message
      {
        state with
          userAgentState := result.state
          pendingFetchMessages := state.pendingFetchMessages ++ result.fetchMessages
      }
  | .fetch message =>
      let result := handleFetchTaskMessagePure state.fetch message
      {
        state with
          fetch := result.state
          pendingUserAgentMessages :=
            state.pendingUserAgentMessages ++
              userAgentMessagesOfFetchNotifications result.notifications
      }

def runtimeStep
    (state : RuntimeState)
    (message : RuntimeMessage) :
    RuntimeState :=
  handleRuntimeMessagePure state message

def runtimeExec
    (state : RuntimeState)
    (messages : List RuntimeMessage) :
    RuntimeState :=
  match messages with
  | [] => state
  | message :: messages => runtimeExec (runtimeStep state message) messages

def runNextPendingUserAgentMessage
    (state : RuntimeState) :
    Option RuntimeState :=
  match state.pendingUserAgentMessages with
  | [] => none
  | message :: pendingUserAgentMessages =>
      let result := handleUserAgentTaskMessagePure state.userAgentState message
      some {
        state with
          userAgentState := result.state
          pendingUserAgentMessages
          pendingFetchMessages := state.pendingFetchMessages ++ result.fetchMessages
      }

def runNextPendingFetchMessage
    (state : RuntimeState) :
    Option RuntimeState :=
  match state.pendingFetchMessages with
  | [] => none
  | message :: pendingFetchMessages =>
      let result := handleFetchTaskMessagePure state.fetch message
      some {
        state with
          fetch := result.state
          pendingFetchMessages
          pendingUserAgentMessages :=
            state.pendingUserAgentMessages ++
              userAgentMessagesOfFetchNotifications result.notifications
      }

theorem runtimeStep_userAgent_appends_fetchMessages
    (state : RuntimeState)
    (message : UserAgentTaskMessage) :
    let result := handleUserAgentTaskMessagePure state.userAgentState message
    (runtimeStep state (.userAgent message)).pendingFetchMessages =
      state.pendingFetchMessages ++ result.fetchMessages := by
  simp [runtimeStep, handleRuntimeMessagePure]

theorem runtimeStep_fetch_appends_userAgentMessages
    (state : RuntimeState)
    (message : FetchTaskMessage) :
    let result := handleFetchTaskMessagePure state.fetch message
    (runtimeStep state (.fetch message)).pendingUserAgentMessages =
      state.pendingUserAgentMessages ++
        userAgentMessagesOfFetchNotifications result.notifications := by
  simp [runtimeStep, handleRuntimeMessagePure]

theorem runNextPendingUserAgentMessage_none
    (state : RuntimeState)
    (h : state.pendingUserAgentMessages = []) :
    runNextPendingUserAgentMessage state = none := by
  simp [runNextPendingUserAgentMessage, h]

theorem runNextPendingFetchMessage_none
    (state : RuntimeState)
    (h : state.pendingFetchMessages = []) :
    runNextPendingFetchMessage state = none := by
  simp [runNextPendingFetchMessage, h]

theorem runNextPendingUserAgentMessage_consumes_head
    (state : RuntimeState)
    (message : UserAgentTaskMessage)
    (pendingUserAgentMessages : List UserAgentTaskMessage) :
    let result := handleUserAgentTaskMessagePure state.userAgentState message
    runNextPendingUserAgentMessage
      { state with pendingUserAgentMessages := message :: pendingUserAgentMessages } =
      some {
        state with
          userAgentState := result.state
          pendingUserAgentMessages
          pendingFetchMessages := state.pendingFetchMessages ++ result.fetchMessages
      } := by
  simp [runNextPendingUserAgentMessage]

theorem runNextPendingFetchMessage_consumes_head
    (state : RuntimeState)
    (message : FetchTaskMessage)
    (pendingFetchMessages : List FetchTaskMessage) :
    let result := handleFetchTaskMessagePure state.fetch message
    runNextPendingFetchMessage
      { state with pendingFetchMessages := message :: pendingFetchMessages } =
      some {
        state with
          fetch := result.state
          pendingFetchMessages
          pendingUserAgentMessages :=
            state.pendingUserAgentMessages ++
              userAgentMessagesOfFetchNotifications result.notifications
      } := by
  simp [runNextPendingFetchMessage]

theorem runtimeStep_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ userAgentActions fetchActions,
      TransitionTrace
        step
        state.userAgentState.userAgent
        userAgentActions
        (runtimeStep state message).userAgentState.userAgent ∧
      TransitionTrace
        fetchStep
        state.fetch
        fetchActions
        (runtimeStep state message).fetch := by
  cases message with
  | userAgent message =>
      rcases handleUserAgentTaskMessagePure_refines state.userAgentState message with
        ⟨userAgentActions, _shape, hUserAgent⟩
      refine ⟨userAgentActions, [], ?_, ?_⟩
      · simpa [runtimeStep, handleRuntimeMessagePure] using hUserAgent
      · simpa [runtimeStep, handleRuntimeMessagePure] using
          (TransitionTrace.nil state.fetch)
  | fetch message =>
      rcases handleFetchTaskMessagePure_refines state.fetch message with
        ⟨fetchActions, hFetch⟩
      refine ⟨[], fetchActions, ?_, ?_⟩
      · simpa [runtimeStep, handleRuntimeMessagePure] using
          (TransitionTrace.nil state.userAgentState.userAgent)
      · simpa [runtimeStep, handleRuntimeMessagePure] using hFetch

theorem runtimeExec_refines
    (state : RuntimeState)
    (messages : List RuntimeMessage) :
    ∃ userAgentActions fetchActions,
      TransitionTrace
        step
        state.userAgentState.userAgent
        userAgentActions
        (runtimeExec state messages).userAgentState.userAgent ∧
      TransitionTrace
        fetchStep
        state.fetch
        fetchActions
        (runtimeExec state messages).fetch := by
  induction messages generalizing state with
  | nil =>
      refine ⟨[], [], ?_, ?_⟩
      · simpa [runtimeExec] using
          (TransitionTrace.nil state.userAgentState.userAgent)
      · simpa [runtimeExec] using
          (TransitionTrace.nil state.fetch)
  | cons message messages ih =>
      rcases runtimeStep_refines state message with
        ⟨userAgentActions₁, fetchActions₁, hUserAgent₁, hFetch₁⟩
      rcases ih (runtimeStep state message) with
        ⟨userAgentActions₂, fetchActions₂, hUserAgent₂, hFetch₂⟩
      refine ⟨userAgentActions₁ ++ userAgentActions₂, fetchActions₁ ++ fetchActions₂, ?_, ?_⟩
      · simpa [runtimeExec] using TransitionTrace.append hUserAgent₁ hUserAgent₂
      · simpa [runtimeExec] using TransitionTrace.append hFetch₁ hFetch₂

theorem runtimeStep_freshTopLevelTraversable_enqueuesFetch_from_default
    (destinationURL : String)
    (nextUserAgent : UserAgent)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL (default : RuntimeState).userAgentState.userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    let afterStartup := runtimeStep (default : RuntimeState) (.userAgent (.freshTopLevelTraversable destinationURL))
    afterStartup.pendingFetchMessages = [.startFetch pendingFetchRequest] ∧
    afterStartup.userAgentState.userAgent = nextUserAgent ∧
    afterStartup.userAgentState.startupTraversableId = some traversableId := by
  refine ⟨?_, ?_, ?_⟩
  · change
      (default : RuntimeState).pendingFetchMessages ++
          (handleUserAgentTaskMessagePure
            (default : RuntimeState).userAgentState
            (.freshTopLevelTraversable destinationURL)).fetchMessages =
        [.startFetch pendingFetchRequest]
    have hpending : (default : RuntimeState).pendingFetchMessages = [] := rfl
    rw [hpending]
    simp [handleUserAgentTaskMessagePure, hbootstrap]
  · simp [runtimeStep, handleRuntimeMessagePure, handleUserAgentTaskMessagePure, hbootstrap]
  · simp [runtimeStep, handleRuntimeMessagePure, handleUserAgentTaskMessagePure, hbootstrap]

theorem runtimeStep_finishFetch_enqueuesFetchCompleted_from_started
    (userAgentState : UserAgentTaskState)
    (pendingFetchRequest : PendingFetchRequest)
    (response : NavigationResponse) :
    let controller := (conceptFetch (default : Fetch) pendingFetchRequest).2
    let state : RuntimeState := {
      userAgentState
      fetch := (conceptFetch (default : Fetch) pendingFetchRequest).1
      pendingUserAgentMessages := []
      pendingFetchMessages := []
    }
    let afterFetchDone := runtimeStep state (.fetch (.finishFetch controller.id response))
    afterFetchDone.pendingUserAgentMessages = [.fetchCompleted pendingFetchRequest.navigationId response] ∧
    afterFetchDone.pendingFetchMessages = [] := by
  simp [runtimeStep, handleRuntimeMessagePure, handleFetchTaskMessagePure, completeFetch, conceptFetch,
    FetchTaskResult.notifications, userAgentMessagesOfFetchNotifications,
    userAgentMessagesOfFetchNotification]

theorem runNextPendingUserAgentMessage_fetchCompleted_updatesUserAgent
    (state : UserAgentTaskState)
    (navigationId : Nat)
    (response : NavigationResponse) :
    let runtimeState : RuntimeState := {
      userAgentState := state
      fetch := default
      pendingUserAgentMessages := [.fetchCompleted navigationId response]
      pendingFetchMessages := []
    }
    ∃ afterResume,
      runNextPendingUserAgentMessage runtimeState = some afterResume ∧
      afterResume.pendingUserAgentMessages = [] ∧
      afterResume.pendingFetchMessages = [] ∧
      afterResume.userAgentState.userAgent = processNavigationFetchResponse state.userAgent navigationId response ∧
      afterResume.userAgentState.startupTraversableId = state.startupTraversableId := by
  let afterResume : RuntimeState := {
    userAgentState := {
      userAgent := processNavigationFetchResponse state.userAgent navigationId response
      startupTraversableId := state.startupTraversableId
      lastDispatchedEvent := state.lastDispatchedEvent
    }
    fetch := default
    pendingUserAgentMessages := []
    pendingFetchMessages := []
  }
  refine ⟨afterResume, ?_, ?_, ?_, ?_, ?_⟩
  · simp [afterResume, runNextPendingUserAgentMessage, handleUserAgentTaskMessagePure, processNavigationFetchResponse]
  · simp [afterResume]
  · simp [afterResume]
  · simp [afterResume]
  · simp [afterResume]

end FormalWeb
