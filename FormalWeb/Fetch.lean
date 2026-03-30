import Std.Data.TreeMap
import Std.Sync.Channel
import FormalWeb.Navigation
import FormalWeb.TransitionTrace

namespace FormalWeb

/-- https://fetch.spec.whatwg.org/#fetch-controller -/
structure FetchController where
  /-- Model-local identifier for https://fetch.spec.whatwg.org/#fetch-controller -/
  id : Nat
  /-- https://fetch.spec.whatwg.org/#fetch-controller-state -/
  state : String := "ongoing"
deriving Repr, DecidableEq

/-- Model-local bridge from an HTML navigation wait to https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetchRequest where
  /-- Model-local reference back to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/-- Model-local pending state for a started https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetch where
  /-- Model-local reference back to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
  /-- https://fetch.spec.whatwg.org/#fetch-params-controller -/
  controller : FetchController
deriving Repr, DecidableEq

/-- Model-local top-level state for fetch processing. -/
structure Fetch where
  /-- Model-local allocator state for https://fetch.spec.whatwg.org/#fetch-controller -/
  nextFetchControllerId : Nat := 0
  /-- Model-local map of started fetches keyed by controller identifier. -/
  pendingFetches : Std.TreeMap Nat PendingFetch := Std.TreeMap.empty
deriving Repr

instance : Inhabited Fetch where
  default := {}

namespace Fetch

def pendingFetch?
    (fetch : Fetch)
    (controllerId : Nat) :
    Option PendingFetch :=
  fetch.pendingFetches.get? controllerId

end Fetch

/-- https://fetch.spec.whatwg.org/#fetch-scheme -/
def isFetchScheme (url : String) : Bool :=
  url.startsWith "http://" || url.startsWith "https://" || url.startsWith "file://"

/-- https://fetch.spec.whatwg.org/#concept-fetch -/
def conceptFetch
    (fetch : Fetch)
    (pendingRequest : PendingFetchRequest) :
    Fetch × FetchController :=
  let controller : FetchController := {
    id := fetch.nextFetchControllerId
  }
  let pendingFetch : PendingFetch := {
    navigationId := pendingRequest.navigationId
    request := pendingRequest.request
    controller
  }
  let fetch := {
    fetch with
      nextFetchControllerId := fetch.nextFetchControllerId + 1
      pendingFetches := fetch.pendingFetches.insert controller.id pendingFetch
  }
  (fetch, controller)

/-- Model the point where a pending fetch completes and leaves the fetch set. -/
def completeFetch
    (fetch : Fetch)
    (controllerId : Nat) :
    Fetch × Option PendingFetch :=
  let pendingFetch := fetch.pendingFetches.get? controllerId
  let pendingFetches :=
    match pendingFetch with
    | some _ => fetch.pendingFetches.erase controllerId
    | none => fetch.pendingFetches
  let fetch := {
    fetch with
      pendingFetches
  }
  (fetch, pendingFetch)

/--
LTS-style actions for the current fetch model.
-/
inductive FetchAction
  | startFetch (pendingRequest : PendingFetchRequest)
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
      pure (conceptFetch fetch pendingRequest).1
  | .completeFetch controllerId =>
      let (fetch, pendingFetch?) := completeFetch fetch controllerId
      pendingFetch?.map (fun _ => fetch)

inductive FetchTaskMessage where
  | startFetch (pendingRequest : PendingFetchRequest)
  | finishFetch (controllerId : Nat) (response : NavigationResponse)
deriving Repr, DecidableEq

inductive FetchNotification where
  | fetchCompleted (navigationId : Nat) (response : NavigationResponse)
deriving Repr, DecidableEq

structure SpawnedFetchTask where
  controllerId : Nat
  request : NavigationRequest
deriving Repr, DecidableEq

inductive FetchTaskResult where
  | stateOnly (state : Fetch)
  | notify (state : Fetch) (notifications : List FetchNotification)
  | scheduleFetchTasks (state : Fetch) (toSpawnFetchTasks : List SpawnedFetchTask)
deriving Repr

namespace FetchTaskResult

def state : FetchTaskResult → Fetch
  | .stateOnly state => state
  | .notify state _ => state
  | .scheduleFetchTasks state _ => state

def notifications : FetchTaskResult → List FetchNotification
  | .stateOnly _ => []
  | .notify _ notifications => notifications
  | .scheduleFetchTasks _ _ => []

def toSpawnFetchTasks : FetchTaskResult → List SpawnedFetchTask
  | .stateOnly _ => []
  | .notify _ _ => []
  | .scheduleFetchTasks _ toSpawnFetchTasks => toSpawnFetchTasks

