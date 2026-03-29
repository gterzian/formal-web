import FormalWeb.Fetch
import FormalWeb.UserAgent

/-
Pure runtime model for the executable logic in `Main.lean`, together with
refinement lemmas relating runtime-message handling to the Fetch and UserAgent
transition systems.
-/
namespace FormalWeb

inductive RuntimeMessage where
  | freshTopLevelTraversable (destinationURL : String)
  | renderingOpportunity
  | fetchCompleted (controllerId : Nat) (response : NavigationResponse)
deriving Repr, DecidableEq

structure SpawnedFetchTask where
  controllerId : Nat
  request : NavigationRequest
deriving Repr, DecidableEq

structure RuntimeState where
  userAgent : UserAgent := default
  fetch : Fetch := default
  startupTraversableId : Option Nat := none
deriving Repr, Inhabited

structure RuntimeMessageResult where
  state : RuntimeState
  spawnedFetchTasks : List SpawnedFetchTask := []
  sentNewTopLevelTraversable : Bool := false
  error : Option String := none
deriving Repr

def startupNavigationFailureDetails
    (destinationURL : String)
    (userAgent : UserAgent)
    (traversableId : Nat) :
    String :=
  let fetchScheme := isFetchScheme destinationURL
  match traversable? userAgent traversableId with
  | none =>
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; traversable missing after navigate, destinationURL={destinationURL}, fetchScheme={fetchScheme}"
  | some traversable =>
      let ongoingNavigation := traversable.toTraversableNavigable.toNavigable.ongoingNavigation
      let activeDocumentUrl :=
        (traversable.toTraversableNavigable.activeDocument.map (·.url)).getD "<none>"
      let ongoingNavigationDescription :=
        match ongoingNavigation with
        | none => "none"
        | some (.navigationId navigationId) => s!"navigationId({navigationId})"
        | some .traversal => "traversal"
      let pendingNavigationFetchDescription :=
        match ongoingNavigation with
        | some (.navigationId navigationId) =>
            match UserAgent.pendingNavigationFetch? userAgent navigationId with
            | some pendingNavigationFetch =>
                s!"present(navigationId={pendingNavigationFetch.navigationId}, requestUrl={pendingNavigationFetch.request.url}, method={pendingNavigationFetch.request.method})"
            | none =>
                s!"missing(navigationId={navigationId})"
        | some .traversal => "not-applicable(traversal)"
        | none => "not-applicable(no ongoing navigation)"
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; destinationURL={destinationURL}, fetchScheme={fetchScheme}, activeDocumentUrl={activeDocumentUrl}, ongoingNavigation={ongoingNavigationDescription}, pendingNavigationFetch={pendingNavigationFetchDescription}"

def bootstrapFreshTopLevelTraversable
    (destinationURL : String)
    (userAgent : UserAgent) :
    Except String (UserAgent × Nat × PendingFetchRequest) :=
  let (userAgent, traversable) := createNewTopLevelTraversable userAgent none ""
  let (userAgent, pendingFetchRequest) :=
    navigateWithPendingFetchRequest userAgent traversable destinationURL
  match pendingFetchRequest with
  | some pendingFetchRequest => .ok (userAgent, traversable.id, pendingFetchRequest)
  | none => .error (startupNavigationFailureDetails destinationURL userAgent traversable.id)

def startupTraversableReadyHtml?
    (userAgent : UserAgent)
    (traversableId : Nat) :
    Option String := do
  let traversable <- traversable? userAgent traversableId
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let document <- traversable.toTraversableNavigable.activeDocument
    pure (UserAgent.documentHtml userAgent document)

def handleRuntimeMessagePure
    (state : RuntimeState)
    (message : RuntimeMessage) :
    RuntimeMessageResult :=
  match message with
  | .freshTopLevelTraversable destinationURL =>
      match bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | .ok (userAgent, traversableId, pendingFetchRequest) =>
          let (fetch, controller) := conceptFetch state.fetch pendingFetchRequest
          {
            state := {
              state with
                userAgent
                fetch
                startupTraversableId := some traversableId
            }
            spawnedFetchTasks := [
              {
                controllerId := controller.id
                request := pendingFetchRequest.request
              }
            ]
          }
      | .error error =>
          { state, error := some error }
  | .renderingOpportunity =>
      -- Notes: The executable runtime performs rendering side effects here without mutating
      -- the Fetch or UserAgent model state, so the pure abstraction stutters.
      { state }
  | .fetchCompleted controllerId response =>
      match state.fetch.pendingFetches.get? controllerId with
      | none =>
          { state }
      | some pendingFetch =>
          let fetch := (completeFetch state.fetch controllerId).1
          let userAgent := processNavigationFetchResponse state.userAgent pendingFetch.navigationId response
          let sentNewTopLevelTraversable :=
            match state.startupTraversableId with
            | none => false
            | some traversableId =>
                (startupTraversableReadyHtml? userAgent traversableId).isSome
          {
            state := { state with userAgent, fetch }
            sentNewTopLevelTraversable
          }

