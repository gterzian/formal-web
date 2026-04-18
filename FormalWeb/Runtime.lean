import FormalWeb.EventLoop
import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.Timer
import FormalWeb.UserAgent
import Std.Sync.Channel

namespace FormalWeb

initialize userAgentMessageChannelRef : IO.Ref (Option (Std.CloseableChannel UserAgentRuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel UserAgentRuntimeMessage))

initialize fetchMessageChannelRef : IO.Ref (Option (Std.CloseableChannel FetchRuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel FetchRuntimeMessage))

initialize timerMessageChannelRef : IO.Ref (Option (Std.CloseableChannel TimerRuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel TimerRuntimeMessage))

initialize userAgentWorkerRef : IO.Ref (Option (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef (none : Option (_root_.Task (Except IO.Error Unit)))

initialize fetchWorkerRef : IO.Ref (Option (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef (none : Option (_root_.Task (Except IO.Error Unit)))

initialize timerWorkerRef : IO.Ref (Option (_root_.Task (Except IO.Error Unit))) ←
  IO.mkRef (none : Option (_root_.Task (Except IO.Error Unit)))

private def trySendOnRef
    (channelRef : IO.Ref (Option (Std.CloseableChannel α)))
    (message : α) :
    IO Unit := do
  let some channel := (← channelRef.get) | return ()
  let _ ← channel.trySend message
  pure ()

private def runtimeStarted : IO Bool := do
  pure (← userAgentMessageChannelRef.get).isSome

private def enqueueUserAgentRuntimeMessage
    (message : UserAgentRuntimeMessage) :
    IO Unit := do
  trySendOnRef userAgentMessageChannelRef message

@[export userAgentNoteRenderingOpportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  enqueueUserAgentRuntimeMessage (.task .renderingOpportunity)

@[export handleRuntimeMessage]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some userAgentMessage := FormalWeb.userAgentTaskMessageOfString? message | pure ()
  enqueueUserAgentRuntimeMessage (.task userAgentMessage)

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
  enqueueUserAgentRuntimeMessage <|
    .eventLoop
      eventLoopId.toNat
      (.documentFetchRequested { raw := handlerPointer } request)

@[export scheduleWindowTimer]
def scheduleWindowTimerFromRust
    (eventLoopId : USize)
    (documentId : USize)
    (timerId : USize)
    (timerKey : USize)
    (timeoutMs : USize)
    (nestingLevel : USize) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .eventLoop
      eventLoopId.toNat
      (.scheduleWindowTimer
        { id := documentId.toNat }
        timerId.toNat
        timerKey.toNat
        timeoutMs.toNat
        nestingLevel.toNat)

@[export clearWindowTimer]
def clearWindowTimerFromRust
    (eventLoopId : USize)
    (timerKey : USize) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .eventLoop eventLoopId.toNat (.clearTimeout timerKey.toNat)

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
  enqueueUserAgentRuntimeMessage <|
    .task
      (.navigateRequested
        documentId.toNat
        destinationURL
        targetName
        parsedUserInvolvement
        (noopener.toNat != 0))

@[export completeBeforeUnload]
def completeBeforeUnloadFromRust
    (documentId : USize)
    (checkId : USize)
    (canceled : USize) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .task
      (.beforeUnloadCompleted
        documentId.toNat
        checkId.toNat
        (canceled.toNat != 0))

@[export finalizeNavigation]
def finalizeNavigationFromRust
    (documentId : USize)
    (url : String) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .task
      (.finalizeNavigation documentId.toNat url)

@[export runNextEventLoopTask]
def runNextEventLoopTaskFromRust
    (eventLoopId : USize) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .eventLoop eventLoopId.toNat .runNextTask

@[export abortNavigation]
def abortNavigationFromRust
    (documentId : USize) :
    IO Unit := do
  enqueueUserAgentRuntimeMessage <|
    .task (.abortNavigationRequested documentId.toNat)

@[export formalWebStartKernel]
def startKernel : IO Unit := do
  if ← runtimeStarted then
    throw <| IO.userError "formal-web Lean kernel is already running"

  let userAgentChannel ← Std.CloseableChannel.new
  let fetchChannel ← Std.CloseableChannel.new
  let timerChannel ← Std.CloseableChannel.new

  userAgentMessageChannelRef.set (some userAgentChannel)
  fetchMessageChannelRef.set (some fetchChannel)
  timerMessageChannelRef.set (some timerChannel)

  let fetchWorker ← IO.asTask <|
    FormalWeb.runFetchRuntime fetchChannel fun notification => do
      match notification with
      | .fetchCompleted fetchId response =>
          let _ ← userAgentChannel.trySend (.task (.fetchCompleted fetchId response))
          pure ()
  let timerWorker ← IO.asTask <|
    FormalWeb.runTimerRuntime timerChannel
  let userAgentWorker ← IO.asTask <|
    FormalWeb.runUserAgent userAgentChannel fetchChannel timerChannel

  userAgentWorkerRef.set (some userAgentWorker)
  fetchWorkerRef.set (some fetchWorker)
  timerWorkerRef.set (some timerWorker)

@[export formalWebShutdownKernel]
def shutdownKernel : IO Unit := do
  let userAgentChannel? ← userAgentMessageChannelRef.get
  let fetchChannel? ← fetchMessageChannelRef.get
  let timerChannel? ← timerMessageChannelRef.get
  let userAgentWorker? ← userAgentWorkerRef.get
  let fetchWorker? ← fetchWorkerRef.get
  let timerWorker? ← timerWorkerRef.get

  userAgentMessageChannelRef.set (none : Option (Std.CloseableChannel UserAgentRuntimeMessage))
  fetchMessageChannelRef.set (none : Option (Std.CloseableChannel FetchRuntimeMessage))
  timerMessageChannelRef.set (none : Option (Std.CloseableChannel TimerRuntimeMessage))
  userAgentWorkerRef.set (none : Option (_root_.Task (Except IO.Error Unit)))
  fetchWorkerRef.set (none : Option (_root_.Task (Except IO.Error Unit)))
  timerWorkerRef.set (none : Option (_root_.Task (Except IO.Error Unit)))

  if let some userAgentChannel := userAgentChannel? then
    userAgentChannel.close

  if let some userAgentWorker := userAgentWorker? then
    let _ ← IO.wait userAgentWorker

  if let some fetchChannel := fetchChannel? then
    fetchChannel.close

  if let some timerChannel := timerChannel? then
    timerChannel.close

  if let some fetchWorker := fetchWorker? then
    let _ ← IO.wait fetchWorker

  if let some timerWorker := timerWorker? then
    let _ ← IO.wait timerWorker

def runKernel : IO Unit := do
  startKernel
  try
    FormalWeb.runEmbedderEventLoop ()
  finally
    shutdownKernel

end FormalWeb
