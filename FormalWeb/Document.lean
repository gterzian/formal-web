import Std.Data.TreeMap

namespace FormalWeb

/-- https://html.spec.whatwg.org/multipage/#concept-origin -/
structure Origin where
  /-- https://html.spec.whatwg.org/multipage/#ascii-serialisation-of-an-origin -/
  serialization : String
  /-- Model-local cache of the result of https://html.spec.whatwg.org/multipage/#obtain-a-site -/
  site : String
deriving Repr, DecidableEq, Ord

def aboutBlankOrigin : Origin :=
  { serialization := "about:blank", site := "about:blank" }

/-- Placeholder for the Rust-side DOM object backing a spec-level document. -/
structure RustDocumentHandle where
  /-- Model-local handle for https://dom.spec.whatwg.org/#concept-document -/
  id : Nat
deriving Repr, DecidableEq, Ord

/-- https://html.spec.whatwg.org/multipage/#document-load-timing-info -/
structure DocumentLoadTimingInfo where
  /-- https://html.spec.whatwg.org/multipage/#navigation-start-time -/
  navigationStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-interactive-time -/
  domInteractiveTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-content-loaded-event-start-time -/
  domContentLoadedEventStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-content-loaded-event-end-time -/
  domContentLoadedEventEndTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#dom-complete-time -/
  domCompleteTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#load-event-start-time -/
  loadEventStartTime : Nat := 0
  /-- https://html.spec.whatwg.org/multipage/#load-event-end-time -/
  loadEventEndTime : Nat := 0
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#cross-origin-opener-policy-value -/
inductive OpenerPolicyValue
  | unsafeNone
  | sameOriginAllowPopups
  | sameOrigin
  | noopenerAllowPopups
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#cross-origin-opener-policy -/
structure OpenerPolicy where
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-value -/
  value : OpenerPolicyValue := .unsafeNone
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-endpoint -/
  reportingEndpoint : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-only-value -/
  reportOnlyValue : OpenerPolicyValue := .unsafeNone
  /-- https://html.spec.whatwg.org/multipage/#coop-struct-report-only-endpoint -/
  reportOnlyReportingEndpoint : Option String := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#policy-container -/
structure PolicyContainer where
  /-- https://html.spec.whatwg.org/multipage/#policy-container-csp-list -/
  cspList : List String := []
  /-- https://html.spec.whatwg.org/multipage/#policy-container-embedder-policy -/
  embedderPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-referrer-policy -/
  referrerPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-integrity-policy -/
  integrityPolicy : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#policy-container-report-only-integrity-policy -/
  reportOnlyIntegrityPolicy : Option String := none
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
structure PermissionsPolicy where
  placeholder : Bool := true
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#active-sandboxing-flag-set -/
abbrev SandboxingFlagSet := List String

/-- Placeholder for the created custom element registry. -/
structure CustomElementRegistry where
  initialized : Bool := true
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document -/
structure Document where
  /-- Model-local handle for the host-side DOM object for https://dom.spec.whatwg.org/#concept-document -/
  ffiHandle : RustDocumentHandle
  /-- https://dom.spec.whatwg.org/#concept-document-type -/
  type : String := "html"
  /-- https://dom.spec.whatwg.org/#concept-document-content-type -/
  contentType : String := "text/html"
  /-- https://dom.spec.whatwg.org/#concept-document-mode -/
  mode : String := "quirks"
  /-- https://dom.spec.whatwg.org/#concept-document-origin -/
  origin : Origin
  /-- https://html.spec.whatwg.org/multipage/#concept-document-bc -/
  browsingContextId : Nat
  /-- Model-local reference to the https://html.spec.whatwg.org/multipage/#concept-agent-event-loop used by the document's relevant global object. -/
  eventLoopId : Nat
  /-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
  permissionsPolicy : PermissionsPolicy := {}
  /-- https://html.spec.whatwg.org/multipage/#active-sandboxing-flag-set -/
  activeSandboxingFlagSet : SandboxingFlagSet := []
  /-- https://html.spec.whatwg.org/multipage/#load-timing-info -/
  loadTimingInfo : DocumentLoadTimingInfo := {}
  /-- https://html.spec.whatwg.org/multipage/#is-initial-about:blank -/
  isInitialAboutBlank : Bool := true
  /-- https://html.spec.whatwg.org/multipage/#concept-document-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://dom.spec.whatwg.org/#concept-document-allow-declarative-shadow-roots -/
  allowDeclarativeShadowRoots : Bool := true
  /-- https://dom.spec.whatwg.org/#document-custom-element-registry -/
  customElementRegistry : CustomElementRegistry := {}
  /-- https://html.spec.whatwg.org/multipage/#concept-document-internal-ancestor-origin-objects-list -/
  internalAncestorOriginObjectsList : List Origin := []
  /-- https://html.spec.whatwg.org/multipage/#concept-document-ancestor-origins-list -/
  ancestorOriginsList : Option (List Origin) := none
  /-- https://html.spec.whatwg.org/multipage/#the-document's-referrer -/
  referrer : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#concept-document-policy-container -/
  policyContainer : PolicyContainer := {}
  /-- https://html.spec.whatwg.org/multipage/#concept-document-coop -/
  openerPolicy : OpenerPolicy := {}
  /-- https://dom.spec.whatwg.org/#concept-document-url -/
  url : String := "about:blank"
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#determining-the-origin -/
def determineOrigin
    (_url : String)
    (_sandboxFlags : SandboxingFlagSet)
    (creatorOrigin : Option Origin) :
    Origin :=
  -- TODO: Model the determining the origin algorithm.
  creatorOrigin.getD aboutBlankOrigin

/-- https://html.spec.whatwg.org/multipage/#concept-document-permissions-policy -/
def createPermissionsPolicy
    (_embedder : Option Unit)
    (_origin : Origin) :
    PermissionsPolicy :=
  -- TODO: Model creating a permissions policy.
  {}

/-- https://html.spec.whatwg.org/multipage/#internal-ancestor-origin-objects-list-creation-steps -/
def internalAncestorOriginObjectsListCreationSteps
    (_document : Document)
    (_iframeReferrerPolicy : Option String) :
    List Origin :=
  -- TODO: Model the internal ancestor origin objects list creation steps.
  []

/-- https://html.spec.whatwg.org/multipage/#ancestor-origins-list-creation-steps -/
def ancestorOriginsListCreationSteps
    (_document : Document) :
    List Origin :=
  -- TODO: Model the ancestor origins list creation steps.
  []

end FormalWeb