def runtimeStep
    (state : RuntimeState)
    (message : RuntimeMessage) :
    RuntimeState :=
  (handleRuntimeMessagePure state message).state

inductive TransitionTrace
    (stepFn : σ → α → Option σ) :
    σ → List α → σ → Prop where
  | nil (state : σ) : TransitionTrace stepFn state [] state
  | cons
      {start intermediate finish : σ}
      {action : α}
      {actions : List α} :
      stepFn start action = some intermediate →
      TransitionTrace stepFn intermediate actions finish →
      TransitionTrace stepFn start (action :: actions) finish

namespace TransitionTrace

theorem single
    {stepFn : σ → α → Option σ}
    {start finish : σ}
    {action : α}
    (h : stepFn start action = some finish) :
    TransitionTrace stepFn start [action] finish :=
  .cons h (.nil finish)

theorem append
    {stepFn : σ → α → Option σ}
    {start middle finish : σ}
    {xs ys : List α}
    (hxs : TransitionTrace stepFn start xs middle)
    (hys : TransitionTrace stepFn middle ys finish) :
    TransitionTrace stepFn start (xs ++ ys) finish := by
  induction hxs with
  | nil state =>
      simpa using hys
  | @cons start intermediate finish action actions hstep htrace ih =>
      simp
      exact TransitionTrace.cons hstep (ih hys)

end TransitionTrace

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

/--
Explicit `UserAgentAction` lists associated with each runtime message.

