import FormalWeb.Fetch
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

inductive FetchAction
  | startFetch (pendingRequest : PendingFetchRequest)
  | startDocumentFetch (pendingRequest : DocumentFetchRequest)
  | completeFetch (controllerId : Nat)
deriving Repr, DecidableEq

/-- Relational LTS for the fetch state machine. -/
def fetchLTS : TransitionSystem.LTS Fetch FetchAction where
  init := fun fetch => fetch = default
  trans := fun fetch action fetch' =>
    match action with
    | .startFetch pendingRequest =>
        fetch' = (conceptNavigationFetch fetch pendingRequest).1
    | .startDocumentFetch pendingRequest =>
        fetch' = (conceptDocumentFetch fetch pendingRequest).1
    | .completeFetch controllerId =>
        ∃ pendingFetch,
          completeFetch fetch controllerId = (fetch', some pendingFetch)

def interpretEffect : FetchEffect → List FetchAction
  | .startFetch pendingRequest _ =>
      [.startFetch pendingRequest]
  | .startDocumentFetch pendingRequest _ =>
      [.startDocumentFetch pendingRequest]
  | .completeFetch controllerId _ _ =>
      [.completeFetch controllerId]

def interpretEffects (effects : Array FetchEffect) : List FetchAction :=
  effects.toList.flatMap interpretEffect

theorem handleFetchTaskMessage_full_refinement
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        fetchLTS
        fetch
        actions
        (runFetchMonadic fetch message).2 ∧
      interpretEffects (runFetchMonadic fetch message).1 = actions := by
  cases message with
  | startFetch pendingRequest =>
      refine ⟨[.startFetch pendingRequest], ?_, ?_⟩
      · refine TransitionSystem.TransitionTrace.single ?_
        change fetchLTS.trans fetch (.startFetch pendingRequest) (conceptNavigationFetch fetch pendingRequest).1
        simp [fetchLTS]
      · change interpretEffects
            #[
              FetchEffect.startFetch
                pendingRequest
                { controllerId := pendingRequest.fetchId, request := pendingRequest.request }
            ] =
            [.startFetch pendingRequest]
        simp [interpretEffects, interpretEffect]
  | startDocumentFetch pendingRequest =>
      refine ⟨[.startDocumentFetch pendingRequest], ?_, ?_⟩
      · refine TransitionSystem.TransitionTrace.single ?_
        change fetchLTS.trans fetch (.startDocumentFetch pendingRequest) (conceptDocumentFetch fetch pendingRequest).1
        simp [fetchLTS]
      · change interpretEffects
            #[
              FetchEffect.startDocumentFetch
                pendingRequest
                { controllerId := pendingRequest.fetchId, request := pendingRequest.request }
            ] =
            [.startDocumentFetch pendingRequest]
        simp [interpretEffects, interpretEffect]
  | finishFetch controllerId response =>
      cases hlookup : fetch.pendingFetches.get? controllerId with
      | none =>
          refine ⟨[], ?_, ?_⟩
          · have hlookup' : fetch.pendingFetches[controllerId]? = none := by
              simpa using hlookup
            have hstate : (runFetchMonadic fetch (.finishFetch controllerId response)).2 = fetch := by
              cases fetch with
              | mk pendingFetches =>
                  change (match (finishFetchM controllerId response) { pendingFetches := pendingFetches } with
                    | ((PUnit.unit, effects), nextFetch) => nextFetch) = { pendingFetches := pendingFetches }
                  have hc : completeFetch { pendingFetches := pendingFetches } controllerId = ({ pendingFetches := pendingFetches }, none) :=
                    completeFetch_none { pendingFetches := pendingFetches } controllerId hlookup'
                  have hc_fst : (completeFetch { pendingFetches := pendingFetches } controllerId).1 = { pendingFetches := pendingFetches } := by
                    simpa [hc]
                  have hc_snd : (completeFetch { pendingFetches := pendingFetches } controllerId).2 = none := by
                    simpa [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            simpa [hstate] using (TransitionSystem.TransitionTrace.nil fetch)
          · have hlookup' : fetch.pendingFetches[controllerId]? = none := by
              simpa using hlookup
            have heffects : (runFetchMonadic fetch (.finishFetch controllerId response)).1 = #[] := by
              cases fetch with
              | mk pendingFetches =>
                  change (match (finishFetchM controllerId response) { pendingFetches := pendingFetches } with
                    | ((PUnit.unit, effects), nextFetch) => effects) = #[]
                  have hc : completeFetch { pendingFetches := pendingFetches } controllerId = ({ pendingFetches := pendingFetches }, none) :=
                    completeFetch_none { pendingFetches := pendingFetches } controllerId hlookup'
                  have hc_fst : (completeFetch { pendingFetches := pendingFetches } controllerId).1 = { pendingFetches := pendingFetches } := by
                    simpa [hc]
                  have hc_snd : (completeFetch { pendingFetches := pendingFetches } controllerId).2 = none := by
                    simpa [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            simpa [heffects, interpretEffects]
      | some pendingFetch =>
          refine ⟨[.completeFetch controllerId], ?_, ?_⟩
          · have hlookup' : fetch.pendingFetches[controllerId]? = some pendingFetch := by
              simpa using hlookup
            have hstate :
                (runFetchMonadic fetch (.finishFetch controllerId response)).2 =
                  (completeFetch fetch controllerId).1 := by
              cases fetch with
              | mk pendingFetches =>
                  change (match (finishFetchM controllerId response) { pendingFetches := pendingFetches } with
                    | ((PUnit.unit, effects), nextFetch) => nextFetch) =
                    (completeFetch { pendingFetches := pendingFetches } controllerId).1
                  have hc : completeFetch { pendingFetches := pendingFetches } controllerId =
                      ({ pendingFetches := pendingFetches.erase controllerId }, some pendingFetch) :=
                    completeFetch_some { pendingFetches := pendingFetches } controllerId pendingFetch hlookup'
                  have hc_fst :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).1 =
                        { pendingFetches := pendingFetches.erase controllerId } := by
                    simpa [hc]
                  have hc_snd :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).2 = some pendingFetch := by
                    simpa [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            have htrans :
                fetchLTS.trans fetch (.completeFetch controllerId) (completeFetch fetch controllerId).1 := by
              refine ⟨pendingFetch, ?_⟩
              simp [completeFetch, hlookup']
            simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
          · have hlookup' : fetch.pendingFetches[controllerId]? = some pendingFetch := by
              simpa using hlookup
            have heffects :
                (runFetchMonadic fetch (.finishFetch controllerId response)).1 =
                  #[FetchEffect.completeFetch controllerId response (.fetchCompleted controllerId response)] := by
              cases fetch with
              | mk pendingFetches =>
                  change (match (finishFetchM controllerId response) { pendingFetches := pendingFetches } with
                    | ((PUnit.unit, effects), nextFetch) => effects) =
                    #[FetchEffect.completeFetch controllerId response (.fetchCompleted controllerId response)]
                  have hc : completeFetch { pendingFetches := pendingFetches } controllerId =
                      ({ pendingFetches := pendingFetches.erase controllerId }, some pendingFetch) :=
                    completeFetch_some { pendingFetches := pendingFetches } controllerId pendingFetch hlookup'
                  have hc_fst :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).1 =
                        { pendingFetches := pendingFetches.erase controllerId } := by
                    simpa [hc]
                  have hc_snd :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).2 = some pendingFetch := by
                    simpa [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            simpa [heffects, interpretEffects, interpretEffect]

theorem fetchTaskStep_refines
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        fetchLTS
        fetch
        actions
        (fetchTaskStep fetch message) := by
  rcases handleFetchTaskMessage_full_refinement fetch message with ⟨actions, htrace, _⟩
  exact ⟨actions, by simpa [fetchTaskStep] using htrace⟩

theorem fetchTaskExec_refines
    (fetch : Fetch)
    (messages : List FetchTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        fetchLTS
        fetch
        actions
        (fetchTaskExec fetch messages) := by
  induction messages generalizing fetch with
  | nil =>
      refine ⟨[], ?_⟩
      simp [fetchTaskExec, TransitionSystem.TransitionTrace.nil]
  | cons message messages ih =>
      rcases fetchTaskStep_refines fetch message with ⟨actions₁, htrace₁⟩
      rcases ih (fetchTaskStep fetch message) with ⟨actions₂, htrace₂⟩
      refine ⟨actions₁ ++ actions₂, ?_⟩
      simpa [fetchTaskExec] using TransitionSystem.TransitionTrace.append htrace₁ htrace₂

theorem default_fetch_has_no_pendingFetch
    (controllerId : Nat) :
    (Fetch.pendingFetch? (default : Fetch) controllerId).isNone = true := by
  change (match (Std.TreeMap.empty : Std.TreeMap Nat PendingFetch)[controllerId]? with
    | some _ => false
    | none => true) = true
  simp

theorem fetchTaskExec_startFetch_from_default
    (pendingFetchRequest : PendingFetchRequest) :
    (fetchTaskExec (default : Fetch) [.startFetch pendingFetchRequest]) =
      (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1 ∧
    (runFetchMonadic (default : Fetch) (.startFetch pendingFetchRequest)).1 =
      #[
        FetchEffect.startFetch
          pendingFetchRequest
          {
            controllerId := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2.id
            request := pendingFetchRequest.request
          }
      ] ∧
    TransitionSystem.TransitionTrace
      fetchLTS
      (default : Fetch)
      [.startFetch pendingFetchRequest]
      (fetchTaskExec (default : Fetch) [.startFetch pendingFetchRequest]) ∧
    Fetch.pendingFetch?
      (fetchTaskExec (default : Fetch) [.startFetch pendingFetchRequest])
      (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2.id =
      some {
        request := pendingFetchRequest.request
        controller := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2
      } := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · change (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1 =
      (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1
    rfl
  · change #[
        FetchEffect.startFetch
          pendingFetchRequest
          {
            controllerId := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2.id
            request := pendingFetchRequest.request
          }
      ] =
      #[
        FetchEffect.startFetch
          pendingFetchRequest
          {
            controllerId := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2.id
            request := pendingFetchRequest.request
          }
      ]
    rfl
  · simpa [fetchTaskExec, fetchTaskStep] using
      (TransitionSystem.TransitionTrace.single
        (sys := fetchLTS)
        (start := (default : Fetch))
        (finish := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1)
        (action := .startFetch pendingFetchRequest)
        (by simp [fetchLTS, conceptNavigationFetch, conceptFetch]))
  · change (runFetchMonadic (default : Fetch) (.startFetch pendingFetchRequest)).2.pendingFetches[pendingFetchRequest.fetchId]? =
      some { request := pendingFetchRequest.request, controller := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2 }
    simp [runFetchMonadic_startFetch, conceptNavigationFetch, conceptFetch]


end FormalWeb
