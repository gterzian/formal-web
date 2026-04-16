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
                    simp [hc]
                  have hc_snd : (completeFetch { pendingFetches := pendingFetches } controllerId).2 = none := by
                    simp [hc]
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
                    simp [hc]
                  have hc_snd : (completeFetch { pendingFetches := pendingFetches } controllerId).2 = none := by
                    simp [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            simp [heffects, interpretEffects]
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
                    simp [hc]
                  have hc_snd :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).2 = some pendingFetch := by
                    simp [hc]
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
                    simp [hc]
                  have hc_snd :
                      (completeFetch { pendingFetches := pendingFetches } controllerId).2 = some pendingFetch := by
                    simp [hc]
                  unfold finishFetchM
                  simp [hc_fst, hc_snd]
            simp [heffects, interpretEffects, interpretEffect]

end FormalWeb
