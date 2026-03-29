import FormalWeb
import FormalWeb.FFI
import Std.Sync.Channel


inductive FetchTaskMessage where
  | startFetch (pendingRequest : FormalWeb.PendingFetchRequest)
  | finishFetch (controllerId : Nat) (response : FormalWeb.NavigationResponse)


inductive RuntimeMessage where
  | freshTopLevelTraversable (destinationURL : String)
  | renderingOpportunity
  | fetchCompleted (navigationId : Nat) (response : FormalWeb.NavigationResponse)


initialize runtimeMessageChannelRef : IO.Ref (Option (Std.CloseableChannel RuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel RuntimeMessage))
initialize runtimeFetchTaskChannelRef : IO.Ref (Option (Std.CloseableChannel FetchTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel FetchTaskMessage))
initialize runtimeUserAgentRef : IO.Ref FormalWeb.UserAgent ←
  IO.mkRef default
initialize runtimeTraversableIdRef : IO.Ref (Option Nat) ←
  IO.mkRef none

def runtimeMessageOfString? (message : String) : Option RuntimeMessage := do
  let messagePrefix := "FreshTopLevelTraversable|"
  if message.startsWith messagePrefix then
    some (.freshTopLevelTraversable (message.drop messagePrefix.length).toString)
  else
    none

def startupNavigationFailureDetails
    (destinationURL : String)
    (userAgent : FormalWeb.UserAgent)
    (traversableId : Nat) :
    String :=
  let fetchScheme := FormalWeb.isFetchScheme destinationURL
  match FormalWeb.traversable? userAgent traversableId with
  | none =>
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; traversable missing after navigate, destinationURL={destinationURL}, fetchScheme={fetchScheme}"
  | some traversable =>
      let activeDocumentUrl :=
        match traversable.toTraversableNavigable.activeDocument with
        | some document => document.url
        | none => "<none>"
      let ongoingNavigationDescription :=
        match traversable.toTraversableNavigable.toNavigable.ongoingNavigation with
        | none => "none"
        | some (.navigationId navigationId) => s!"navigationId({navigationId})"
        | some .traversal => "traversal"
      let pendingNavigationFetchDescription :=
        match traversable.toTraversableNavigable.toNavigable.ongoingNavigation with
        | some (.navigationId navigationId) =>
            match FormalWeb.UserAgent.pendingNavigationFetch? userAgent navigationId with
            | some pendingNavigationFetch =>
                s!"present(navigationId={pendingNavigationFetch.navigationId}, requestUrl={pendingNavigationFetch.request.url}, method={pendingNavigationFetch.request.method})"
            | none =>
                s!"missing(navigationId={navigationId})"
        | some .traversal => "not-applicable(traversal)"
        | none => "not-applicable(no ongoing navigation)"
      s!"expected startup navigation for traversable {traversableId} to create a pending fetch; destinationURL={destinationURL}, fetchScheme={fetchScheme}, activeDocumentUrl={activeDocumentUrl}, ongoingNavigation={ongoingNavigationDescription}, pendingNavigationFetch={pendingNavigationFetchDescription}"

def bootstrapFreshTopLevelTraversable
    (destinationURL : String)
    (userAgent : FormalWeb.UserAgent) :
    IO (Except String (FormalWeb.UserAgent × Nat × FormalWeb.PendingFetchRequest)) := do
  let (userAgent, traversable) := FormalWeb.createNewTopLevelTraversable userAgent none ""
  let (userAgent, pendingFetchRequest) :=
    FormalWeb.navigateWithPendingFetchRequest userAgent traversable destinationURL
  let some pendingFetchRequest := pendingFetchRequest
    | pure <| .error <| startupNavigationFailureDetails destinationURL userAgent traversable.id
  pure <| .ok (userAgent, traversable.id, pendingFetchRequest)

def startupTraversableReadyHtml?
    (userAgent : FormalWeb.UserAgent)
    (traversableId : Nat) :
    Option String := do
  let traversable <- FormalWeb.traversable? userAgent traversableId
  if traversable.toTraversableNavigable.toNavigable.ongoingNavigation.isSome then
    none
  else
    let document <- traversable.toTraversableNavigable.activeDocument
    pure (FormalWeb.UserAgent.documentHtml userAgent document)

def notifyStartupTraversableReady
    (userAgent : FormalWeb.UserAgent)
    (traversableId : Nat) :
    IO Unit := do
  let some _html := startupTraversableReadyHtml? userAgent traversableId | pure ()
  FormalWeb.sendRuntimeMessage "NewTopLevelTraversable"

def enqueueFetchTaskMessage (message : FetchTaskMessage) : IO Unit := do
  let some channel := (← runtimeFetchTaskChannelRef.get) | pure ()
  let sent ← channel.trySend message
  let _ := sent
  pure ()

def fetchResponseForRequest
    (request : FormalWeb.NavigationRequest) :
    IO FormalWeb.NavigationResponse := do
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

