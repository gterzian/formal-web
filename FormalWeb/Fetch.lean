import Std.Data.TreeMap
import FormalWeb.Navigation

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
  let fetch := {
    fetch with
      pendingFetches := fetch.pendingFetches.erase controllerId
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

end FormalWeb
