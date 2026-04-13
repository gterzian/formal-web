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
      let worker ← IO.asTask <| FormalWeb.runEventLoop channel { eventLoop := eventLoop }
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
  enqueueUserAgentMessage <|
    .documentFetchRequested { raw := handlerPointer } request

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

@[export runNextEventLoopTask]
def runNextEventLoopTaskFromRust
    (eventLoopId : USize) :
    IO Unit := do
  enqueueUserAgentMessage <|
    .runNextEventLoopTask eventLoopId.toNat

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
  eventLoopShutdownChannelsRef.set ([] : List (Std.CloseableChannel EventLoopTaskMessage))
  eventLoopWorkersRef.set ([] : List (_root_.Task (Except IO.Error Unit)))

  let userAgentWorker ← IO.asTask <|
    FormalWeb.runUserAgent
      userAgentChannel
      (fun message => trySendAndForget fetchChannel message)
      ensureEventLoopWorker
      enqueueEventLoopMessage
  let fetchWorker ← IO.asTask <|
    FormalWeb.runFetch fetchChannel fun notification =>
      match notification with
      | .fetchCompleted fetchId response =>
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
