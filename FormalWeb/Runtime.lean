import FormalWeb.FFI
import FormalWeb.EventLoop
import FormalWeb.Fetch
import FormalWeb.UserAgent
import Std.Data.TreeMap
import Std.Sync.Channel

namespace FormalWeb

initialize userAgentMessageChannelRef : IO.Ref (Option (Std.CloseableChannel UserAgentTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel UserAgentTaskMessage))

initialize fetchMessageChannelRef : IO.Ref (Option (Std.CloseableChannel FetchTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel FetchTaskMessage))

initialize eventLoopMessageChannelsRef : IO.Ref (Std.TreeMap Nat (Std.CloseableChannel EventLoopTaskMessage)) ←
  IO.mkRef
    (Std.TreeMap.empty : Std.TreeMap Nat (Std.CloseableChannel EventLoopTaskMessage) compare)

initialize documentEventLoopIdsRef : IO.Ref (Std.TreeMap Nat Nat) ←
  IO.mkRef (Std.TreeMap.empty : Std.TreeMap Nat Nat compare)

initialize documentFetchRecipientsRef : IO.Ref (Std.TreeMap Nat (Nat × RustNetHandlerPointer)) ←
  IO.mkRef
    (Std.TreeMap.empty : Std.TreeMap Nat (Nat × RustNetHandlerPointer) compare)

initialize nextDocumentFetchIdRef : IO.Ref Nat ←
  IO.mkRef 0

initialize eventLoopShutdownChannelsRef : IO.Ref (List (Std.CloseableChannel EventLoopTaskMessage)) ←
  IO.mkRef ([] : List (Std.CloseableChannel EventLoopTaskMessage))

