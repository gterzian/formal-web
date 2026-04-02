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

theorem handleFetchTaskMessagePure_refines
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        fetchLTS
        fetch
        actions
        (handleFetchTaskMessagePure fetch message).state := by
  cases message with
  | startFetch pendingRequest =>
      refine ⟨[.startFetch pendingRequest], ?_⟩
      refine TransitionSystem.TransitionTrace.single ?_
      simp [handleFetchTaskMessagePure, fetchLTS, conceptNavigationFetch, conceptFetch, FetchTaskResult.state]
  | startDocumentFetch pendingRequest =>
      refine ⟨[.startDocumentFetch pendingRequest], ?_⟩
      refine TransitionSystem.TransitionTrace.single ?_
      simp [handleFetchTaskMessagePure, fetchLTS, conceptDocumentFetch, conceptFetch, FetchTaskResult.state]
  | finishFetch controllerId response =>
      cases hlookup : fetch.pendingFetches.get? controllerId with
      | none =>
          refine ⟨[], ?_⟩
          have hlookup' : fetch.pendingFetches[controllerId]? = none := by
            simpa using hlookup
          simpa [handleFetchTaskMessagePure, completeFetch, hlookup', FetchTaskResult.state] using
            (TransitionSystem.TransitionTrace.nil fetch)
      | some pendingFetch =>
          refine ⟨[.completeFetch controllerId], ?_⟩
          have hlookup' : fetch.pendingFetches[controllerId]? = some pendingFetch := by
            simpa using hlookup
          have hresult :
              (handleFetchTaskMessagePure fetch (.finishFetch controllerId response)).state =
                (completeFetch fetch controllerId).1 := by
            simp [handleFetchTaskMessagePure, completeFetch, hlookup', FetchTaskResult.state]
          have htrans :
              fetchLTS.trans fetch (.completeFetch controllerId) ((completeFetch fetch controllerId).1) := by
            refine ⟨pendingFetch, ?_⟩
            simp [completeFetch, hlookup']
          simpa [hresult] using TransitionSystem.TransitionTrace.single htrans

theorem fetchTaskStep_refines
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionSystem.TransitionTrace
        fetchLTS
        fetch
        actions
        (fetchTaskStep fetch message) := by
  simpa [fetchTaskStep] using handleFetchTaskMessagePure_refines fetch message

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
      have hstep := fetchTaskStep_refines fetch message
      have htail := ih (fetchTaskStep fetch message)
      rcases hstep with ⟨actions₁, htrace₁⟩
      rcases htail with ⟨actions₂, htrace₂⟩
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
    (FetchTaskResult.toSpawnFetchTasks
      (handleFetchTaskMessagePure (default : Fetch) (.startFetch pendingFetchRequest))) =
      [{
        controllerId := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).2.id
        request := pendingFetchRequest.request
      }] ∧
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
  · change (handleFetchTaskMessagePure (default : Fetch) (.startFetch pendingFetchRequest)).state =
      (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1
    simp [handleFetchTaskMessagePure, conceptNavigationFetch, conceptFetch, FetchTaskResult.state]
  · simp [handleFetchTaskMessagePure, conceptNavigationFetch, conceptFetch, FetchTaskResult.toSpawnFetchTasks]
  · simpa [fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure, FetchTaskResult.state] using
      (TransitionSystem.TransitionTrace.single
        (sys := fetchLTS)
        (start := (default : Fetch))
        (finish := (conceptNavigationFetch (default : Fetch) pendingFetchRequest).1)
        (action := .startFetch pendingFetchRequest)
        (by simp [fetchLTS, conceptNavigationFetch, conceptFetch]))
  · simp [Fetch.pendingFetch?, fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure,
      conceptNavigationFetch, conceptFetch, FetchTaskResult.state]

end FormalWeb