For startup success, the runtime exposes the intended pair of LTS-visible user-agent
actions even though the current proof still tracks the second step through the helper
composition used by `navigateWithPendingFetchRequest`.
-/
inductive RuntimeMessageUserAgentActionShape : RuntimeMessage → List UserAgentAction → Prop where
  | freshTopLevelTraversableError
      (destinationURL : String) :
      RuntimeMessageUserAgentActionShape (.freshTopLevelTraversable destinationURL) []
  | freshTopLevelTraversableSuccess
      (destinationURL : String)
      (traversableId : Nat) :
      RuntimeMessageUserAgentActionShape
        (.freshTopLevelTraversable destinationURL)
        [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
  | renderingOpportunity :
      RuntimeMessageUserAgentActionShape .renderingOpportunity []
  | fetchCompleted
      (navigationId : Nat)
      (response : NavigationResponse) :
      RuntimeMessageUserAgentActionShape
        (.fetchCompleted navigationId response)
        [.completeNavigation navigationId response]

/--
Runtime-message refinement into the user-agent LTS as an explicit action list and trace.

This packages the message-specific action-shape theorem with the corresponding
`TransitionTrace step ...` witness.
-/
def RuntimeMessageUserAgentRefinement
    (startUserAgent : UserAgent)
    (message : RuntimeMessage)
    (finishUserAgent : UserAgent) :
    Prop :=
  ∃ actions,
    RuntimeMessageUserAgentActionShape message actions ∧
    TransitionTrace
      step
      startUserAgent
      actions
      finishUserAgent

theorem handleRuntimeMessagePure_fetch_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ actions,
      TransitionTrace
        fetchStep
        state.fetch
        actions
        (handleRuntimeMessagePure state message).state.fetch := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
      | error _ =>
          refine ⟨[], ?_⟩
          simp [handleRuntimeMessagePure, hbootstrap, TransitionTrace.nil]
      | ok result =>
          refine ⟨[.startFetch result.2.2], ?_⟩
          refine TransitionTrace.single ?_
          simp [handleRuntimeMessagePure, hbootstrap, fetchStep, conceptFetch]
  | renderingOpportunity =>
      refine ⟨[], ?_⟩
      simp [handleRuntimeMessagePure, TransitionTrace.nil]
  | fetchCompleted controllerId response =>
      cases hlookup : state.fetch.pendingFetches.get? controllerId with
      | none =>
          refine ⟨[], ?_⟩
          have hlookup' : state.fetch.pendingFetches[controllerId]? = none := by
            simpa using hlookup
          simp [handleRuntimeMessagePure, hlookup', TransitionTrace.nil]
      | some pendingFetch =>
          refine ⟨[.completeFetch controllerId], ?_⟩
          have hlookup' : state.fetch.pendingFetches[controllerId]? = some pendingFetch := by
            simpa using hlookup
          have hresult :
              (handleRuntimeMessagePure state (.fetchCompleted controllerId response)).state.fetch =
                (completeFetch state.fetch controllerId).1 := by
            simp [handleRuntimeMessagePure, hlookup']
          have hstep :
              fetchStep state.fetch (.completeFetch controllerId) =
                some ((completeFetch state.fetch controllerId).1) := by
            simp [fetchStep, completeFetch, hlookup']
          simpa [hresult] using TransitionTrace.single hstep

theorem handleRuntimeMessagePure_userAgent_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
        (handleRuntimeMessagePure state message).state.userAgent := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
    cases hbootstrap : bootstrapFreshTopLevelTraversable destinationURL state.userAgent with
    | error _ =>
      refine ⟨[], ?_⟩
      simp [handleRuntimeMessagePure, hbootstrap, TransitionTrace.nil]
    | ok result =>
      refine ⟨
      [
        .createTopLevelTraversable "",
        .beginNavigation result.2.1 destinationURL none
      ],
      ?_
      ⟩
      · simpa [handleRuntimeMessagePure, hbootstrap] using
        startupSuccess_trace state.userAgent result.1 destinationURL result.2.1 result.2.2 hbootstrap
  | renderingOpportunity =>
    refine ⟨[], ?_⟩
    simp [handleRuntimeMessagePure, TransitionTrace.nil]
  | fetchCompleted controllerId response =>
    cases hlookup : state.fetch.pendingFetches.get? controllerId with
    | none =>
        refine ⟨[], ?_⟩
        have hlookup' : state.fetch.pendingFetches[controllerId]? = none := by
          simpa using hlookup
        simp [handleRuntimeMessagePure, hlookup', TransitionTrace.nil]
    | some pendingFetch =>
        refine ⟨[.completeNavigation pendingFetch.navigationId response], ?_⟩
        refine TransitionTrace.single ?_
        have hlookup' : state.fetch.pendingFetches[controllerId]? = some pendingFetch := by
          simpa using hlookup
        simp [handleRuntimeMessagePure, hlookup', step, processNavigationFetchResponse]

theorem handleRuntimeMessagePure_userAgent_trace
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
      (handleRuntimeMessagePure state message).state.userAgent :=
  handleRuntimeMessagePure_userAgent_refines state message

/-- Lift the total runtime transition into the partial `TransitionTrace` interface. -/
def runtimeTraceStep
    (state : RuntimeState)
    (message : RuntimeMessage) :
    Option RuntimeState :=
  some (runtimeStep state message)

/-- Deterministic execution of a list of runtime messages. -/
def runtimeExec
    (state : RuntimeState)
    (messages : List RuntimeMessage) :
    RuntimeState :=
  messages.foldl runtimeStep state

theorem runtimeExec_nil
    (state : RuntimeState) :
    runtimeExec state [] = state := by
  rfl

theorem runtimeExec_cons
    (state : RuntimeState)
    (message : RuntimeMessage)
    (messages : List RuntimeMessage) :
    runtimeExec state (message :: messages) = runtimeExec (runtimeStep state message) messages := by
  rfl

/-- Multi-step trace generated by deterministic execution of runtime messages. -/
theorem runtimeExec_trace
    (state : RuntimeState)
    (messages : List RuntimeMessage) :
    TransitionTrace
      runtimeTraceStep
      state
      messages
      (runtimeExec state messages) := by
  induction messages generalizing state with
  | nil =>
      simp [runtimeExec, TransitionTrace.nil]
  | cons message messages ih =>
      refine TransitionTrace.cons (intermediate := runtimeStep state message) ?_ ?_
      · simp [runtimeTraceStep]
      · simpa [runtimeExec_cons] using ih (runtimeStep state message)

/-- Clearer alias for the fetch-LTS refinement of runtime-message handling. -/
theorem runtimeMessage_refines_fetch
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ actions,
      TransitionTrace
        fetchStep
        state.fetch
        actions
        (handleRuntimeMessagePure state message).state.fetch :=
  handleRuntimeMessagePure_fetch_refines state message

/-- Clearer alias for the user-agent refinement of runtime-message handling. -/
theorem runtimeMessage_refines_userAgent
    (state : RuntimeState)
    (message : RuntimeMessage) :
    ∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
        (handleRuntimeMessagePure state message).state.userAgent :=
  handleRuntimeMessagePure_userAgent_refines state message

/-- Combined refinement statement for handling a runtime message in the pure runtime model. -/
theorem handleRuntimeMessagePure_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    (∃ actions,
      TransitionTrace
        step
        state.userAgent
        actions
        (handleRuntimeMessagePure state message).state.userAgent) ∧
    (∃ actions,
      TransitionTrace
        fetchStep
        state.fetch
        actions
        (handleRuntimeMessagePure state message).state.fetch) := by
  refine ⟨?_, ?_⟩
  · exact handleRuntimeMessagePure_userAgent_trace state message
  · exact handleRuntimeMessagePure_fetch_refines state message

/-- Combined refinement statement for one pure runtime step. -/
theorem runtimeStep_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    (∃ userAgentActions,
      TransitionTrace
        step
        state.userAgent
        userAgentActions
        (runtimeStep state message).userAgent) ∧
    (∃ fetchActions,
      TransitionTrace
        fetchStep
        state.fetch
        fetchActions
        (runtimeStep state message).fetch) := by
  simpa [runtimeStep] using handleRuntimeMessagePure_refines state message

/-- List-free formulation of `runtimeStep_refines` for one runtime message. -/
theorem runtimeMessage_step_refines
    (state : RuntimeState)
    (message : RuntimeMessage) :
    (∃ userAgentActions,
      TransitionTrace
        step
        state.userAgent
        userAgentActions
        (runtimeStep state message).userAgent) ∧
    (∃ fetchActions,
      TransitionTrace
        fetchStep
        state.fetch
        fetchActions
        (runtimeStep state message).fetch) :=
  runtimeStep_refines state message

/--
List-level refinement theorem for the deterministic pure runtime machine.

Executing a list of runtime messages yields some user-agent action trace and some
fetch action trace that reach the corresponding projections of the final runtime state.
-/
theorem runtimeExec_refines
    (state : RuntimeState)
    (messages : List RuntimeMessage) :
    (∃ userAgentActions,
      TransitionTrace
        step
        state.userAgent
        userAgentActions
        (runtimeExec state messages).userAgent) ∧
    (∃ fetchActions,
      TransitionTrace
        fetchStep
        state.fetch
        fetchActions
        (runtimeExec state messages).fetch) := by
  induction messages generalizing state with
  | nil =>
      constructor
      · refine ⟨[], ?_⟩
        simp [runtimeExec, TransitionTrace.nil]
      · refine ⟨[], ?_⟩
        simp [runtimeExec, TransitionTrace.nil]
  | cons message messages ih =>
      have hstep := runtimeMessage_step_refines state message
      have htail := ih (runtimeStep state message)
      rcases hstep with ⟨⟨userAgentActions₁, huserAgent₁⟩, ⟨fetchActions₁, hfetch₁⟩⟩
      rcases htail with ⟨⟨userAgentActions₂, huserAgent₂⟩, ⟨fetchActions₂, hfetch₂⟩⟩
      constructor
      · refine ⟨userAgentActions₁ ++ userAgentActions₂, ?_⟩
        simpa [runtimeExec_cons] using TransitionTrace.append huserAgent₁ huserAgent₂
      · refine ⟨fetchActions₁ ++ fetchActions₂, ?_⟩
        simpa [runtimeExec_cons] using TransitionTrace.append hfetch₁ hfetch₂

theorem default_runtimeState_has_no_traversable
    (traversableId : Nat) :
    (traversable? (default : RuntimeState).userAgent traversableId).isNone = true := by
  change
    (match (Std.TreeMap.empty : Std.TreeMap Nat TopLevelTraversable)[traversableId]? with
      | some _ => false
      | none => true) = true
  simp

theorem default_runtimeState_has_no_pendingNavigationFetch
    (navigationId : Nat) :
    (UserAgent.pendingNavigationFetch? (default : RuntimeState).userAgent navigationId).isNone = true := by
  change
    (match (Std.TreeMap.empty : Std.TreeMap Nat PendingNavigationFetch)[navigationId]? with
      | some _ => false
      | none => true) = true
  simp

theorem default_runtimeState_has_no_pendingFetch
    (controllerId : Nat) :
    (Fetch.pendingFetch? (default : RuntimeState).fetch controllerId).isNone = true := by
  change
    (match (Std.TreeMap.empty : Std.TreeMap Nat PendingFetch)[controllerId]? with
      | some _ => false
      | none => true) = true
  simp

theorem default_runtimeState_empty
    (traversableId navigationId controllerId : Nat) :
    (default : RuntimeState).startupTraversableId = none ∧
    (traversable? (default : RuntimeState).userAgent traversableId).isNone = true ∧
    (UserAgent.pendingNavigationFetch? (default : RuntimeState).userAgent navigationId).isNone = true ∧
    (Fetch.pendingFetch? (default : RuntimeState).fetch controllerId).isNone = true := by
  exact ⟨
    rfl,
    default_runtimeState_has_no_traversable traversableId,
    default_runtimeState_has_no_pendingNavigationFetch navigationId,
    default_runtimeState_has_no_pendingFetch controllerId
  ⟩

theorem runtimeExec_startup_from_default_success
    (destinationURL : String)
    (nextUserAgent : UserAgent)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL (default : RuntimeState).userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).startupTraversableId = some traversableId ∧
    (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).userAgent = nextUserAgent ∧
    (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).fetch =
      (conceptFetch (default : RuntimeState).fetch pendingFetchRequest).1 ∧
    (handleRuntimeMessagePure (default : RuntimeState) (.freshTopLevelTraversable destinationURL)).error = none ∧
    (handleRuntimeMessagePure (default : RuntimeState) (.freshTopLevelTraversable destinationURL)).spawnedFetchTasks =
      [{
        controllerId := (conceptFetch (default : RuntimeState).fetch pendingFetchRequest).2.id
        request := pendingFetchRequest.request
      }] ∧
    TransitionTrace
      step
      (default : RuntimeState).userAgent
      [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
      (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).userAgent ∧
    TransitionTrace
      fetchStep
      (default : RuntimeState).fetch
      [.startFetch pendingFetchRequest]
      (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).fetch := by
  refine ⟨?_, ?_, ?_, ?_, ?_, ?_, ?_⟩
  · simp [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap]
  · simp [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap]
  · simp [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap]
  · simp [handleRuntimeMessagePure, hbootstrap]
  · simp [handleRuntimeMessagePure, hbootstrap, conceptFetch]
  · simpa [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap] using
      startupSuccess_trace
        (default : RuntimeState).userAgent
        nextUserAgent
        destinationURL
        traversableId
        pendingFetchRequest
        hbootstrap
  · refine TransitionTrace.single ?_
    simpa [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap] using
      (show
        fetchStep
          (default : RuntimeState).fetch
          (.startFetch pendingFetchRequest) =
            some ((conceptFetch (default : RuntimeState).fetch pendingFetchRequest).1) by
          simp [fetchStep, conceptFetch])

theorem runtimeExec_startup_from_default_pendingFetch
    (destinationURL : String)
    (nextUserAgent : UserAgent)
    (traversableId : Nat)
    (pendingFetchRequest : PendingFetchRequest)
    (hbootstrap :
      bootstrapFreshTopLevelTraversable destinationURL (default : RuntimeState).userAgent =
        .ok (nextUserAgent, traversableId, pendingFetchRequest)) :
    Fetch.pendingFetch?
      (runtimeExec (default : RuntimeState) [.freshTopLevelTraversable destinationURL]).fetch
      (conceptFetch (default : RuntimeState).fetch pendingFetchRequest).2.id =
      some {
        navigationId := pendingFetchRequest.navigationId
        request := pendingFetchRequest.request
        controller := (conceptFetch (default : RuntimeState).fetch pendingFetchRequest).2
      } := by
  simp [runtimeExec, runtimeStep, handleRuntimeMessagePure, hbootstrap, Fetch.pendingFetch?, conceptFetch]

end FormalWeb