initialize eventLoopWorkersRef : IO.Ref (List (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef ([] : List (_root_.Task (Except IO.Error Unit)))

initialize userAgentWorkerRef : IO.Ref (Option (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef (none : Option (_root_.Task (Except IO.Error Unit)))

initialize fetchWorkerRef : IO.Ref (Option (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef (none : Option (_root_.Task (Except IO.Error Unit)))

def trySendOnRef
    (channelRef : IO.Ref (Option (Std.CloseableChannel α)))
    (message : α) :
    IO Unit := do
  let some channel := (← channelRef.get) | return ()
  let _ ← channel.trySend message
  pure ()

def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

def enqueueUserAgentMessage (message : UserAgentTaskMessage) : IO Unit := do
  trySendOnRef userAgentMessageChannelRef message

def enqueueFetchMessage (message : FetchTaskMessage) : IO Unit := do
  trySendOnRef fetchMessageChannelRef message

def enqueueEventLoopMessage
    (eventLoopId : Nat)
    (message : EventLoopTaskMessage) :
    IO Unit := do
  let channels ← eventLoopMessageChannelsRef.get
  let some channel := channels.get? eventLoopId | pure ()
  let _ ← channel.trySend message
  pure ()

def registerDocumentEventLoop
    (documentId : Nat)
    (eventLoopId : Nat) :
    IO Unit := do
  documentEventLoopIdsRef.modify (·.insert documentId eventLoopId)

def unregisterDocumentEventLoop
    (documentId : Nat) :
    IO Unit := do
  documentEventLoopIdsRef.modify (·.erase documentId)

def eventLoopIdForDocument?
    (documentId : Nat) :
    IO (Option Nat) := do
  pure ((← documentEventLoopIdsRef.get).get? documentId)

def allocateDocumentFetchId : IO Nat := do
  let nextFetchId ← nextDocumentFetchIdRef.get
  nextDocumentFetchIdRef.set (nextFetchId + 1)
  pure (nextFetchId * 2 + 1)

def registerDocumentFetchRecipient
    (fetchId : Nat)
    (eventLoopId : Nat)
    (handler : RustNetHandlerPointer) :
    IO Unit := do
  documentFetchRecipientsRef.modify (·.insert fetchId (eventLoopId, handler))

def takeDocumentFetchRecipient?
    (fetchId : Nat) :
    IO (Option (Nat × RustNetHandlerPointer)) := do
  let recipients ← documentFetchRecipientsRef.get
  let recipient? := recipients.get? fetchId
  documentFetchRecipientsRef.modify (·.erase fetchId)
  pure recipient?

def handleEventLoopRuntimeEffect
    (state : EventLoopTaskState)
    (runtimeEffect : EventLoopRuntimeEffect) :
    IO Unit := do
  match runtimeEffect with
  | .createEmptyDocument documentId =>
      registerDocumentEventLoop documentId.id state.eventLoop.id
      contentProcessCreateEmptyDocument state.contentProcess.raw (USize.ofNat documentId.id)
  | .createLoadedDocument documentId url body =>
      registerDocumentEventLoop documentId.id state.eventLoop.id
      contentProcessCreateLoadedDocument
        state.contentProcess.raw
        (USize.ofNat documentId.id)
        url
        body
  | .destroyDocument documentId =>
      unregisterDocumentEventLoop documentId.id
      contentProcessDestroyDocument state.contentProcess.raw (USize.ofNat documentId.id)
  | .updateTheRendering documentId =>
      contentProcessUpdateTheRendering
        state.contentProcess.raw
        (USize.ofNat documentId.id)
  | .dispatchEvent events =>
      contentProcessDispatchEvent
        state.contentProcess.raw
        (encodeDispatchEventBatch events)
  | .runBeforeUnload documentId checkId =>
      contentProcessRunBeforeUnload
        state.contentProcess.raw
        (USize.ofNat documentId.id)
        (USize.ofNat checkId)
  | .startDocumentFetch handler request =>
      let fetchId ← allocateDocumentFetchId
      registerDocumentFetchRecipient fetchId state.eventLoop.id handler
      enqueueFetchMessage <| .startDocumentFetch { fetchId, request }
  | .documentFetchCompletion handler resolvedUrl body =>
      contentProcessCompleteDocumentFetch
        state.contentProcess.raw
        handler.raw
        resolvedUrl
        body

def ensureEventLoopWorker
    (eventLoop : EventLoop) :
    IO Unit := do
  let channels ← eventLoopMessageChannelsRef.get
  match channels.get? eventLoop.id with
  | some _ =>
      pure ()
  | none =>
      let channel ← Std.CloseableChannel.new
      eventLoopMessageChannelsRef.modify (·.insert eventLoop.id channel)
      eventLoopShutdownChannelsRef.modify (fun shutdownChannels => channel :: shutdownChannels)
      let worker ← IO.asTask <| do
        try
          FormalWeb.runEventLoop handleEventLoopRuntimeEffect channel { eventLoop := eventLoop }
        finally
          eventLoopMessageChannelsRef.modify (·.erase eventLoop.id)
      eventLoopWorkersRef.modify (fun workers => worker :: workers)

@[export userAgentNoteRenderingOpportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  enqueueUserAgentMessage .renderingOpportunity

@[export handleRuntimeMessage]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some userAgentMessage := FormalWeb.userAgentTaskMessageOfString? message | pure ()
  enqueueUserAgentMessage userAgentMessage

@[export startDocumentFetch]
def startDocumentFetchFromRust
  (eventLoopId : USize)
    (handlerPointer : USize)
    (url : String)
    (method : String)
    (body : String) :
    IO Unit := do
  let request : NavigationRequest := {
    url
    method
    body := if body.isEmpty then none else some body
  }
  enqueueEventLoopMessage
    eventLoopId.toNat
    (.documentFetchRequested { raw := handlerPointer } request)

@[export startNavigation]
def startNavigationFromRust
    (documentId : USize)
    (destinationURL : String)
    (targetName : String)
    (userInvolvement : String)
    (noopener : USize) :
    IO Unit := do
  let parsedUserInvolvement :=
    if userInvolvement = "activation" then
      UserNavigationInvolvement.activation
    else if userInvolvement = "browser-ui" then
      UserNavigationInvolvement.browserUI
    else
      UserNavigationInvolvement.none
  enqueueUserAgentMessage <|
    .navigateRequested
      documentId.toNat
      destinationURL
      targetName
      parsedUserInvolvement
      (noopener.toNat != 0)

@[export completeBeforeUnload]
def completeBeforeUnloadFromRust
    (documentId : USize)
    (checkId : USize)
    (canceled : USize) :
    IO Unit := do
  enqueueUserAgentMessage <|
    .beforeUnloadCompleted
      documentId.toNat
      checkId.toNat
      (canceled.toNat != 0)

@[export finalizeNavigation]
def finalizeNavigationFromRust
    (documentId : USize)
    (url : String) :
    IO Unit := do
  enqueueUserAgentMessage <|
    .finalizeNavigation
      documentId.toNat
      url

@[export runNextEventLoopTask]
def runNextEventLoopTaskFromRust
    (eventLoopId : USize) :
    IO Unit := do
  enqueueEventLoopMessage eventLoopId.toNat .runNextTask

@[export abortNavigation]
def abortNavigationFromRust
    (documentId : USize) :
    IO Unit := do
  enqueueUserAgentMessage <|
    .abortNavigationRequested documentId.toNat

def kernelStarted : IO Bool := do
  return (← userAgentMessageChannelRef.get).isSome

@[export formalWebStartKernel]
def startKernel : IO Unit := do
  if ← kernelStarted then
    throw <| IO.userError "formal-web Lean kernel is already running"

  let userAgentChannel ← Std.CloseableChannel.new
  let fetchChannel ← Std.CloseableChannel.new

  userAgentMessageChannelRef.set (some userAgentChannel)
  fetchMessageChannelRef.set (some fetchChannel)
  eventLoopMessageChannelsRef.set
    (Std.TreeMap.empty : Std.TreeMap Nat (Std.CloseableChannel EventLoopTaskMessage) compare)
  documentEventLoopIdsRef.set (Std.TreeMap.empty : Std.TreeMap Nat Nat compare)
  documentFetchRecipientsRef.set
    (Std.TreeMap.empty : Std.TreeMap Nat (Nat × RustNetHandlerPointer) compare)
  nextDocumentFetchIdRef.set 0
  eventLoopShutdownChannelsRef.set ([] : List (Std.CloseableChannel EventLoopTaskMessage))
  eventLoopWorkersRef.set ([] : List (_root_.Task (Except IO.Error Unit)))

  let userAgentWorker ← IO.asTask <|
    FormalWeb.runUserAgent
      userAgentChannel
      (fun message => trySendAndForget fetchChannel message)
      ensureEventLoopWorker
      enqueueEventLoopMessage
  let fetchWorker ← IO.asTask <|
    FormalWeb.runFetch fetchChannel fun notification => do
      match notification with
      | .fetchCompleted fetchId response =>
          match ← takeDocumentFetchRecipient? fetchId with
          | some (eventLoopId, handler) =>
              enqueueEventLoopMessage
                eventLoopId
                (.queueDocumentFetchCompletion handler response.url response.body)
          | none =>
              trySendAndForget userAgentChannel (.fetchCompleted fetchId response)

  userAgentWorkerRef.set (some userAgentWorker)
  fetchWorkerRef.set (some fetchWorker)

@[export formalWebShutdownKernel]
def shutdownKernel : IO Unit := do
  let userAgentChannel? ← userAgentMessageChannelRef.get
  let fetchChannel? ← fetchMessageChannelRef.get
  let eventLoopShutdownChannels ← eventLoopShutdownChannelsRef.get
  let eventLoopWorkers ← eventLoopWorkersRef.get
  let userAgentWorker? ← userAgentWorkerRef.get
  let fetchWorker? ← fetchWorkerRef.get

  userAgentMessageChannelRef.set (none : Option (Std.CloseableChannel UserAgentTaskMessage))
  fetchMessageChannelRef.set (none : Option (Std.CloseableChannel FetchTaskMessage))
  eventLoopMessageChannelsRef.set
    (Std.TreeMap.empty : Std.TreeMap Nat (Std.CloseableChannel EventLoopTaskMessage) compare)
  documentEventLoopIdsRef.set (Std.TreeMap.empty : Std.TreeMap Nat Nat compare)
  documentFetchRecipientsRef.set
    (Std.TreeMap.empty : Std.TreeMap Nat (Nat × RustNetHandlerPointer) compare)
  nextDocumentFetchIdRef.set 0
  eventLoopShutdownChannelsRef.set ([] : List (Std.CloseableChannel EventLoopTaskMessage))
  eventLoopWorkersRef.set ([] : List (_root_.Task (Except IO.Error Unit)))
  userAgentWorkerRef.set (none : Option (_root_.Task (Except IO.Error Unit)))
  fetchWorkerRef.set (none : Option (_root_.Task (Except IO.Error Unit)))

  if let some fetchChannel := fetchChannel? then
    fetchChannel.close

  for channel in eventLoopShutdownChannels do
    channel.close

  if let some fetchWorker := fetchWorker? then
    let _ ← IO.wait fetchWorker

  for worker in eventLoopWorkers do
    let _ ← IO.wait worker

  if let some userAgentChannel := userAgentChannel? then
    userAgentChannel.close

  if let some userAgentWorker := userAgentWorker? then
    let _ ← IO.wait userAgentWorker

def runKernel : IO Unit := do
  startKernel
  try
    FormalWeb.runEmbedderEventLoop ()
  finally
    shutdownKernel

end FormalWeb
