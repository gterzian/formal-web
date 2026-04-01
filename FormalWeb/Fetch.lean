import Std.Data.TreeMap
import Std.Sync.Channel
import FormalWeb.FFI
import FormalWeb.Navigation

namespace FormalWeb

deriving instance Repr for ByteArray

/-- https://fetch.spec.whatwg.org/#fetch-controller -/
structure FetchController where
  /-- Model-local identifier for https://fetch.spec.whatwg.org/#fetch-controller -/
  id : Nat
  /-- https://fetch.spec.whatwg.org/#fetch-controller-state -/
  state : String := "ongoing"
deriving Repr, DecidableEq

/-- Model-local bridge from an HTML navigation wait to https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetchRequest where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- Model-local reference back to https://html.spec.whatwg.org/multipage/#navigation-params-id -/
  navigationId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

structure DocumentFetchRequest where
  /-- Model-local identifier corresponding to https://fetch.spec.whatwg.org/#fetch-controller -/
  fetchId : Nat
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
deriving Repr, DecidableEq

/-- Model-local pending state for a started https://fetch.spec.whatwg.org/#concept-fetch. -/
structure PendingFetch where
  /-- https://fetch.spec.whatwg.org/#concept-request -/
  request : NavigationRequest
  /-- https://fetch.spec.whatwg.org/#fetch-params-controller -/
  controller : FetchController
deriving Repr, DecidableEq

structure FetchResponse where
  /-- https://fetch.spec.whatwg.org/#concept-response-url -/
  url : String
  /-- https://fetch.spec.whatwg.org/#concept-response-status -/
  status : Nat := 200
  /-- Minimal MIME type surface for loading-a-document dispatch. -/
  contentType : String := ""
  /-- Raw response bytes for Rust-side document fetch consumers. -/
  body : ByteArray := ByteArray.empty
deriving Repr, DecidableEq

/-- Model-local top-level state for fetch processing. -/
structure Fetch where
  /-- Model-local map of started fetches keyed by controller identifier. -/
  pendingFetches : Std.TreeMap Nat PendingFetch := Std.TreeMap.empty
deriving Repr

instance : Inhabited Fetch where
  default := {}

namespace Fetch

def pendingFetch?
    (fetch : Fetch)
    (controllerId : Nat) :
    Option PendingFetch :=
  fetch.pendingFetches.get? controllerId

end Fetch

/-- https://fetch.spec.whatwg.org/#fetch-scheme -/
def isFetchScheme (url : String) : Bool :=
  url.startsWith "http://" || url.startsWith "https://" || url.startsWith "file://"

/-- https://fetch.spec.whatwg.org/#concept-fetch -/
def conceptFetch
    (fetch : Fetch)
    (fetchId : Nat)
    (request : NavigationRequest) :
    Fetch × FetchController :=
  let controller : FetchController := {
    id := fetchId
  }
  let pendingFetch : PendingFetch := {
    request
    controller
  }
  let fetch := {
    fetch with
      pendingFetches := fetch.pendingFetches.insert controller.id pendingFetch
  }
  (fetch, controller)

def conceptNavigationFetch
    (fetch : Fetch)
    (pendingRequest : PendingFetchRequest) :
    Fetch × FetchController :=
  conceptFetch fetch pendingRequest.fetchId pendingRequest.request

def conceptDocumentFetch
    (fetch : Fetch)
    (pendingRequest : DocumentFetchRequest) :
    Fetch × FetchController :=
  conceptFetch fetch pendingRequest.fetchId pendingRequest.request

def navigationResponseOfFetchResponse
    (response : FetchResponse) :
    NavigationResponse :=
  {
    url := response.url
    status := response.status
    contentType := if response.contentType.isEmpty then "text/html" else response.contentType
    body := (String.fromUTF8? response.body).getD ""
  }

/-- Model the point where a pending fetch completes and leaves the fetch set. -/
def completeFetch
    (fetch : Fetch)
    (controllerId : Nat) :
    Fetch × Option PendingFetch :=
  let pendingFetch := fetch.pendingFetches.get? controllerId
  let pendingFetches :=
    match pendingFetch with
    | some _ => fetch.pendingFetches.erase controllerId
    | none => fetch.pendingFetches
  let fetch := {
    fetch with
      pendingFetches
  }
  (fetch, pendingFetch)