end FetchTaskResult

def handleFetchTaskMessagePure
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    FetchTaskResult :=
  match message with
  | .startFetch pendingRequest =>
      let (fetch, controller) := conceptFetch fetch pendingRequest
      .scheduleFetchTasks fetch [{ controllerId := controller.id, request := pendingRequest.request }]
  | .finishFetch controllerId response =>
      let (fetch, pendingFetch?) := completeFetch fetch controllerId
      match pendingFetch? with
      | none =>
          .stateOnly fetch
      | some pendingFetch =>
          .notify fetch [.fetchCompleted pendingFetch.navigationId response]

def fetchTaskStep
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    Fetch :=
  (handleFetchTaskMessagePure fetch message).state

def fetchTaskExec
    (fetch : Fetch)
    (messages : List FetchTaskMessage) :
    Fetch :=
  messages.foldl fetchTaskStep fetch

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
      simp [handleFetchTaskMessagePure, fetchStep, conceptFetch, FetchTaskResult.state]
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
      (conceptFetch (default : Fetch) pendingFetchRequest).1 ∧
    (FetchTaskResult.toSpawnFetchTasks
      (handleFetchTaskMessagePure (default : Fetch) (.startFetch pendingFetchRequest))) =
      [{
        controllerId := (conceptFetch (default : Fetch) pendingFetchRequest).2.id
        request := pendingFetchRequest.request
      }] ∧
    TransitionTrace
      fetchStep
      (default : Fetch)
      [.startFetch pendingFetchRequest]
      (fetchTaskExec (default : Fetch) [.startFetch pendingFetchRequest]) ∧
    Fetch.pendingFetch?
      (fetchTaskExec (default : Fetch) [.startFetch pendingFetchRequest])
      (conceptFetch (default : Fetch) pendingFetchRequest).2.id =
      some {
        navigationId := pendingFetchRequest.navigationId
        request := pendingFetchRequest.request
        controller := (conceptFetch (default : Fetch) pendingFetchRequest).2
      } := by
  refine ⟨?_, ?_, ?_, ?_⟩
  · simp [fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure, conceptFetch, FetchTaskResult.state]
  · simp [handleFetchTaskMessagePure, conceptFetch, FetchTaskResult.toSpawnFetchTasks]
  · refine TransitionTrace.single ?_
    simpa [fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure, FetchTaskResult.state] using
      (show
        fetchStep (default : Fetch) (.startFetch pendingFetchRequest) =
          some ((conceptFetch (default : Fetch) pendingFetchRequest).1) by
            simp [fetchStep, conceptFetch])
  · simp [Fetch.pendingFetch?, fetchTaskExec, fetchTaskStep, handleFetchTaskMessagePure, conceptFetch, FetchTaskResult.state]

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

private def spawnDetached (action : IO Unit) : IO Unit := do
  let _ ← IO.asTask action
  pure ()

def fetchResponseForRequest
    (request : NavigationRequest) :
    IO NavigationResponse := do
  let output ← IO.Process.output {
    cmd := "curl"
    args := #["-L", "--silent", "--show-error", request.url]
  }
  if output.exitCode == 0 then
    pure {
      url := request.url
      body := output.stdout
    }
  else
    pure {
      url := request.url
      status := 599
      body :=
        s!"<!DOCTYPE html><html><head><title>Fetch failed</title></head><body><pre>{output.stderr}</pre></body></html>"
    }

private def spawnFetchRequestTask
    (channel : Std.CloseableChannel FetchTaskMessage)
    (controllerId : Nat)
    (request : NavigationRequest) :
    IO Unit := do
  spawnDetached do
    let response ← fetchResponseForRequest request
    trySendAndForget channel (.finishFetch controllerId response)

def runFetchMessage
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    IO Fetch := do
  let result := handleFetchTaskMessagePure fetch message
  match result with
  | .stateOnly nextFetch =>
      pure nextFetch
  | .notify nextFetch notifications =>
      for notification in notifications do
        onNotification notification
      pure nextFetch
  | .scheduleFetchTasks nextFetch toSpawnFetchTasks =>
      for toSpawnFetchTask in toSpawnFetchTasks do
        spawnFetchRequestTask channel toSpawnFetchTask.controllerId toSpawnFetchTask.request
      pure nextFetch

/-- Process fetch-task messages until the channel is closed. -/
partial def runFetch
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch := default) :
    IO Unit := do
  let nextMessage? ← recvCloseableChannel? channel
  match nextMessage? with
  | none =>
      pure ()
  | some message =>
      let nextFetch ← runFetchMessage channel onNotification fetch message
      runFetch channel onNotification nextFetch

end FormalWeb
