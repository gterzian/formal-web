import FormalWeb.Fetch
import FormalWeb.TransitionTrace

namespace FormalWeb

inductive FetchAction
  | startFetch (pendingRequest : PendingFetchRequest)
  | startDocumentFetch (pendingRequest : DocumentFetchRequest)
  | completeFetch (controllerId : Nat)
deriving Repr, DecidableEq

/--
Apply one fetch transition.

This sits above helper algorithms such as `conceptFetch` and `completeFetch`,
which implement the details of each labeled step.
-/
def fetchStep
    (fetch : Fetch)
    (action : FetchAction) :
    Option Fetch :=
  match action with
  | .startFetch pendingRequest =>
      pure (conceptNavigationFetch fetch pendingRequest).1
  | .startDocumentFetch pendingRequest =>
      pure (conceptDocumentFetch fetch pendingRequest).1
  | .completeFetch controllerId =>
      let (fetch, pendingFetch?) := completeFetch fetch controllerId
      pendingFetch?.map (fun _ => fetch)

theorem handleFetchTaskMessagePure_refines
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionTrace
        fetchStep
        fetch
        actions
        (handleFetchTaskMessagePure fetch message).state := by
  cases message with
  | startFetch pendingRequest =>
      refine ⟨[.startFetch pendingRequest], ?_⟩
      refine TransitionTrace.single ?_
      simp [handleFetchTaskMessagePure, fetchStep, conceptNavigationFetch, conceptFetch, FetchTaskResult.state]
  | startDocumentFetch pendingRequest =>
      refine ⟨[.startDocumentFetch pendingRequest], ?_⟩
      refine TransitionTrace.single ?_
      simp [handleFetchTaskMessagePure, fetchStep, conceptDocumentFetch, conceptFetch, FetchTaskResult.state]
  | finishFetch controllerId response =>
      cases hlookup : fetch.pendingFetches.get? controllerId with
      | none =>
          refine ⟨[], ?_⟩
          have hlookup' : fetch.pendingFetches[controllerId]? = none := by
            simpa using hlookup
          simpa [handleFetchTaskMessagePure, completeFetch, hlookup', FetchTaskResult.state] using
            (TransitionTrace.nil fetch)
      | some pendingFetch =>
          refine ⟨[.completeFetch controllerId], ?_⟩
          have hlookup' : fetch.pendingFetches[controllerId]? = some pendingFetch := by
            simpa using hlookup
          have hresult :
              (handleFetchTaskMessagePure fetch (.finishFetch controllerId response)).state =
                (completeFetch fetch controllerId).1 := by
            simp [handleFetchTaskMessagePure, completeFetch, hlookup', FetchTaskResult.state]
          have hstep :
              fetchStep fetch (.completeFetch controllerId) =
                some ((completeFetch fetch controllerId).1) := by
            simp [fetchStep, completeFetch, hlookup']
          simpa [hresult] using TransitionTrace.single hstep

theorem fetchTaskStep_refines
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    ∃ actions,
      TransitionTrace
        fetchStep
        fetch
        actions
        (fetchTaskStep fetch message) := by
  simpa [fetchTaskStep] using handleFetchTaskMessagePure_refines fetch message

theorem fetchTaskExec_refines
    (fetch : Fetch)
    (messages : List FetchTaskMessage) :
    ∃ actions,
      TransitionTrace
        fetchStep
        fetch
        actions
        (fetchTaskExec fetch messages) := by
  induction messages generalizing fetch with
  | nil =>
      refine ⟨[], ?_⟩
      simp [fetchTaskExec, TransitionTrace.nil]
  | cons message messages ih =>
      have hstep := fetchTaskStep_refines fetch message
      have htail := ih (fetchTaskStep fetch message)
      rcases hstep with ⟨actions₁, htrace₁⟩
      rcases htail with ⟨actions₂, htrace₂⟩
      refine ⟨actions₁ ++ actions₂, ?_⟩
      simpa [fetchTaskExec] using TransitionTrace.append htrace₁ htrace₂

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
    TransitionTrace
      fetchStep
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
  · simp [fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure,
      conceptNavigationFetch, conceptFetch, FetchTaskResult.state]
  · simp [handleFetchTaskMessagePure, conceptNavigationFetch, conceptFetch, FetchTaskResult.toSpawnFetchTasks]
  · refine TransitionTrace.single ?_
    simpa [fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure, FetchTaskResult.state] using
      (show
        fetchStep (default : Fetch) (.startFetch pendingFetchRequest) =
          some ((conceptNavigationFetch (default : Fetch) pendingFetchRequest).1) by
            simp [fetchStep, conceptNavigationFetch, conceptFetch])
  · simp [Fetch.pendingFetch?, fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure,
      conceptNavigationFetch, conceptFetch, FetchTaskResult.state]

end FormalWeb