inductive FetchTaskMessage where
  | startFetch (pendingRequest : PendingFetchRequest)
  | startDocumentFetch (pendingRequest : DocumentFetchRequest)
  | finishFetch (controllerId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

inductive FetchNotification where
  | fetchCompleted (fetchId : Nat) (response : FetchResponse)
deriving Repr, DecidableEq

structure SpawnedFetchTask where
  controllerId : Nat
  request : NavigationRequest
deriving Repr, DecidableEq

inductive FetchTaskResult where
  | stateOnly (state : Fetch)
  | notify (state : Fetch) (notifications : List FetchNotification)
  | scheduleFetchTasks (state : Fetch) (toSpawnFetchTasks : List SpawnedFetchTask)
deriving Repr

namespace FetchTaskResult

def state : FetchTaskResult → Fetch
  | .stateOnly state => state
  | .notify state _ => state
  | .scheduleFetchTasks state _ => state

def notifications : FetchTaskResult → List FetchNotification
  | .stateOnly _ => []
  | .notify _ notifications => notifications
  | .scheduleFetchTasks _ _ => []

def toSpawnFetchTasks : FetchTaskResult → List SpawnedFetchTask
  | .stateOnly _ => []
  | .notify _ _ => []
  | .scheduleFetchTasks _ toSpawnFetchTasks => toSpawnFetchTasks

end FetchTaskResult

def handleFetchTaskMessagePure
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    FetchTaskResult :=
  match message with
  | .startFetch pendingRequest =>
      let (fetch, controller) := conceptNavigationFetch fetch pendingRequest
      .scheduleFetchTasks fetch [{ controllerId := controller.id, request := pendingRequest.request }]
  | .startDocumentFetch pendingRequest =>
      let (fetch, controller) := conceptDocumentFetch fetch pendingRequest
      .scheduleFetchTasks fetch [{ controllerId := controller.id, request := pendingRequest.request }]
  | .finishFetch controllerId response =>
      let (fetch, pendingFetch?) := completeFetch fetch controllerId
      match pendingFetch? with
      | none =>
          .stateOnly fetch
      | some _pendingFetch =>
        .notify fetch [.fetchCompleted controllerId response]

def fetchTaskStep
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    Fetch :=
  (handleFetchTaskMessagePure fetch message).state

def fetchTaskExec
    (fetch : Fetch)
    (messages : List FetchTaskMessage) :
    Fetch :=
  messages.foldl fetchTaskStep fetch

private def recvCloseableChannel?
    (channel : Std.CloseableChannel α) :
    IO (Option α) := do
  let receiveTask ← channel.recv
  IO.wait receiveTask

private def trySendAndForget
    (channel : Std.CloseableChannel α)
    (message : α) :
    IO Unit := do
  let _ ← channel.trySend message
  pure ()

private def spawnDetached (action : IO Unit) : IO Unit := do
  let _ ← IO.asTask action
  pure ()

private def withTempOutputPath
    (action : System.FilePath → IO α) :
    IO α := do
  let output ← IO.Process.output { cmd := "mktemp" }
  if output.exitCode != 0 then
    throw <| IO.userError s!"mktemp failed: {output.stderr.trimAscii.toString}"
  let outputPath : System.FilePath := output.stdout.trimAscii.toString
  try
    action outputPath
  finally
    let _ ← IO.Process.output {
      cmd := "rm"
      args := #["-f", outputPath.toString]
    }
    pure ()

private def curlArgsForRequest
    (outputPath : System.FilePath)
    (request : NavigationRequest) :
    Array String :=
  let args := #[
    "-L",
    "--silent",
    "--show-error",
    "-o",
    outputPath.toString,
    "--write-out",
    "%{http_code}\n%{content_type}\n%{url_effective}"
  ]
  let args :=
    if request.method = "GET" then
      args
    else
      args ++ #["-X", request.method]
  let args :=
    match request.body with
    | some body => args ++ #["--data-binary", body]
    | none => args
  args ++ #[request.url]

def fetchResponseForRequest
    (request : NavigationRequest) :
    IO FetchResponse := do
  try
    withTempOutputPath fun outputPath => do
      let output ← IO.Process.output {
        cmd := "curl"
        args := curlArgsForRequest outputPath request
      }
      if output.exitCode == 0 then
        let body ← IO.FS.readBinFile outputPath
        let metadata := output.stdout.splitOn "\n"
        let (statusLine, contentTypeLine, resolvedUrlLine) :=
          match metadata with
          | statusLine :: contentTypeLine :: resolvedUrlLine :: _ =>
              (statusLine, contentTypeLine, resolvedUrlLine)
          | _ =>
              ("200", "", request.url)
        let resolvedUrlLine := resolvedUrlLine.trimAscii.toString
        let statusLine := statusLine.trimAscii.toString
        let contentTypeLine := contentTypeLine.trimAscii.toString
        pure {
          url := if resolvedUrlLine.isEmpty then request.url else resolvedUrlLine
          status := (statusLine.toNat?).getD 200
          contentType := contentTypeLine
          body
        }
      else
        pure {
          url := request.url
          status := 599
          contentType := "text/html"
          body :=
            s!"<!DOCTYPE html><html><head><title>Fetch failed</title></head><body><pre>{output.stderr}</pre></body></html>".toUTF8
        }
  catch error =>
    pure {
      url := request.url
      status := 599
      contentType := "text/html"
      body :=
        s!"<!DOCTYPE html><html><head><title>Fetch failed</title></head><body><pre>{error.toString}</pre></body></html>".toUTF8
    }

private def spawnFetchRequestTask
    (channel : Std.CloseableChannel FetchTaskMessage)
    (controllerId : Nat)
    (request : NavigationRequest) :
    IO Unit := do
  spawnDetached do
    let response ← fetchResponseForRequest request
    trySendAndForget channel (.finishFetch controllerId response)

def runFetchMessage
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch)
    (message : FetchTaskMessage) :
    IO Fetch := do
  let result := handleFetchTaskMessagePure fetch message
  match result with
  | .stateOnly nextFetch =>
      pure nextFetch
  | .notify nextFetch notifications =>
      for notification in notifications do
        onNotification notification
      pure nextFetch
  | .scheduleFetchTasks nextFetch toSpawnFetchTasks =>
      for toSpawnFetchTask in toSpawnFetchTasks do
        spawnFetchRequestTask channel toSpawnFetchTask.controllerId toSpawnFetchTask.request
      pure nextFetch

/-- Process fetch-task messages until the channel is closed. -/
partial def runFetch
    (channel : Std.CloseableChannel FetchTaskMessage)
    (onNotification : FetchNotification -> IO Unit)
    (fetch : Fetch := default) :
    IO Unit := do
  let nextMessage? ← recvCloseableChannel? channel
  match nextMessage? with
  | none =>
      pure ()
  | some message =>
      let nextFetch ← runFetchMessage channel onNotification fetch message
      runFetch channel onNotification nextFetch

end FormalWeb
