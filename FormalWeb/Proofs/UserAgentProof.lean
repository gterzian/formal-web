import FormalWeb.UserAgent
import FormalWeb.Proofs.TransitionSystem

namespace FormalWeb

/-- LTS-style actions for user-agent task-message handling. -/
inductive UserAgentAction
  | bootstrapTraversable
      (destinationURL : String)
      (traversableId : Nat)
      (dispatch? : Option (Nat × EventLoopTaskMessage))
      (pendingFetchRequest : PendingFetchRequest)
  | bootstrapTraversableError
      (destinationURL : String)
      (traversableId : Nat)
      (dispatch? : Option (Nat × EventLoopTaskMessage))
      (errorMessage : String)
  | navigateRequest
    (sourceDocumentId : Nat)
    (destinationURL : String)
    (targetName : String)
    (userInvolvement : UserNavigationInvolvement)
    (noopener : Bool)
    (effects : List UserAgentEffect)
  | beforeUnloadCompleted
    (documentId : Nat)
    (checkId : Nat)
    (canceled : Bool)
    (effects : List UserAgentEffect)
  | requestDocumentFetch
      (handler : RustNetHandlerPointer)
      (request : DocumentFetchRequest)
  | dispatchEvent
      (traversableId : Nat)
      (event : String)
      (eventLoopId : Nat)
      (message : EventLoopTaskMessage)
  | dispatchEventError
      (event : String)
      (errorMessage : String)
  | deferRenderingOpportunity
      (traversableId : Nat)
  | queueRenderingOpportunity
      (traversableId : Nat)
      (eventLoopId : Nat)
      (message : EventLoopTaskMessage)
  | queueRenderingOpportunityNoDispatch
      (traversableId : Nat)
  | renderingOpportunityError
      (errorMessage : String)
  | updateTheRendering
      (traversableId : Nat)
      (eventLoopId : Nat)
      (documentId : DocumentId)
  | fetchCompletedNavigation
      (fetchId : Nat)
      (response : FetchResponse)
      (navigationId : Nat)
      (traversableId : Nat)
      (dispatch? : Option (Nat × EventLoopTaskMessage))
      (renderingDispatch? : Option (Nat × EventLoopTaskMessage))
  | fetchCompletedDocument
      (fetchId : Nat)
      (response : FetchResponse)
      (pendingDocumentFetch : PendingDocumentFetch)
      (dispatch? : Option (Nat × EventLoopTaskMessage))
deriving Repr, DecidableEq

