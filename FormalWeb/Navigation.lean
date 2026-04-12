import FormalWeb.SessionHistory

namespace FormalWeb

/-- https://w3c.github.io/navigation-timing/#dom-navigationtimingtype -/
inductive NavigationTimingType
  | navigate
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#user-navigation-involvement -/
inductive UserNavigationInvolvement
  | none
  | activation
  | browserUI
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#ongoing-navigation -/
inductive OngoingNavigation
  | navigationId (id : Nat)
  | traversal
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#source-snapshot-params -/
structure SourceSnapshotParams where
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-activation -/
  hasTransientActivation : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-client -/
  fetchClientId : Option Nat := none
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params-policy-container -/
  sourcePolicyContainer : PolicyContainer := {}
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#target-snapshot-params -/
structure TargetSnapshotParams where
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params-sandbox -/
  sandboxingFlags : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params-iframe-referrer-policy -/
  iframeElementReferrerPolicy : Option String := none
deriving Repr, DecidableEq

/-- https://fetch.spec.whatwg.org/#concept-request -/
structure NavigationRequest where
  /-- https://fetch.spec.whatwg.org/#concept-request-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-request-method -/
  method : String := "GET"
  /-- https://fetch.spec.whatwg.org/#concept-request-referrer -/
  referrer : String := "client"
  /-- https://fetch.spec.whatwg.org/#concept-request-referrer-policy -/
  referrerPolicy : String := ""
  /-- https://fetch.spec.whatwg.org/#concept-request-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://fetch.spec.whatwg.org/#concept-request-body -/
  body : Option String := none
deriving Repr, DecidableEq

/-- https://fetch.spec.whatwg.org/#concept-response -/
structure NavigationResponse where
  /-- https://fetch.spec.whatwg.org/#concept-response-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-response-status -/
  status : Nat := 200
  /-- Minimal MIME type surface for loading-a-document dispatch. -/
  contentType : String := "text/html"
  /-- Placeholder response body for the future parser/runtime model. -/
  body : String := ""
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#navigation-params -/
structure NavigationParams where
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  id : Nat
  /-- Model-local reference to the destination traversable. -/
  traversableId : Nat
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-request -/
  request : Option NavigationRequest := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-response -/
  response : NavigationResponse
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-fetch-controller -/
  fetchControllerId : Option Nat := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-origin -/
  origin : Origin
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-sandboxing -/
  finalSandboxingFlagSet : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-iframe-referrer-policy -/
  iframeElementReferrerPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-coop -/
  coop : OpenerPolicy := {}
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-nav-timing-type -/
  navigationTimingType : NavigationTimingType := .navigate
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-user-involvement -/
  userInvolvement : UserNavigationInvolvement := .none
deriving Repr, DecidableEq

/-- Pending fetch-backed navigation paused at the spec's wait-for-response point. -/
structure PendingNavigationFetch where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- Model-local identifier corresponding to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- Model-local reference to https://html.spec.whatwg.org/multipage/#navigation-params-navigable -/
  traversableId : Nat
  /-- https://html.spec.whatwg.org/multipage/#session-history-entry -/
  historyEntry : SessionHistoryEntry
  /-- https://html.spec.whatwg.org/multipage/#source-snapshot-params -/
  sourceSnapshotParams : SourceSnapshotParams
  /-- https://html.spec.whatwg.org/multipage/#target-snapshot-params -/
  targetSnapshotParams : TargetSnapshotParams
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-nav-timing-type -/
  navTimingType : NavigationTimingType := .navigate
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-user-involvement -/
  userInvolvement : UserNavigationInvolvement := .none
  /-- Model-local summary of CSP navigation type from https://html.spec.whatwg.org/multipage/#create-navigation-params-by-fetching -/
  cspNavigationType : String := "other"
  /-- Model-local flag for the POST special-case in https://html.spec.whatwg.org/multipage/#attempt-to-populate-the-history-entry's-document -/
  allowPOST : Bool := false
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/-- Model-local continuation for a navigation paused at https://html.spec.whatwg.org/multipage/#checking-if-unloading-is-canceled. -/
structure PendingBeforeUnloadNavigation where
  /-- Model-local identifier for the queued beforeunload check. -/
  checkId : Nat
  /-- Model-local reference to the document whose relevant global object receives `beforeunload`. -/
  documentId : Nat
  /-- Model-local reference to the target traversable that will navigate if the check continues. -/
  traversableId : Nat
  /-- Destination URL for the deferred navigation. -/
  destinationURL : String
  /-- https://html.spec.whatwg.org/multipage/#navigation-params-user-involvement -/
  userInvolvement : UserNavigationInvolvement := .none
deriving Repr, DecidableEq

end FormalWeb