def spawnFetchRequestTask
    (fetchChannel : Std.CloseableChannel FetchTaskMessage)
    (controllerId : Nat)
    (request : FormalWeb.NavigationRequest) :
    IO Unit := do
  let _ <- IO.asTask do
    let response ← fetchResponseForRequest request
    let sent ← fetchChannel.trySend (.finishFetch controllerId response)
    let _ := sent
    pure ()
  pure ()

def handleFetchTaskMessage
    (fetchChannel : Std.CloseableChannel FetchTaskMessage)
    (runtimeChannel : Std.CloseableChannel RuntimeMessage)
    (fetch : FormalWeb.Fetch)
    (message : FetchTaskMessage) :
    IO FormalWeb.Fetch := do
  match message with
  | .startFetch pendingRequest =>
      let (fetch, controller) := FormalWeb.conceptFetch fetch pendingRequest
      spawnFetchRequestTask fetchChannel controller.id pendingRequest.request
      pure fetch
  | .finishFetch controllerId response =>
      let (fetch, pendingFetch?) := FormalWeb.completeFetch fetch controllerId
      let some pendingFetch := pendingFetch? | pure fetch
      let sent ← runtimeChannel.trySend (.fetchCompleted pendingFetch.navigationId response)
      let _ := sent
      pure fetch

partial def fetchTaskLoop
    (fetchChannel : Std.CloseableChannel FetchTaskMessage)
    (runtimeChannel : Std.CloseableChannel RuntimeMessage)
    (fetch : FormalWeb.Fetch := default) :
    IO Unit := do
  let receiveTask ← fetchChannel.recv
  let some message := (← IO.wait receiveTask) | pure ()
  let fetch ← handleFetchTaskMessage fetchChannel runtimeChannel fetch message
  fetchTaskLoop fetchChannel runtimeChannel fetch

def handleRuntimeMessage (message : RuntimeMessage) : IO Unit := do
  match message with
  | .freshTopLevelTraversable destinationURL =>
    let userAgent ← runtimeUserAgentRef.get
    let bootstrapResult ←
      bootstrapFreshTopLevelTraversable destinationURL userAgent
    match bootstrapResult with
    | .ok (userAgent, traversableId, pendingFetchRequest) =>
        runtimeUserAgentRef.set userAgent
        runtimeTraversableIdRef.set (some traversableId)
        enqueueFetchTaskMessage (.startFetch pendingFetchRequest)
    | .error error =>
        IO.eprintln s!"bootstrapFreshTopLevelTraversable failed: {error}"
  | .renderingOpportunity =>
    let some traversableId := (← runtimeTraversableIdRef.get) | pure ()
    let userAgent ← runtimeUserAgentRef.get
    FormalWeb.noteRenderingOpportunity userAgent traversableId
  | .fetchCompleted navigationId response =>
    let userAgent ← runtimeUserAgentRef.get
    let userAgent := FormalWeb.processNavigationFetchResponse userAgent navigationId response
    runtimeUserAgentRef.set userAgent
    let some traversableId := (← runtimeTraversableIdRef.get) | pure ()
    notifyStartupTraversableReady userAgent traversableId

partial def runtimeMessageLoop (channel : Std.CloseableChannel RuntimeMessage) : IO Unit := do
  let receiveTask ← channel.recv
  let some message := (← IO.wait receiveTask) | pure ()
  handleRuntimeMessage message
  runtimeMessageLoop channel

def enqueueRuntimeMessage (message : RuntimeMessage) : IO Unit := do
  let some channel := (← runtimeMessageChannelRef.get) | pure ()
  let sent ← channel.trySend message
  let _ := sent
  pure ()


@[export formal_web_user_agent_note_rendering_opportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  let _ <- IO.asTask <| enqueueRuntimeMessage .renderingOpportunity
  pure ()

@[export formal_web_handle_runtime_message]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some runtimeMessage := runtimeMessageOfString? message | pure ()
  let _ <- IO.asTask <| enqueueRuntimeMessage runtimeMessage
  pure ()

def main : IO Unit := do
  let runtimeMessageChannel ← Std.CloseableChannel.new
  let runtimeFetchTaskChannel ← Std.CloseableChannel.new
  runtimeUserAgentRef.set default
  runtimeTraversableIdRef.set none
  runtimeMessageChannelRef.set (some runtimeMessageChannel)
  runtimeFetchTaskChannelRef.set (some runtimeFetchTaskChannel)
  let worker ← IO.asTask (runtimeMessageLoop runtimeMessageChannel)
  let fetchWorker ← IO.asTask (fetchTaskLoop runtimeFetchTaskChannel runtimeMessageChannel)
  FormalWeb.runWinitEventLoop ()
  runtimeFetchTaskChannelRef.set (none : Option (Std.CloseableChannel FetchTaskMessage))
  runtimeMessageChannelRef.set (none : Option (Std.CloseableChannel RuntimeMessage))
  runtimeFetchTaskChannel.close
  runtimeMessageChannel.close
  let _ ← IO.wait fetchWorker
  let _ ← IO.wait worker
  pure ()
