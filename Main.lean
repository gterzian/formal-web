import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.UserAgent
import Std.Sync.Channel

open FormalWeb

initialize userAgentMessageChannelRef : IO.Ref (Option (Std.CloseableChannel UserAgentTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel UserAgentTaskMessage))

initialize fetchMessageChannelRef : IO.Ref (Option (Std.CloseableChannel FetchTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel FetchTaskMessage))

def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

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

def spawnDetached (action : IO Unit) : IO Unit := do
  let _ ← IO.asTask action
  pure ()

def enqueueUserAgentMessage (message : UserAgentTaskMessage) : IO Unit := do
  trySendOnRef userAgentMessageChannelRef message

def enqueueFetchMessage (message : FetchTaskMessage) : IO Unit := do
  trySendOnRef fetchMessageChannelRef message


@[export formal_web_user_agent_note_rendering_opportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  spawnDetached <| enqueueUserAgentMessage .renderingOpportunity

@[export formal_web_handle_runtime_message]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some userAgentMessage := FormalWeb.userAgentTaskMessageOfString? message | pure ()
  spawnDetached <| enqueueUserAgentMessage userAgentMessage

@[export formal_web_start_document_fetch]
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
  spawnDetached <| enqueueUserAgentMessage <|
    .documentFetchRequested { raw := handlerPointer } request

def main : IO Unit := do
  let userAgentChannel ← Std.CloseableChannel.new
  let fetchChannel ← Std.CloseableChannel.new
  userAgentMessageChannelRef.set (some userAgentChannel)
  fetchMessageChannelRef.set (some fetchChannel)
  let userAgentWorker ← IO.asTask <|
    FormalWeb.runUserAgent userAgentChannel (fun message => trySendAndForget fetchChannel message)
  let fetchWorker ← IO.asTask <|
    FormalWeb.runFetch fetchChannel fun notification =>
      match notification with
      | .fetchCompleted fetchId response =>
          trySendAndForget userAgentChannel (.fetchCompleted fetchId response)
  FormalWeb.runWinitEventLoop ()
  userAgentMessageChannelRef.set (none : Option (Std.CloseableChannel UserAgentTaskMessage))
  fetchMessageChannelRef.set (none : Option (Std.CloseableChannel FetchTaskMessage))
  fetchChannel.close
  userAgentChannel.close
  let _ ← IO.wait fetchWorker
  let _ ← IO.wait userAgentWorker
  pure ()
