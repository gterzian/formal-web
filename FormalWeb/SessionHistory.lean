import FormalWeb.Document

namespace FormalWeb

/-- Model-local representation of https://html.spec.whatwg.org/multipage/#post-resource -/
structure PostResource where
  /-- https://html.spec.whatwg.org/multipage/#post-resource-request-body -/
  requestBody : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#post-resource-request-content-type -/
  requestContentType : String := "application/x-www-form-urlencoded"
deriving Repr, DecidableEq

/-- Model-local wrapper for the string branch of https://html.spec.whatwg.org/multipage/#document-state-resource -/
structure SrcdocResource where
  /-- https://html.spec.whatwg.org/multipage/#document-state-resource -/
  source : String
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document-state-resource -/
inductive DocumentResource
  | srcdoc (resource : SrcdocResource)
  | post (resource : PostResource)
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#document-state-2 -/
structure DocumentState where
  /-- https://html.spec.whatwg.org/multipage/#document-state-request-referrer-policy -/
  requestReferrerPolicy : String := ""
  /-- https://html.spec.whatwg.org/multipage/#document-state-initiator-origin -/
  initiatorOrigin : Option Origin := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-resource -/
  resource : Option DocumentResource := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-nav-target-name -/
  navigableTargetName : String := ""
  /-- https://html.spec.whatwg.org/multipage/#document-state-document -/
  document : Option Document := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-origin -/
  origin : Option Origin := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-about-base-url -/
  aboutBaseURL : Option String := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-history-policy-container -/
  historyPolicyContainer : Option PolicyContainer := none
  /-- https://html.spec.whatwg.org/multipage/#document-state-request-referrer -/
  requestReferrer : String := "client"
  /-- https://html.spec.whatwg.org/multipage/#document-state-ever-populated -/
  everPopulated : Bool := false
  /-- https://html.spec.whatwg.org/multipage/#document-state-reload-pending -/
  reloadPending : Bool := false
deriving Repr, DecidableEq

/-- https://html.spec.whatwg.org/multipage/#session-history-entry -/
structure SessionHistoryEntry where
  /-- https://html.spec.whatwg.org/multipage/#she-url -/
  url : String
  /-- https://html.spec.whatwg.org/multipage/#she-document-state -/
  documentState : DocumentState
  /-- https://html.spec.whatwg.org/multipage/#she-step -/
  step : Nat := 0
deriving Repr, DecidableEq

def hasUsablePostResource
    (documentResource : DocumentResource) :
    Bool :=
  match documentResource with
  | .post postResource => postResource.requestBody.isSome
  | _ => false

end FormalWeb
