import FormalWeb.UserAgent
import FormalWeb.Proofs.EventLoopProof
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for the current user-agent navigation model. -/
inductive UserAgentAction
  | createTopLevelTraversable (targetName : String := "")
  | beginNavigation
      (traversableId : Nat)
      (destinationURL : String)
      (documentResource : Option DocumentResource := none)
  | completeNavigation (navigationId : Nat) (response : NavigationResponse)
  | requestDocumentFetch (handler : RustNetHandlerPointer) (request : NavigationRequest)
  | finishDocumentFetch (fetchId : Nat)
  | abortNavigation (traversableId : Nat)
  /-- Models the user agent applying a serialized input event to the active document of a traversable. -/
  | dispatchEvent (traversableId : Nat) (event : String)
  /--
  Models the user agent sending a NavigationFinished user event to the winit app.
  Pre-condition: the traversable has an active document and no ongoing navigation.
  The app responds by calling `request_redraw()` and sending an UpdateTheRendering message.
  -/
  | navigationFinished (traversableId : Nat)
  /--
  Models the user agent receiving an UpdateTheRendering message from the winit app
  and enqueuing an UpdateTheRendering task on the given event loop, deduplicating if
  one is already pending. This can happen at any time, but only if the event loop exists.
  -/
  | queueUpdateTheRendering (traversableId : Nat) (eventLoopId : Nat)
  /--
  Models the event-loop task running to completion for the active document handle.
  Clears `hasPendingUpdateTheRendering` on the event loop. This requires the
  traversable's navigation to have completed.
  -/
  | updateTheRendering (traversableId : Nat) (eventLoopId : Nat) (documentId : DocumentId)
deriving Repr, DecidableEq

/--
Relational LTS for the user-agent navigation model.

This sits above helper algorithms such as `navigate` and
`processNavigationFetchResponse`, which implement the details of each labeled step.
-/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun userAgent => userAgent = default
  trans := fun userAgent action userAgent' =>
    match action with
    | .createTopLevelTraversable targetName =>
        userAgent' = (createNewTopLevelTraversable userAgent none targetName).1
    | .beginNavigation traversableId destinationURL documentResource =>
        ∃ traversable,
          traversable? userAgent traversableId = some traversable ∧
          userAgent' = navigate userAgent traversable destinationURL documentResource
    | .completeNavigation navigationId response =>
        userAgent' = processNavigationFetchResponse userAgent navigationId response
    | .requestDocumentFetch handler request =>
        userAgent' = (requestDocumentFetch userAgent handler request).1
    | .finishDocumentFetch fetchId =>
        ∃ pendingDocumentFetch,
          UserAgent.pendingDocumentFetch? userAgent fetchId = some pendingDocumentFetch ∧
          userAgent' = (userAgent.takePendingDocumentFetch fetchId).1
    | .abortNavigation traversableId =>
        userAgent' = abortNavigation userAgent traversableId
    | .dispatchEvent traversableId event =>
        ∃ eventLoopId message,
          queueDispatchedEvent userAgent traversableId event = some (userAgent', eventLoopId, message)
    | .navigationFinished traversableId =>
        ∃ traversable document,
          traversable? userAgent traversableId = some traversable ∧
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation = none ∧
          traversable.toTraversableNavigable.activeDocument = some document ∧
          userAgent' = userAgent
    | .queueUpdateTheRendering traversableId eventLoopId =>
        ∃ message,
          queueUpdateTheRendering userAgent traversableId = (userAgent', some (eventLoopId, message))
    | .updateTheRendering traversableId eventLoopId documentId =>
        completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some userAgent'

abbrev UserAgentTrace := TransitionSystem.TransitionTrace userAgentLTS

inductive UserAgentTaskMessageActionShape : UserAgentTaskMessage → List UserAgentAction → Prop where
  | freshTopLevelTraversable
      (destinationURL : String)
      (traversableId : Nat) :
      UserAgentTaskMessageActionShape
        (.freshTopLevelTraversable destinationURL)
        [.createTopLevelTraversable "", .beginNavigation traversableId destinationURL none]
  | documentFetchRequested
      (handler : RustNetHandlerPointer)
      (request : NavigationRequest) :
      UserAgentTaskMessageActionShape
        (.documentFetchRequested handler request)
        [.requestDocumentFetch handler request]
  | dispatchEventError
      (event : String) :
      UserAgentTaskMessageActionShape (.dispatchEvent event) []
  | dispatchEvent
      (traversableId : Nat)
      (event : String) :
      UserAgentTaskMessageActionShape (.dispatchEvent event) [.dispatchEvent traversableId event]
  | renderingOpportunityError :
      UserAgentTaskMessageActionShape .renderingOpportunity []
  | renderingOpportunity
      (traversableId eventLoopId : Nat) :
      UserAgentTaskMessageActionShape
        .renderingOpportunity
        [.queueUpdateTheRendering traversableId eventLoopId]
  | updateTheRenderingCompletedError
      (traversableId eventLoopId : Nat)
      (documentId : DocumentId) :
      UserAgentTaskMessageActionShape
        (.updateTheRenderingCompleted traversableId eventLoopId documentId)
        []
  | updateTheRenderingCompleted
      (traversableId eventLoopId : Nat)
      (documentId : DocumentId) :
      UserAgentTaskMessageActionShape
        (.updateTheRenderingCompleted traversableId eventLoopId documentId)
        [.updateTheRendering traversableId eventLoopId documentId]
  | fetchCompletedNavigation
      (fetchId : Nat)
      (pendingNavigationFetch : PendingNavigationFetch)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [.completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response)]
  | fetchCompletedNavigationWithPendingRendering
      (fetchId : Nat)
      (pendingNavigationFetch : PendingNavigationFetch)
      (response : FetchResponse)
      (eventLoopId : Nat) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [
          .completeNavigation pendingNavigationFetch.navigationId (navigationResponseOfFetchResponse response),
          .queueUpdateTheRendering pendingNavigationFetch.traversableId eventLoopId
        ]
  | fetchCompletedDocument
      (fetchId : Nat)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        [.finishDocumentFetch fetchId]
  | fetchCompletedMissing
      (fetchId : Nat)
      (response : FetchResponse) :
      UserAgentTaskMessageActionShape
        (.fetchCompleted fetchId response)
        []

  def interpretUserAgentEffect : UserAgentEffect → List UserAgentAction
    | UserAgentEffect.createTopLevelTraversable targetName _ _ =>
      [.createTopLevelTraversable targetName]
    | UserAgentEffect.beginNavigation traversableId destinationURL documentResource _ =>
      [.beginNavigation traversableId destinationURL documentResource]
    | UserAgentEffect.requestDocumentFetch handler request =>
      [.requestDocumentFetch handler request.request]
    | UserAgentEffect.dispatchEvent traversableId _ event _ =>
      [.dispatchEvent traversableId event]
    | UserAgentEffect.queueUpdateTheRendering traversableId eventLoopId _ =>
      [.queueUpdateTheRendering traversableId eventLoopId]
    | UserAgentEffect.updateTheRendering traversableId eventLoopId documentId _ =>
      [.updateTheRendering traversableId eventLoopId documentId]
    | UserAgentEffect.completeNavigation navigationId _ response _ =>
      [.completeNavigation navigationId response]
    | UserAgentEffect.finishDocumentFetch fetchId _ _ =>
      [.finishDocumentFetch fetchId]
    | UserAgentEffect.logError _ =>
      []

  def interpretUserAgentEffects (effects : Array UserAgentEffect) : List UserAgentAction :=
    effects.toList.flatMap interpretUserAgentEffect

end FormalWeb