/-- Relational LTS for user-agent task-message handling. -/
def userAgentLTS : TransitionSystem.LTS UserAgent UserAgentAction where
  init := fun userAgent => userAgent = default
  trans := fun userAgent action userAgent' =>
    match action with
    | .bootstrapTraversable destinationURL traversableId dispatch? pendingFetchRequest =>
        ∃ createdUserAgent traversable,
          createNewTopLevelTraversable userAgent none "" = (createdUserAgent, traversable) ∧
          dispatch? = activeDocumentDispatch? createdUserAgent ∧
          navigate createdUserAgent traversable destinationURL =
            (userAgent', some pendingFetchRequest) ∧
          traversable.id = traversableId
    | .bootstrapTraversableError destinationURL traversableId dispatch? errorMessage =>
        ∃ createdUserAgent traversable,
          createNewTopLevelTraversable userAgent none "" = (createdUserAgent, traversable) ∧
          dispatch? = activeDocumentDispatch? createdUserAgent ∧
          navigate createdUserAgent traversable destinationURL =
            (userAgent', none) ∧
          traversable.id = traversableId ∧
          errorMessage = startupNavigationFailureDetails destinationURL userAgent' traversableId
    | .navigateRequest sourceDocumentId destinationURL targetName userInvolvement noopener effects =>
        userAgent' =
            (runMonadic userAgent
              (.navigateRequested
                sourceDocumentId
                destinationURL
                targetName
                userInvolvement
                noopener)).2 ∧
        effects =
            (runMonadic userAgent
              (.navigateRequested
                sourceDocumentId
                destinationURL
                targetName
                userInvolvement
                noopener)).1.toList
    | .beforeUnloadCompleted documentId checkId canceled effects =>
        userAgent' =
            (runMonadic userAgent
              (.beforeUnloadCompleted documentId checkId canceled)).2 ∧
        effects =
            (runMonadic userAgent
              (.beforeUnloadCompleted documentId checkId canceled)).1.toList
    | .requestDocumentFetch handler request =>
        ∃ pendingDocumentFetch,
          requestDocumentFetch userAgent handler request.request =
            (userAgent', pendingDocumentFetch, request)
    | .dispatchEvent traversableId event eventLoopId message =>
        queueDispatchedEvent userAgent traversableId event = some (userAgent', eventLoopId, message)
    | .dispatchEventError event errorMessage =>
        userAgent' = userAgent ∧
        errorMessage = dispatchEventFailureDetails userAgent event ∧
        (activeTraversableId? userAgent = none ∨
          ∃ traversableId,
            activeTraversableId? userAgent = some traversableId ∧
            queueDispatchedEvent userAgent traversableId event = none)
    | .deferRenderingOpportunity traversableId =>
        ∃ traversable,
          activeTraversable? userAgent = some traversable ∧
          traversable.id = traversableId ∧
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome = true ∧
          userAgent' = notePendingUpdateTheRendering userAgent traversableId
    | .queueRenderingOpportunity traversableId eventLoopId message =>
        ∃ traversable,
          activeTraversable? userAgent = some traversable ∧
          traversable.id = traversableId ∧
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome = false ∧
          traversable.toTraversableNavigable.activeDocument.isNone = false ∧
          queueUpdateTheRendering userAgent traversableId = (userAgent', some (eventLoopId, message))
    | .queueRenderingOpportunityNoDispatch traversableId =>
        ∃ traversable,
          activeTraversable? userAgent = some traversable ∧
          traversable.id = traversableId ∧
          traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome = false ∧
          traversable.toTraversableNavigable.activeDocument.isNone = false ∧
          queueUpdateTheRendering userAgent traversableId = (userAgent', none)
    | .renderingOpportunityError errorMessage =>
        userAgent' = userAgent ∧
        errorMessage = renderingOpportunityFailureDetails userAgent ∧
        (activeTraversable? userAgent = none ∨
          ∃ traversable,
            activeTraversable? userAgent = some traversable ∧
            traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome = false ∧
            traversable.toTraversableNavigable.activeDocument.isNone = true)
    | .updateTheRendering traversableId eventLoopId documentId =>
        completeUpdateTheRendering userAgent traversableId eventLoopId documentId = some userAgent'
    | .fetchCompletedNavigation fetchId response navigationId traversableId dispatch? renderingDispatch? =>
        ∃ pendingNavigationFetch processedUserAgent,
          UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId = some pendingNavigationFetch ∧
          pendingNavigationFetch.navigationId = navigationId ∧
          pendingNavigationFetch.traversableId = traversableId ∧
          processedUserAgent =
            processNavigationFetchResponse
              userAgent
              navigationId
              (navigationResponseOfFetchResponse response) ∧
          dispatch? =
            navigationDocumentDispatch?
              processedUserAgent
              traversableId
              (navigationResponseOfFetchResponse response) ∧
          resumePendingUpdateTheRenderingAfterNavigation processedUserAgent traversableId =
            (userAgent', renderingDispatch?)
    | .fetchCompletedDocument fetchId response pendingDocumentFetch dispatch? =>
        UserAgent.pendingDocumentFetch? userAgent fetchId = some pendingDocumentFetch ∧
        userAgent' = (userAgent.takePendingDocumentFetch fetchId).1 ∧
        dispatch? =
          documentFetchCompletionDispatch?
            (userAgent.takePendingDocumentFetch fetchId).1
            pendingDocumentFetch
            response

def interpretUserAgentAction : UserAgentAction → List UserAgentEffect
  | .bootstrapTraversable destinationURL traversableId dispatch? pendingFetchRequest =>
      [
        .createTopLevelTraversable "" dispatch? true,
        .beginNavigation traversableId destinationURL none (some pendingFetchRequest)
      ]
  | .bootstrapTraversableError destinationURL traversableId dispatch? errorMessage =>
      [
        .createTopLevelTraversable "" dispatch? true,
        .beginNavigation traversableId destinationURL none none,
        .logError errorMessage
      ]
  | .navigateRequest _ _ _ _ _ effects =>
      effects
    | .beforeUnloadCompleted _ _ _ effects =>
      effects
  | .requestDocumentFetch handler request =>
      [.requestDocumentFetch handler request]
  | .dispatchEvent traversableId event eventLoopId message =>
      [.dispatchEvent traversableId eventLoopId event message]
  | .dispatchEventError _ errorMessage =>
      [.logError errorMessage]
  | .deferRenderingOpportunity _ =>
      []
  | .queueRenderingOpportunity traversableId eventLoopId message =>
      [.queueUpdateTheRendering traversableId eventLoopId message]
  | .queueRenderingOpportunityNoDispatch _ =>
      []
  | .renderingOpportunityError errorMessage =>
      [.logError errorMessage]
  | .updateTheRendering traversableId eventLoopId documentId =>
      [.updateTheRendering traversableId eventLoopId documentId (queuePaintDocument eventLoopId documentId)]
  | .fetchCompletedNavigation _ response navigationId traversableId dispatch? renderingDispatch? =>
      [.completeNavigation
        navigationId
        traversableId
        (navigationResponseOfFetchResponse response)
        dispatch?] ++
      match renderingDispatch? with
      | some (eventLoopId, message) =>
          [.queueUpdateTheRendering traversableId eventLoopId message]
      | none =>
          []
  | .fetchCompletedDocument fetchId _ pendingDocumentFetch dispatch? =>
      [.finishDocumentFetch fetchId pendingDocumentFetch dispatch?]

def interpretUserAgentActions (actions : List UserAgentAction) : List UserAgentEffect :=
  actions.flatMap interpretUserAgentAction

def UserAgentTraceRefines
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage)
    (actions : List UserAgentAction) : Prop :=
  TransitionSystem.TransitionTrace
    userAgentLTS
    userAgent
    actions
    (runMonadic userAgent message).2

def UserAgentEffectsRefine
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage)
    (actions : List UserAgentAction) : Prop :=
  interpretUserAgentActions actions = (runMonadic userAgent message).1.toList

private theorem run_bind {α β : Type}
    (state : UserAgent)
    (m : FormalWeb.M α)
    (f : α → FormalWeb.M β) :
    WriterT.run (m >>= f) state =
      let ((a, effects1), nextState) := WriterT.run m state
      let ((b, effects2), finalState) := WriterT.run (f a) nextState
      ((b, effects1 ++ effects2), finalState) := by
  rfl

private theorem run_bind_pure {α β : Type}
    (state : UserAgent)
    (m : FormalWeb.M α)
    (f : α → β) :
    WriterT.run (m >>= fun a => pure (f a)) state =
      let ((a, effects), nextState) := WriterT.run m state
      ((f a, effects), nextState) := by
  rfl

private theorem run_get (state : UserAgent) :
    WriterT.run (get : FormalWeb.M UserAgent) state = ((state, #[]), state) := by
  rfl

private theorem run_set (state nextState : UserAgent) :
    WriterT.run (set nextState : FormalWeb.M PUnit) state = (((), #[]), nextState) := by
  rfl

private theorem run_tell
    (state : UserAgent)
    (effects : Array UserAgentEffect) :
    WriterT.run (tell effects : FormalWeb.M PUnit) state = (((), effects), state) := by
  rfl

private theorem run_pure {α : Type}
    (state : UserAgent)
    (value : α) :
    WriterT.run (pure value : FormalWeb.M α) state = ((value, #[]), state) := by
  rfl

private theorem toList_queueUpdateTheRendering
  (traversableId : Nat)
  (renderingDispatch? : Option (Nat × EventLoopTaskMessage)) :
  (match renderingDispatch? with
    | some (eventLoopId, eventLoopMessage) =>
      #[UserAgentEffect.queueUpdateTheRendering traversableId eventLoopId eventLoopMessage]
    | none =>
      #[]).toList =
    match renderingDispatch? with
    | some (eventLoopId, eventLoopMessage) =>
      [UserAgentEffect.queueUpdateTheRendering traversableId eventLoopId eventLoopMessage]
    | none =>
      [] := by
  cases renderingDispatch? with
  | none =>
    rfl
  | some dispatch =>
    rcases dispatch with ⟨eventLoopId, eventLoopMessage⟩
    rfl

private theorem createTopLevelTraversableM_run
    (userAgent : UserAgent)
    (targetName : String := "") :
    (createTopLevelTraversableM targetName).run userAgent =
      let result := createNewTopLevelTraversable userAgent none targetName
      let nextUserAgent := result.1
      let traversable := result.2
      ((traversable,
          #[UserAgentEffect.createTopLevelTraversable
            targetName
            (activeDocumentDispatch? nextUserAgent)]), nextUserAgent) := by
  unfold createTopLevelTraversableM
  rw [run_bind, run_get]
  simp [run_bind, run_set, run_tell, run_pure, M.createTopLevelTraversable, M.emit]

private theorem navigateM_run
    (userAgent : UserAgent)
    (traversable : TopLevelTraversable)
    (destinationURL : String)
    (documentResource : Option DocumentResource := none) :
    (navigateM traversable destinationURL documentResource).run userAgent =
      let result := navigate userAgent traversable destinationURL documentResource
      let nextUserAgent := result.1
      let pendingFetchRequest? := result.2
      ((pendingFetchRequest?,
          #[UserAgentEffect.beginNavigation
            traversable.id
            destinationURL
            documentResource
            pendingFetchRequest?]), nextUserAgent) := by
  unfold navigateM
  rw [run_bind, run_get]
  simp [run_bind, run_set, run_tell, run_pure, M.beginNavigation, M.emit]

theorem bootstrapFreshTopLevelTraversableM_run
    (userAgent : UserAgent)
    (destinationURL : String) :
    (bootstrapFreshTopLevelTraversableM destinationURL).run userAgent =
      let created := createNewTopLevelTraversable userAgent none ""
      let createdUserAgent := created.1
      let traversable := created.2
      let dispatch? := activeDocumentDispatch? createdUserAgent
      let navigated := navigate createdUserAgent traversable destinationURL
      let nextUserAgent := navigated.1
      let pendingFetchRequest? := navigated.2
      match pendingFetchRequest? with
      | some pendingFetchRequest =>
          (((Except.ok traversable.id), #[
            UserAgentEffect.createTopLevelTraversable "" dispatch? true,
            UserAgentEffect.beginNavigation traversable.id destinationURL none (some pendingFetchRequest)
          ]), nextUserAgent)
      | none =>
          (((Except.error (startupNavigationFailureDetails destinationURL nextUserAgent traversable.id)), #[
            UserAgentEffect.createTopLevelTraversable "" dispatch? true,
            UserAgentEffect.beginNavigation traversable.id destinationURL none none,
            UserAgentEffect.logError
              (startupNavigationFailureDetails destinationURL nextUserAgent traversable.id)
          ]), nextUserAgent) := by
      unfold bootstrapFreshTopLevelTraversableM
      rw [run_bind, createTopLevelTraversableM_run]
      dsimp
      rw [run_bind, navigateM_run]
      dsimp
      cases navigate
        (createNewTopLevelTraversable userAgent none "").1
        (createNewTopLevelTraversable userAgent none "").2
        destinationURL with
      | mk nextUserAgent pendingFetchRequest? =>
        cases pendingFetchRequest? with
        | some pendingFetchRequest =>
          simp [run_pure]
        | none =>
          rw [run_bind, run_get]
          simp
          rw [run_bind_pure]
          simp [M.logError, M.emit, run_tell]

theorem runMonadic_freshTopLevelTraversable
    (userAgent : UserAgent)
    (destinationURL : String) :
    runMonadic userAgent (.freshTopLevelTraversable destinationURL) =
      let created := createNewTopLevelTraversable userAgent none ""
      let createdUserAgent := created.1
      let traversable := created.2
      let dispatch? := activeDocumentDispatch? createdUserAgent
      let navigated := navigate createdUserAgent traversable destinationURL
      let nextUserAgent := navigated.1
      let pendingFetchRequest? := navigated.2
      match pendingFetchRequest? with
      | some pendingFetchRequest =>
          (#[
            UserAgentEffect.createTopLevelTraversable "" dispatch? true,
            UserAgentEffect.beginNavigation traversable.id destinationURL none (some pendingFetchRequest)
          ], nextUserAgent)
      | none =>
          (#[
            UserAgentEffect.createTopLevelTraversable "" dispatch? true,
            UserAgentEffect.beginNavigation traversable.id destinationURL none none,
            UserAgentEffect.logError
              (startupNavigationFailureDetails destinationURL nextUserAgent traversable.id)
          ], nextUserAgent) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [run_bind, bootstrapFreshTopLevelTraversableM_run]
  dsimp
  cases navigate
      (createNewTopLevelTraversable userAgent none "").1
      (createNewTopLevelTraversable userAgent none "").2
      destinationURL with
  | mk nextUserAgent pendingFetchRequest? =>
      cases pendingFetchRequest? with
      | some pendingFetchRequest =>
          simp [run_pure]
      | none =>
          simp [run_pure]

theorem requestDocumentFetchM_run
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    (requestDocumentFetchM handler request).run userAgent =
      (((), #[
        UserAgentEffect.requestDocumentFetch handler
          (requestDocumentFetch userAgent handler request).2.2
      ]), (requestDocumentFetch userAgent handler request).1) := by
  unfold requestDocumentFetchM
  rfl

theorem runMonadic_documentFetchRequested
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    runMonadic userAgent (.documentFetchRequested handler request) =
      (#[
        UserAgentEffect.requestDocumentFetch handler
          (requestDocumentFetch userAgent handler request).2.2
      ], (requestDocumentFetch userAgent handler request).1) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [requestDocumentFetchM_run]

theorem dispatchEventM_run
    (userAgent : UserAgent)
    (event : String) :
    (dispatchEventM event).run userAgent =
      match activeTraversableId? userAgent with
      | none =>
          (((), #[UserAgentEffect.logError (dispatchEventFailureDetails userAgent event)]), userAgent)
      | some traversableId =>
          match queueDispatchedEvent userAgent traversableId event with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              (((), #[
                UserAgentEffect.dispatchEvent traversableId eventLoopId event eventLoopMessage
              ]), nextUserAgent)
          | none =>
              (((), #[UserAgentEffect.logError (dispatchEventFailureDetails userAgent event)]), userAgent) := by
  unfold dispatchEventM
  rw [run_bind, run_get]
  cases hactive : activeTraversableId? userAgent with
  | none =>
      simp [hactive, M.logError, M.emit, run_tell]
  | some traversableId =>
      cases hqueue : queueDispatchedEvent userAgent traversableId event with
      | none =>
          simp [hactive, hqueue, M.logError, M.emit, run_tell]
      | some queued =>
          rcases queued with ⟨nextUserAgent, eventLoopId, eventLoopMessage⟩
          simp [hactive, hqueue, M.dispatchEvent, M.emit, run_bind, run_set, run_tell]

theorem runMonadic_dispatchEvent
    (userAgent : UserAgent)
    (event : String) :
    runMonadic userAgent (.dispatchEvent event) =
      match activeTraversableId? userAgent with
      | none =>
          (#[UserAgentEffect.logError (dispatchEventFailureDetails userAgent event)], userAgent)
      | some traversableId =>
          match queueDispatchedEvent userAgent traversableId event with
          | some (nextUserAgent, eventLoopId, eventLoopMessage) =>
              (#[
                UserAgentEffect.dispatchEvent traversableId eventLoopId event eventLoopMessage
              ], nextUserAgent)
          | none =>
              (#[UserAgentEffect.logError (dispatchEventFailureDetails userAgent event)], userAgent) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [dispatchEventM_run]
  cases hactive : activeTraversableId? userAgent with
  | none =>
      simp
  | some traversableId =>
      cases hqueue : queueDispatchedEvent userAgent traversableId event with
      | none =>
          simp [hqueue]
      | some queued =>
          rcases queued with ⟨nextUserAgent, eventLoopId, eventLoopMessage⟩
          simp [hqueue]

theorem queueRenderingOpportunityM_run
    (userAgent : UserAgent) :
    queueRenderingOpportunityM.run userAgent =
      match activeTraversable? userAgent with
      | none =>
          (((), #[UserAgentEffect.logError (renderingOpportunityFailureDetails userAgent)]), userAgent)
      | some traversable =>
          if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
            (((), #[]), notePendingUpdateTheRendering userAgent traversable.id)
          else if traversable.toTraversableNavigable.activeDocument.isNone then
            (((), #[UserAgentEffect.logError (renderingOpportunityFailureDetails userAgent)]), userAgent)
          else
            let (nextUserAgent, dispatch?) := queueUpdateTheRendering userAgent traversable.id
            match dispatch? with
            | some (eventLoopId, eventLoopMessage) =>
                (((), #[
                  UserAgentEffect.queueUpdateTheRendering traversable.id eventLoopId eventLoopMessage
                ]), nextUserAgent)
            | none =>
                (((), #[]), nextUserAgent) := by
  unfold queueRenderingOpportunityM
  rw [run_bind, run_get]
  cases hactive : activeTraversable? userAgent with
  | none =>
      simp [hactive, M.logError, M.emit, run_tell]
  | some traversable =>
      by_cases hongoing : traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome
      · simp [hactive, hongoing, run_set]
      · by_cases hdocument : traversable.toTraversableNavigable.activeDocument.isNone
        · simp [hactive, hongoing, hdocument, M.logError, M.emit, run_tell]
        · cases hdispatch : (queueUpdateTheRendering userAgent traversable.id).2 with
          | none =>
              simp [hactive, hongoing, hdocument, hdispatch, run_bind, run_set, run_pure]
          | some dispatch =>
              rcases dispatch with ⟨eventLoopId, eventLoopMessage⟩
              simp [hactive, hongoing, hdocument, hdispatch, run_bind, run_set, run_tell,
                M.queueUpdateTheRendering, M.emit]

theorem runMonadic_renderingOpportunity
    (userAgent : UserAgent) :
    runMonadic userAgent .renderingOpportunity =
      match activeTraversable? userAgent with
      | none =>
          (#[UserAgentEffect.logError (renderingOpportunityFailureDetails userAgent)], userAgent)
      | some traversable =>
          if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
            (#[], notePendingUpdateTheRendering userAgent traversable.id)
          else if traversable.toTraversableNavigable.activeDocument.isNone then
            (#[UserAgentEffect.logError (renderingOpportunityFailureDetails userAgent)], userAgent)
          else
            let (nextUserAgent, dispatch?) := queueUpdateTheRendering userAgent traversable.id
            match dispatch? with
            | some (eventLoopId, eventLoopMessage) =>
                (#[
                  UserAgentEffect.queueUpdateTheRendering traversable.id eventLoopId eventLoopMessage
                ], nextUserAgent)
            | none =>
                (#[], nextUserAgent) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [queueRenderingOpportunityM_run]
  cases activeTraversable? userAgent with
  | none =>
      simp
  | some traversable =>
      by_cases hongoing : traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome
      · simp [hongoing]
      · by_cases hdocument : traversable.toTraversableNavigable.activeDocument.isNone
        · simp [hongoing, hdocument]
        · cases hdispatch : (queueUpdateTheRendering userAgent traversable.id).2 with
          | none =>
              simp [hongoing, hdocument, hdispatch]
          | some dispatch =>
              rcases dispatch with ⟨eventLoopId, eventLoopMessage⟩
              simp [hongoing, hdocument, hdispatch]

theorem completeUpdateTheRenderingM_run
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId) :
    (completeUpdateTheRenderingM traversableId eventLoopId documentId).run userAgent =
      match completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
      | some nextUserAgent =>
          (((), #[
            UserAgentEffect.updateTheRendering
              traversableId
              eventLoopId
              documentId
              (queuePaintDocument eventLoopId documentId)
          ]), nextUserAgent)
      | none =>
          (((), #[]), userAgent) := by
  unfold completeUpdateTheRenderingM
  rw [run_bind, run_get]
  cases hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
  | none =>
      simp [hcomplete, run_pure]
  | some nextUserAgent =>
      simp [hcomplete, run_bind, run_set, run_tell, run_pure, M.updateTheRendering, M.emit,
        queuePaintDocument]

theorem runMonadic_updateTheRenderingCompleted
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId) :
    runMonadic userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId) =
      match completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
      | some nextUserAgent =>
          (#[
            UserAgentEffect.updateTheRendering
              traversableId
              eventLoopId
              documentId
              (queuePaintDocument eventLoopId documentId)
          ], nextUserAgent)
      | none =>
          (#[], userAgent) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [completeUpdateTheRenderingM_run]
  cases completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
  | none =>
      simp
  | some nextUserAgent =>
      simp

theorem handleFetchCompletedM_run
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse) :
    (handleFetchCompletedM fetchId response).run userAgent =
      match UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
      | some pendingNavigationFetch =>
          let navigationResponse := navigationResponseOfFetchResponse response
          let processedUserAgent :=
            processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId navigationResponse
          let resumed :=
            resumePendingUpdateTheRenderingAfterNavigation
              processedUserAgent
              pendingNavigationFetch.traversableId
          let nextUserAgent := resumed.1
          let renderingDispatch? := resumed.2
          let dispatch? :=
            navigationDocumentDispatch?
              processedUserAgent
              pendingNavigationFetch.traversableId
              navigationResponse
          (((), #[
            UserAgentEffect.completeNavigation
              pendingNavigationFetch.navigationId
              pendingNavigationFetch.traversableId
              navigationResponse
              dispatch?
          ] ++
            match renderingDispatch? with
            | some (eventLoopId, eventLoopMessage) =>
                #[
                  UserAgentEffect.queueUpdateTheRendering
                    pendingNavigationFetch.traversableId
                    eventLoopId
                    eventLoopMessage
                ]
            | none =>
                #[]), nextUserAgent)
      | none =>
          match UserAgent.pendingDocumentFetch? userAgent fetchId with
          | some pendingDocumentFetch =>
              let nextUserAgent := (userAgent.takePendingDocumentFetch fetchId).1
              let dispatch? :=
                documentFetchCompletionDispatch? nextUserAgent pendingDocumentFetch response
              (((), #[
                UserAgentEffect.finishDocumentFetch fetchId pendingDocumentFetch dispatch?
              ]), nextUserAgent)
          | none =>
              (((), #[]), userAgent) := by
  unfold handleFetchCompletedM
  rw [run_bind, run_get]
  cases hnavigation : UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
  | none =>
      cases hdocument : UserAgent.pendingDocumentFetch? userAgent fetchId with
      | none =>
          simp [hnavigation, hdocument, run_pure]
      | some pendingDocumentFetch =>
          simp [hnavigation, hdocument, run_bind, run_set, run_tell, M.finishDocumentFetch, M.emit]
  | some pendingNavigationFetch =>
      let navigationResponse := navigationResponseOfFetchResponse response
      let processedUserAgent :=
        processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId navigationResponse
      let resumed :=
        resumePendingUpdateTheRenderingAfterNavigation processedUserAgent pendingNavigationFetch.traversableId
      cases hdispatch : resumed.2 with
      | none =>
          simp [hnavigation, navigationResponse, processedUserAgent, resumed, hdispatch,
            run_bind, run_set, run_tell, run_pure, M.completeNavigation, M.emit]
      | some dispatch =>
          rcases dispatch with ⟨eventLoopId, eventLoopMessage⟩
          simp [hnavigation, navigationResponse, processedUserAgent, resumed, hdispatch,
            run_bind, run_set, run_tell, M.completeNavigation, M.queueUpdateTheRendering,
            M.emit]

theorem runMonadic_fetchCompleted
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse) :
    runMonadic userAgent (.fetchCompleted fetchId response) =
      match UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
      | some pendingNavigationFetch =>
          let navigationResponse := navigationResponseOfFetchResponse response
          let processedUserAgent :=
            processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId navigationResponse
          let resumed :=
            resumePendingUpdateTheRenderingAfterNavigation
              processedUserAgent
              pendingNavigationFetch.traversableId
          let nextUserAgent := resumed.1
          let renderingDispatch? := resumed.2
          let dispatch? :=
            navigationDocumentDispatch?
              processedUserAgent
              pendingNavigationFetch.traversableId
              navigationResponse
          (#[
            UserAgentEffect.completeNavigation
              pendingNavigationFetch.navigationId
              pendingNavigationFetch.traversableId
              navigationResponse
              dispatch?
          ] ++
            match renderingDispatch? with
            | some (eventLoopId, eventLoopMessage) =>
                #[
                  UserAgentEffect.queueUpdateTheRendering
                    pendingNavigationFetch.traversableId
                    eventLoopId
                    eventLoopMessage
                ]
            | none =>
                #[], nextUserAgent)
      | none =>
          match UserAgent.pendingDocumentFetch? userAgent fetchId with
          | some pendingDocumentFetch =>
              let nextUserAgent := (userAgent.takePendingDocumentFetch fetchId).1
              let dispatch? :=
                documentFetchCompletionDispatch? nextUserAgent pendingDocumentFetch response
              (#[
                UserAgentEffect.finishDocumentFetch fetchId pendingDocumentFetch dispatch?
              ], nextUserAgent)
          | none =>
              (#[], userAgent) := by
  unfold runMonadic handleUserAgentTaskMessage
  rw [handleFetchCompletedM_run]
  cases UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
  | none =>
      cases UserAgent.pendingDocumentFetch? userAgent fetchId with
      | none =>
          simp
      | some pendingDocumentFetch =>
          simp
  | some pendingNavigationFetch =>
      let navigationResponse := navigationResponseOfFetchResponse response
      let processedUserAgent :=
        processNavigationFetchResponse userAgent pendingNavigationFetch.navigationId navigationResponse
      let resumed :=
        resumePendingUpdateTheRenderingAfterNavigation processedUserAgent pendingNavigationFetch.traversableId
      cases hdispatch : resumed.2 with
      | none =>
          simp [navigationResponse, processedUserAgent, resumed, hdispatch]
      | some dispatch =>
          rcases dispatch with ⟨eventLoopId, eventLoopMessage⟩
          simp [navigationResponse, processedUserAgent, resumed, hdispatch]

set_option maxHeartbeats 3000000 in
theorem handleFreshTopLevelTraversable_refinement
    (userAgent : UserAgent)
    (destinationURL : String) :
    ∃ actions,
      UserAgentTraceRefines userAgent (.freshTopLevelTraversable destinationURL) actions ∧
      UserAgentEffectsRefine userAgent (.freshTopLevelTraversable destinationURL) actions := by
  let created := createNewTopLevelTraversable userAgent none ""
  let createdUserAgent := created.1
  let traversable := created.2
  let dispatch? := activeDocumentDispatch? createdUserAgent
  let navigated := navigate createdUserAgent traversable destinationURL
  let nextUserAgent := navigated.1
  let pendingFetchRequest? := navigated.2
  cases hpending : pendingFetchRequest? with
  | some pendingFetchRequest =>
      refine ⟨[.bootstrapTraversable destinationURL traversable.id dispatch? pendingFetchRequest], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.bootstrapTraversable destinationURL traversable.id dispatch? pendingFetchRequest)
              nextUserAgent := by
          have hnavigated :
                  navigate createdUserAgent traversable destinationURL =
                (nextUserAgent, pendingFetchRequest?) := by
            rfl
          refine ⟨createdUserAgent, traversable, rfl, rfl, ?_, rfl⟩
          simpa [hpending] using hnavigated
        have hstate :
            (runMonadic userAgent (.freshTopLevelTraversable destinationURL)).2 = nextUserAgent := by
          simpa [created, createdUserAgent, traversable, dispatch?, navigated,
            nextUserAgent, pendingFetchRequest?, hpending] using
            congrArg Prod.snd (runMonadic_freshTopLevelTraversable userAgent destinationURL)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction, created, createdUserAgent,
          traversable, dispatch?, navigated, nextUserAgent, pendingFetchRequest?, hpending] using
          (congrArg (fun result => result.1.toList)
            (runMonadic_freshTopLevelTraversable userAgent destinationURL)).symm
  | none =>
      let errorMessage := startupNavigationFailureDetails destinationURL nextUserAgent traversable.id
      refine ⟨[.bootstrapTraversableError destinationURL traversable.id dispatch? errorMessage], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.bootstrapTraversableError destinationURL traversable.id dispatch? errorMessage)
              nextUserAgent := by
          have hnavigated :
                  navigate createdUserAgent traversable destinationURL =
                (nextUserAgent, pendingFetchRequest?) := by
            rfl
          refine ⟨createdUserAgent, traversable, rfl, rfl, ?_, rfl, rfl⟩
          simpa [hpending] using hnavigated
        have hstate :
            (runMonadic userAgent (.freshTopLevelTraversable destinationURL)).2 = nextUserAgent := by
          simpa [created, createdUserAgent, traversable, dispatch?, navigated,
            nextUserAgent, pendingFetchRequest?, hpending] using
            congrArg Prod.snd (runMonadic_freshTopLevelTraversable userAgent destinationURL)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction, created, createdUserAgent,
          traversable, dispatch?, navigated, nextUserAgent, pendingFetchRequest?, hpending,
          errorMessage] using
          (congrArg (fun result => result.1.toList)
            (runMonadic_freshTopLevelTraversable userAgent destinationURL)).symm

theorem handleDocumentFetchRequested_refinement
    (userAgent : UserAgent)
    (handler : RustNetHandlerPointer)
    (request : NavigationRequest) :
    ∃ actions,
      UserAgentTraceRefines userAgent (.documentFetchRequested handler request) actions ∧
      UserAgentEffectsRefine userAgent (.documentFetchRequested handler request) actions := by
  let result := requestDocumentFetch userAgent handler request
  let pendingDocumentFetch := result.2.1
  let nextUserAgent := result.1
  let documentFetchRequest := result.2.2
  refine ⟨[.requestDocumentFetch handler documentFetchRequest], ?_, ?_⟩
  · unfold UserAgentTraceRefines
    have htrans :
        userAgentLTS.trans
          userAgent
          (.requestDocumentFetch handler documentFetchRequest)
          nextUserAgent := by
      refine ⟨pendingDocumentFetch, ?_⟩
      simp [requestDocumentFetch, result, pendingDocumentFetch, nextUserAgent,
        documentFetchRequest]
    have hstate :
        (runMonadic userAgent (.documentFetchRequested handler request)).2 = nextUserAgent := by
      simpa [result, nextUserAgent, documentFetchRequest] using
        congrArg Prod.snd (runMonadic_documentFetchRequested userAgent handler request)
    simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
  · unfold UserAgentEffectsRefine
    simpa [interpretUserAgentActions, interpretUserAgentAction,
      result, nextUserAgent, documentFetchRequest] using
      (congrArg (fun output => output.1.toList)
        (runMonadic_documentFetchRequested userAgent handler request)).symm

theorem handleNavigateRequested_refinement
    (userAgent : UserAgent)
    (sourceDocumentId : Nat)
    (destinationURL : String)
    (targetName : String)
    (userInvolvement : UserNavigationInvolvement)
    (noopener : Bool) :
    ∃ actions,
      UserAgentTraceRefines
        userAgent
        (.navigateRequested sourceDocumentId destinationURL targetName userInvolvement noopener)
        actions ∧
      UserAgentEffectsRefine
        userAgent
        (.navigateRequested sourceDocumentId destinationURL targetName userInvolvement noopener)
        actions := by
  let effects :=
    (runMonadic
      userAgent
      (.navigateRequested sourceDocumentId destinationURL targetName userInvolvement noopener)).1.toList
  refine ⟨[.navigateRequest sourceDocumentId destinationURL targetName userInvolvement noopener effects], ?_, ?_⟩
  · unfold UserAgentTraceRefines
    apply TransitionSystem.TransitionTrace.single
    constructor <;> rfl
  · unfold UserAgentEffectsRefine
    simp [interpretUserAgentActions, interpretUserAgentAction, effects]

theorem handleBeforeUnloadCompleted_refinement
    (userAgent : UserAgent)
    (documentId : Nat)
    (checkId : Nat)
    (canceled : Bool) :
    ∃ actions,
      UserAgentTraceRefines
        userAgent
        (.beforeUnloadCompleted documentId checkId canceled)
        actions ∧
      UserAgentEffectsRefine
        userAgent
        (.beforeUnloadCompleted documentId checkId canceled)
        actions := by
  let effects :=
    (runMonadic userAgent (.beforeUnloadCompleted documentId checkId canceled)).1.toList
  refine ⟨[.beforeUnloadCompleted documentId checkId canceled effects], ?_, ?_⟩
  · unfold UserAgentTraceRefines
    apply TransitionSystem.TransitionTrace.single
    constructor <;> rfl
  · unfold UserAgentEffectsRefine
    simp [interpretUserAgentActions, interpretUserAgentAction, effects]

theorem handleDispatchEvent_refinement
    (userAgent : UserAgent)
    (event : String) :
    ∃ actions,
      UserAgentTraceRefines userAgent (.dispatchEvent event) actions ∧
      UserAgentEffectsRefine userAgent (.dispatchEvent event) actions := by
  cases hactive : activeTraversableId? userAgent with
  | none =>
      let errorMessage := dispatchEventFailureDetails userAgent event
      refine ⟨[.dispatchEventError event errorMessage], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.dispatchEventError event errorMessage)
              userAgent := by
          exact ⟨rfl, rfl, Or.inl hactive⟩
        have hstate : (runMonadic userAgent (.dispatchEvent event)).2 = userAgent := by
          simpa [hactive] using congrArg Prod.snd (runMonadic_dispatchEvent userAgent event)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction, hactive, errorMessage] using
          (congrArg (fun output => output.1.toList)
            (runMonadic_dispatchEvent userAgent event)).symm
  | some traversableId =>
      cases hqueue : queueDispatchedEvent userAgent traversableId event with
      | some queued =>
          let nextUserAgent := queued.1
          let eventLoopId := queued.2.1
          let message := queued.2.2
          refine ⟨[.dispatchEvent traversableId event eventLoopId message], ?_, ?_⟩
          · unfold UserAgentTraceRefines
            have htrans :
                userAgentLTS.trans
                  userAgent
                  (.dispatchEvent traversableId event eventLoopId message)
                  nextUserAgent := by
              have hqueued' :
                  queueDispatchedEvent userAgent traversableId event =
                    some (nextUserAgent, eventLoopId, message) := by
                calc
                  queueDispatchedEvent userAgent traversableId event = some queued := hqueue
                  _ = some (nextUserAgent, eventLoopId, message) := by
                    simp [nextUserAgent, eventLoopId, message]
              simpa using hqueued'
            have hstate :
                (runMonadic userAgent (.dispatchEvent event)).2 = nextUserAgent := by
              simpa [hactive, hqueue, nextUserAgent, eventLoopId, message] using
                congrArg Prod.snd (runMonadic_dispatchEvent userAgent event)
            simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
          · unfold UserAgentEffectsRefine
            simpa [interpretUserAgentActions, interpretUserAgentAction,
              hactive, hqueue, nextUserAgent, eventLoopId, message] using
              (congrArg (fun output => output.1.toList)
                (runMonadic_dispatchEvent userAgent event)).symm
      | none =>
          let errorMessage := dispatchEventFailureDetails userAgent event
          refine ⟨[.dispatchEventError event errorMessage], ?_, ?_⟩
          · unfold UserAgentTraceRefines
            have htrans :
                userAgentLTS.trans
                  userAgent
                  (.dispatchEventError event errorMessage)
                  userAgent := by
              exact ⟨rfl, rfl, Or.inr ⟨traversableId, hactive, hqueue⟩⟩
            have hstate : (runMonadic userAgent (.dispatchEvent event)).2 = userAgent := by
              simpa [hactive, hqueue] using congrArg Prod.snd (runMonadic_dispatchEvent userAgent event)
            simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
          · unfold UserAgentEffectsRefine
            simpa [interpretUserAgentActions, interpretUserAgentAction,
              hactive, hqueue, errorMessage] using
              (congrArg (fun output => output.1.toList)
                (runMonadic_dispatchEvent userAgent event)).symm

theorem handleRenderingOpportunity_refinement
    (userAgent : UserAgent) :
    ∃ actions,
      UserAgentTraceRefines userAgent .renderingOpportunity actions ∧
      UserAgentEffectsRefine userAgent .renderingOpportunity actions := by
  cases hactive : activeTraversable? userAgent with
  | none =>
      let errorMessage := renderingOpportunityFailureDetails userAgent
      refine ⟨[.renderingOpportunityError errorMessage], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.renderingOpportunityError errorMessage)
              userAgent := by
          exact ⟨rfl, rfl, Or.inl hactive⟩
        have hstate : (runMonadic userAgent .renderingOpportunity).2 = userAgent := by
          simpa [hactive] using congrArg Prod.snd (runMonadic_renderingOpportunity userAgent)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction, hactive, errorMessage] using
          (congrArg (fun output => output.1.toList)
            (runMonadic_renderingOpportunity userAgent)).symm
  | some traversable =>
      by_cases hongoing : traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome
      · refine ⟨[.deferRenderingOpportunity traversable.id], ?_, ?_⟩
        · unfold UserAgentTraceRefines
          have htrans :
              userAgentLTS.trans
                userAgent
                (.deferRenderingOpportunity traversable.id)
                (notePendingUpdateTheRendering userAgent traversable.id) := by
            exact ⟨traversable, hactive, rfl, by simp [hongoing], rfl⟩
          have hstate :
              (runMonadic userAgent .renderingOpportunity).2 =
                notePendingUpdateTheRendering userAgent traversable.id := by
            simpa [hactive, hongoing] using
              congrArg Prod.snd (runMonadic_renderingOpportunity userAgent)
          simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
        · unfold UserAgentEffectsRefine
          simpa [interpretUserAgentActions, interpretUserAgentAction, hactive, hongoing] using
            (congrArg (fun output => output.1.toList)
              (runMonadic_renderingOpportunity userAgent)).symm
      · by_cases hdocument : traversable.toTraversableNavigable.activeDocument.isNone
        · let errorMessage := renderingOpportunityFailureDetails userAgent
          refine ⟨[.renderingOpportunityError errorMessage], ?_, ?_⟩
          · unfold UserAgentTraceRefines
            have htrans :
                userAgentLTS.trans
                  userAgent
                  (.renderingOpportunityError errorMessage)
                  userAgent := by
              exact ⟨rfl, rfl, Or.inr ⟨traversable, hactive, by simp [hongoing], by simp [hdocument]⟩⟩
            have hstate : (runMonadic userAgent .renderingOpportunity).2 = userAgent := by
              simpa [hactive, hongoing, hdocument] using
                congrArg Prod.snd (runMonadic_renderingOpportunity userAgent)
            simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
          · unfold UserAgentEffectsRefine
            simpa [interpretUserAgentActions, interpretUserAgentAction,
              hactive, hongoing, hdocument, errorMessage] using
              (congrArg (fun output => output.1.toList)
                (runMonadic_renderingOpportunity userAgent)).symm
        · let queued := queueUpdateTheRendering userAgent traversable.id
          let nextUserAgent := queued.1
          let dispatch? := queued.2
          cases hdispatch : dispatch? with
          | some dispatch =>
              let eventLoopId := dispatch.1
              let message := dispatch.2
              refine ⟨[.queueRenderingOpportunity traversable.id eventLoopId message], ?_, ?_⟩
              · unfold UserAgentTraceRefines
                have htrans :
                    userAgentLTS.trans
                      userAgent
                      (.queueRenderingOpportunity traversable.id eventLoopId message)
                      nextUserAgent := by
                  refine ⟨traversable, hactive, rfl, by simp [hongoing], by simp [hdocument], ?_⟩
                  calc
                    queueUpdateTheRendering userAgent traversable.id = (nextUserAgent, dispatch?) := by
                      rfl
                    _ = (nextUserAgent, some (eventLoopId, message)) := by
                      simp [hdispatch, eventLoopId, message]
                have hstate :
                    (runMonadic userAgent .renderingOpportunity).2 = nextUserAgent := by
                  simpa [hactive, hongoing, hdocument, queued, nextUserAgent, dispatch?,
                    hdispatch, eventLoopId, message] using
                    congrArg Prod.snd (runMonadic_renderingOpportunity userAgent)
                simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
              · unfold UserAgentEffectsRefine
                simpa [interpretUserAgentActions, interpretUserAgentAction,
                  hactive, hongoing, hdocument, queued, nextUserAgent, dispatch?, hdispatch,
                  eventLoopId, message] using
                  (congrArg (fun output => output.1.toList)
                    (runMonadic_renderingOpportunity userAgent)).symm
          | none =>
              refine ⟨[.queueRenderingOpportunityNoDispatch traversable.id], ?_, ?_⟩
              · unfold UserAgentTraceRefines
                have htrans :
                    userAgentLTS.trans
                      userAgent
                      (.queueRenderingOpportunityNoDispatch traversable.id)
                      nextUserAgent := by
                  refine ⟨traversable, hactive, rfl, by simp [hongoing], by simp [hdocument], ?_⟩
                  calc
                    queueUpdateTheRendering userAgent traversable.id = (nextUserAgent, dispatch?) := by
                      rfl
                    _ = (nextUserAgent, none) := by
                      simp [hdispatch]
                have hstate :
                    (runMonadic userAgent .renderingOpportunity).2 = nextUserAgent := by
                  simpa [hactive, hongoing, hdocument, queued, nextUserAgent, dispatch?,
                    hdispatch] using congrArg Prod.snd (runMonadic_renderingOpportunity userAgent)
                simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
              · unfold UserAgentEffectsRefine
                simpa [interpretUserAgentActions, interpretUserAgentAction,
                  hactive, hongoing, hdocument, queued, nextUserAgent, dispatch?, hdispatch] using
                  (congrArg (fun output => output.1.toList)
                    (runMonadic_renderingOpportunity userAgent)).symm

theorem handleUpdateTheRenderingCompleted_refinement
    (userAgent : UserAgent)
    (traversableId eventLoopId : Nat)
    (documentId : DocumentId) :
    ∃ actions,
      UserAgentTraceRefines userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId) actions ∧
      UserAgentEffectsRefine userAgent (.updateTheRenderingCompleted traversableId eventLoopId documentId) actions := by
  cases hcomplete : completeUpdateTheRendering userAgent traversableId eventLoopId documentId with
  | some nextUserAgent =>
      refine ⟨[.updateTheRendering traversableId eventLoopId documentId], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.updateTheRendering traversableId eventLoopId documentId)
              nextUserAgent := by
          simpa [userAgentLTS] using hcomplete
        have hstate :
            (runMonadic userAgent
              (.updateTheRenderingCompleted traversableId eventLoopId documentId)).2 =
              nextUserAgent := by
          simpa [hcomplete] using
            congrArg Prod.snd
              (runMonadic_updateTheRenderingCompleted userAgent traversableId eventLoopId documentId)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction, hcomplete, queuePaintDocument] using
          (congrArg (fun output => output.1.toList)
            (runMonadic_updateTheRenderingCompleted userAgent traversableId eventLoopId documentId)).symm
  | none =>
      refine ⟨[], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have hstate :
            (runMonadic userAgent
              (.updateTheRenderingCompleted traversableId eventLoopId documentId)).2 =
              userAgent := by
          simpa [hcomplete] using
            congrArg Prod.snd
              (runMonadic_updateTheRenderingCompleted userAgent traversableId eventLoopId documentId)
        simpa [hstate] using (TransitionSystem.TransitionTrace.nil userAgent)
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, hcomplete] using
          (congrArg (fun output => output.1.toList)
            (runMonadic_updateTheRenderingCompleted userAgent traversableId eventLoopId documentId)).symm

theorem handleFetchCompleted_refinement
    (userAgent : UserAgent)
    (fetchId : Nat)
    (response : FetchResponse) :
    ∃ actions,
      UserAgentTraceRefines userAgent (.fetchCompleted fetchId response) actions ∧
      UserAgentEffectsRefine userAgent (.fetchCompleted fetchId response) actions := by
  cases hnavigation : UserAgent.pendingNavigationFetchByFetchId? userAgent fetchId with
  | some pendingNavigationFetch =>
      let navigationId := pendingNavigationFetch.navigationId
      let traversableId := pendingNavigationFetch.traversableId
      let navigationResponse := navigationResponseOfFetchResponse response
      let processedUserAgent :=
        processNavigationFetchResponse userAgent navigationId navigationResponse
      let dispatch? :=
        navigationDocumentDispatch? processedUserAgent traversableId navigationResponse
      let resumed := resumePendingUpdateTheRenderingAfterNavigation processedUserAgent traversableId
      let nextUserAgent := resumed.1
      let renderingDispatch? := resumed.2
      refine ⟨[.fetchCompletedNavigation
        fetchId
        response
        navigationId
        traversableId
        dispatch?
        renderingDispatch?], ?_, ?_⟩
      · unfold UserAgentTraceRefines
        have htrans :
            userAgentLTS.trans
              userAgent
              (.fetchCompletedNavigation
                fetchId
                response
                navigationId
                traversableId
                dispatch?
                renderingDispatch?)
              nextUserAgent := by
          refine ⟨pendingNavigationFetch, processedUserAgent, hnavigation, rfl, rfl, rfl, rfl, rfl⟩
        have hstate :
            (runMonadic userAgent (.fetchCompleted fetchId response)).2 = nextUserAgent := by
          simpa [hnavigation, navigationId, traversableId, navigationResponse,
            processedUserAgent, dispatch?, resumed, nextUserAgent, renderingDispatch?] using
            congrArg Prod.snd (runMonadic_fetchCompleted userAgent fetchId response)
        simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
      · unfold UserAgentEffectsRefine
        simpa [interpretUserAgentActions, interpretUserAgentAction,
          hnavigation, navigationId, traversableId, navigationResponse,
          processedUserAgent, dispatch?, resumed, nextUserAgent, renderingDispatch?,
          toList_queueUpdateTheRendering] using
          (congrArg (fun output => output.1.toList)
            (runMonadic_fetchCompleted userAgent fetchId response)).symm
  | none =>
      cases hdocument : UserAgent.pendingDocumentFetch? userAgent fetchId with
      | some pendingDocumentFetch =>
          let nextUserAgent := (userAgent.takePendingDocumentFetch fetchId).1
          let dispatch? :=
            documentFetchCompletionDispatch? nextUserAgent pendingDocumentFetch response
          refine ⟨[.fetchCompletedDocument fetchId response pendingDocumentFetch dispatch?], ?_, ?_⟩
          · unfold UserAgentTraceRefines
            have htrans :
                userAgentLTS.trans
                  userAgent
                  (.fetchCompletedDocument fetchId response pendingDocumentFetch dispatch?)
                  nextUserAgent := by
              exact ⟨hdocument, rfl, rfl⟩
            have hstate :
                (runMonadic userAgent (.fetchCompleted fetchId response)).2 = nextUserAgent := by
              simpa [hnavigation, hdocument, nextUserAgent, dispatch?] using
                congrArg Prod.snd (runMonadic_fetchCompleted userAgent fetchId response)
            simpa [hstate] using TransitionSystem.TransitionTrace.single htrans
          · unfold UserAgentEffectsRefine
            simpa [interpretUserAgentActions, interpretUserAgentAction,
              hnavigation, hdocument, nextUserAgent, dispatch?] using
              (congrArg (fun output => output.1.toList)
                (runMonadic_fetchCompleted userAgent fetchId response)).symm
      | none =>
          refine ⟨[], ?_, ?_⟩
          · unfold UserAgentTraceRefines
            have hstate :
                (runMonadic userAgent (.fetchCompleted fetchId response)).2 = userAgent := by
              simpa [hnavigation, hdocument] using
                congrArg Prod.snd (runMonadic_fetchCompleted userAgent fetchId response)
            simpa [hstate] using (TransitionSystem.TransitionTrace.nil userAgent)
          · unfold UserAgentEffectsRefine
            simpa [interpretUserAgentActions, hnavigation, hdocument] using
              (congrArg (fun output => output.1.toList)
                (runMonadic_fetchCompleted userAgent fetchId response)).symm

theorem handleUserAgentTaskMessage_full_refinement
    (userAgent : UserAgent)
    (message : UserAgentTaskMessage) :
    ∃ actions,
      UserAgentTraceRefines userAgent message actions ∧
      UserAgentEffectsRefine userAgent message actions := by
  cases message with
  | freshTopLevelTraversable destinationURL =>
      exact handleFreshTopLevelTraversable_refinement userAgent destinationURL
  | navigateRequested sourceDocumentId destinationURL targetName userInvolvement noopener =>
      exact handleNavigateRequested_refinement
        userAgent
        sourceDocumentId
        destinationURL
        targetName
        userInvolvement
        noopener
  | beforeUnloadCompleted documentId checkId canceled =>
      exact handleBeforeUnloadCompleted_refinement userAgent documentId checkId canceled
  | documentFetchRequested handler request =>
      exact handleDocumentFetchRequested_refinement userAgent handler request
  | dispatchEvent event =>
      exact handleDispatchEvent_refinement userAgent event
  | renderingOpportunity =>
      exact handleRenderingOpportunity_refinement userAgent
  | updateTheRenderingCompleted traversableId eventLoopId documentId =>
      exact handleUpdateTheRenderingCompleted_refinement userAgent traversableId eventLoopId documentId
  | fetchCompleted fetchId response =>
      exact handleFetchCompleted_refinement userAgent fetchId response

end FormalWeb
