import FormalWeb.FFI
import FormalWeb.Fetch
import FormalWeb.UserAgent
import Std.Sync.Channel

open FormalWeb

initialize userAgentMessageChannelRef : IO.Ref (Option (Std.CloseableChannel UserAgentTaskMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel UserAgentTaskMessage))

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


@[export formal_web_user_agent_note_rendering_opportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  spawnDetached <| enqueueUserAgentMessage .renderingOpportunity

@[export formal_web_handle_runtime_message]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some userAgentMessage := FormalWeb.userAgentTaskMessageOfString? message | pure ()
  spawnDetached <| enqueueUserAgentMessage userAgentMessage

def main : IO Unit := do
  let userAgentChannel ← Std.CloseableChannel.new
  let fetchChannel ← Std.CloseableChannel.new
  userAgentMessageChannelRef.set (some userAgentChannel)
  let userAgentWorker ← IO.asTask <|
    FormalWeb.runUserAgent userAgentChannel (fun message => trySendAndForget fetchChannel message)
  let fetchWorker ← IO.asTask <|
    FormalWeb.runFetch fetchChannel fun notification =>
      match notification with
      | .fetchCompleted navigationId response =>
          trySendAndForget userAgentChannel (.fetchCompleted navigationId response)
  FormalWeb.runWinitEventLoop ()
  userAgentMessageChannelRef.set (none : Option (Std.CloseableChannel UserAgentTaskMessage))
  fetchChannel.close
  userAgentChannel.close
  let _ ← IO.wait fetchWorker
  let _ ← IO.wait userAgentWorker
  pure ()
