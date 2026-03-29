import FormalWeb.FFI
import FormalWeb.Runtime
import Std.Sync.Channel

open FormalWeb

initialize runtimeMessageChannelRef : IO.Ref (Option (Std.CloseableChannel RuntimeMessage)) ←
  IO.mkRef (none : Option (Std.CloseableChannel RuntimeMessage))

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

def runtimeMessageOfString? (message : String) : Option RuntimeMessage := do
  let messagePrefix := "FreshTopLevelTraversable|"
  if message.startsWith messagePrefix then
    some (.freshTopLevelTraversable (message.drop messagePrefix.length).toString)
  else
    none

def notifyStartupTraversableReady
    (userAgent : FormalWeb.UserAgent)
    (traversableId : Nat) :
    IO Unit := do
  let some _html := FormalWeb.startupTraversableReadyHtml? userAgent traversableId | pure ()
  FormalWeb.sendRuntimeMessage "NewTopLevelTraversable"

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
    (runtimeChannel : Std.CloseableChannel RuntimeMessage)
    (controllerId : Nat)
    (request : FormalWeb.NavigationRequest) :
    IO Unit := do
  spawnDetached do
    let response ← fetchResponseForRequest request
    trySendAndForget runtimeChannel (.fetchCompleted controllerId response)

def handleRuntimeMessage
    (runtimeChannel : Std.CloseableChannel RuntimeMessage)
    (state : RuntimeState)
    (message : RuntimeMessage) :
    IO RuntimeState := do
  let nextState := runtimeExec state [message]
  let result := handleRuntimeMessagePure state message
  if let some error := result.error then
    IO.eprintln s!"handleRuntimeMessagePure failed: {error}"
  for spawnedFetchTask in result.spawnedFetchTasks do
    spawnFetchRequestTask runtimeChannel spawnedFetchTask.controllerId spawnedFetchTask.request
  match message with
  | .freshTopLevelTraversable _ =>
      pure ()
  | .renderingOpportunity =>
      let some traversableId := nextState.startupTraversableId | pure ()
      FormalWeb.noteRenderingOpportunity nextState.userAgent traversableId
  | .fetchCompleted _ _ =>
      if result.sentNewTopLevelTraversable then
        let some traversableId := nextState.startupTraversableId | pure ()
        notifyStartupTraversableReady nextState.userAgent traversableId
  pure nextState

partial def runtimeMessageLoop
    (channel : Std.CloseableChannel RuntimeMessage)
    (state : RuntimeState := default) :
    IO Unit := do
  let some message ← recvCloseableChannel? channel | pure ()
  let state ← handleRuntimeMessage channel state message
  runtimeMessageLoop channel state

def enqueueRuntimeMessage (message : RuntimeMessage) : IO Unit := do
  trySendOnRef runtimeMessageChannelRef message


@[export formal_web_user_agent_note_rendering_opportunity]
def userAgentNoteRenderingOpportunity (message : String) : IO Unit := do
  let _ := message
  spawnDetached <| enqueueRuntimeMessage .renderingOpportunity

@[export formal_web_handle_runtime_message]
def handleRuntimeMessageFromRust (message : String) : IO Unit := do
  let some runtimeMessage := runtimeMessageOfString? message | pure ()
  spawnDetached <| enqueueRuntimeMessage runtimeMessage

def main : IO Unit := do
  let runtimeMessageChannel ← Std.CloseableChannel.new
  runtimeMessageChannelRef.set (some runtimeMessageChannel)
  let worker ← IO.asTask (runtimeMessageLoop runtimeMessageChannel)
  FormalWeb.runWinitEventLoop ()
  runtimeMessageChannelRef.set (none : Option (Std.CloseableChannel RuntimeMessage))
  runtimeMessageChannel.close
  let _ ← IO.wait worker
  pure ()
